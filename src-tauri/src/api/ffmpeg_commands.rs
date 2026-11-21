// ==============================================================================
// OWNERSHIP:
//   IPC for the FFmpeg that video conversion needs: is it there, and download
//   it (once, with the user's consent asked by the frontend beforehand).
// ==============================================================================
use crate::core::app_error::AppError;
use crate::core::handlers::video::ffmpeg::{self, Source};
use crate::core::handlers::video::install::{self, Phase, PIN};
use serde::Serialize;
use std::sync::Mutex;
use tauri::ipc::Channel;
use tokio_util::sync::CancellationToken;

/// The download in progress, if any, so the user can stop it.
static RUNNING: Mutex<Option<CancellationToken>> = Mutex::new(None);

#[derive(Serialize)]
pub struct FfmpegStatusDto {
    pub available: bool,
    /// "managed" (downloaded by the app), "system" (already on the machine)
    /// or "override".
    pub source: Option<&'static str>,
    /// The ffmpeg executable in use, for diagnosis.
    pub path: Option<String>,
    pub version: Option<String>,
    /// False off Windows x64: the user installs FFmpeg themselves.
    pub can_install: bool,
    pub download_bytes: u64,
    pub download_url: &'static str,
    pub sha256: &'static str,
    pub label: &'static str,
    pub license: &'static str,
}

fn status() -> FfmpegStatusDto {
    let found = ffmpeg::locate();
    FfmpegStatusDto {
        available: found.is_some(),
        source: found.as_ref().map(|f| match f.source {
            Source::Managed => "managed",
            Source::System => "system",
            Source::Override => "override",
        }),
        path: found.as_ref().map(|f| f.ffmpeg.display().to_string()),
        version: found.as_ref().and_then(ffmpeg::version_line),
        can_install: install::install_supported(),
        download_bytes: PIN.size_bytes,
        download_url: PIN.url,
        sha256: PIN.sha256,
        label: PIN.label,
        license: PIN.license,
    }
}

#[tauri::command]
pub async fn ffmpeg_status() -> Result<FfmpegStatusDto, AppError> {
    tokio::task::spawn_blocking(status)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))
}

/// Downloads and verifies the pinned build. Progress goes to `on_event` as
/// `{ phase: "download" | "verify" | "extract", done, total }`.
/// `ffmpeg_install_cancel` stops it; the error then reads "CANCELLED".
#[tauri::command]
pub async fn ffmpeg_install(
    on_event: Channel<serde_json::Value>,
) -> Result<FfmpegStatusDto, AppError> {
    let cancel = CancellationToken::new();
    {
        let mut running = RUNNING
            .lock()
            .map_err(|e| AppError::Internal(e.to_string()))?;
        if running.is_some() {
            return Err(AppError::Execute(
                "FFmpeg is already being downloaded".into(),
            ));
        }
        *running = Some(cancel.clone());
    }
    let result = tokio::task::spawn_blocking(move || {
        let report = |p: install::Progress| {
            let phase = match p.phase {
                Phase::Download => "download",
                Phase::Verify => "verify",
                Phase::Extract => "extract",
            };
            let _ = on_event.send(serde_json::json!({
                "phase": phase, "done": p.done, "total": p.total
            }));
        };
        install::install(&report, &cancel)?;
        Ok(status())
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()));
    if let Ok(mut running) = RUNNING.lock() {
        *running = None;
    }
    result?
}

/// Stops a running download; `install` removes the partial files.
#[tauri::command]
pub fn ffmpeg_install_cancel() {
    if let Ok(running) = RUNNING.lock() {
        if let Some(token) = running.as_ref() {
            token.cancel();
        }
    }
}

/// The settings page the backend asked the window to show (a rename that
/// needs FFmpeg), if the window has not opened it yet.
#[tauri::command]
pub fn take_settings_request() -> Option<String> {
    crate::tray::take_settings_request()
}
