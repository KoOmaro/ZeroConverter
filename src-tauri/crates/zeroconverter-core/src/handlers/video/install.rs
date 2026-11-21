//! The one place the app touches the network: downloading a pinned LGPL
//! FFmpeg build on first video use. The archive must match the pinned SHA-256
//! before anything is extracted; a mismatch deletes it.
//!
//! Only the app builds this module (the `ffmpeg-download` feature); the
//! command line never touches the network. The pin itself is in `pin.rs`.
use std::io::{Read, Write};
use std::path::Path;

use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use super::ffmpeg::{self, Ffmpeg, READY_MARKER};
use crate::app_error::AppError;

pub use super::pin::{Pin, PIN};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Download,
    Verify,
    Extract,
}

#[derive(Debug, Clone, Copy)]
pub struct Progress {
    pub phase: Phase,
    pub done: u64,
    pub total: u64,
}

/// Files copied out of the archive. Names are matched on the last path
/// component only, so nothing in the archive can choose where it is written.
const WANTED: &[&str] = &["ffmpeg.exe", "ffprobe.exe", "LICENSE.txt"];

pub fn install_supported() -> bool {
    cfg!(all(windows, target_arch = "x86_64"))
}

fn err(msg: impl Into<String>) -> AppError {
    AppError::Execute(msg.into())
}

pub fn install(
    on_progress: &dyn Fn(Progress),
    cancel: &CancellationToken,
) -> Result<Ffmpeg, AppError> {
    if !install_supported() {
        return Err(err(
            "Automatic FFmpeg download is available on Windows x64 only. Install FFmpeg and put it on PATH.",
        ));
    }
    let dir = ffmpeg::managed_dir().ok_or_else(|| err("App data folder is not ready"))?;
    let parent = dir.parent().ok_or_else(|| err("Bad FFmpeg folder"))?;
    std::fs::create_dir_all(parent).map_err(|e| AppError::Io(e.to_string()))?;
    let staging = parent.join(format!("{}.partial", PIN.id));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| AppError::Io(e.to_string()))?;

    let result = download_and_extract(&staging, on_progress, cancel);
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
        return result.map(|_| unreachable!());
    }
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::rename(&staging, &dir).map_err(|e| AppError::Io(e.to_string()))?;
    ffmpeg::locate().ok_or_else(|| err("FFmpeg was installed but could not be started"))
}

fn download_and_extract(
    staging: &Path,
    on_progress: &dyn Fn(Progress),
    cancel: &CancellationToken,
) -> Result<(), AppError> {
    let archive = staging.join("ffmpeg.zip");
    let digest = download(&archive, on_progress, cancel)?;
    on_progress(Progress {
        phase: Phase::Verify,
        done: 1,
        total: 1,
    });
    if !digest.eq_ignore_ascii_case(PIN.sha256) {
        return Err(err(format!(
            "The downloaded FFmpeg did not match the expected checksum (got {digest}). It was deleted."
        )));
    }
    extract(&archive, staging, on_progress, cancel)?;
    std::fs::remove_file(&archive).map_err(|e| AppError::Io(e.to_string()))?;
    // The marker is written last: a half-installed folder never looks usable.
    std::fs::write(staging.join(READY_MARKER), PIN.sha256).map_err(|e| AppError::Io(e.to_string()))
}

fn download(
    to: &Path,
    on_progress: &dyn Fn(Progress),
    cancel: &CancellationToken,
) -> Result<String, AppError> {
    let tls = ureq::tls::TlsConfig::builder()
        .provider(ureq::tls::TlsProvider::NativeTls)
        .build();
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .tls_config(tls)
        .https_only(true)
        .user_agent("ZeroConverter")
        .build()
        .into();
    let response = agent
        .get(PIN.url)
        .call()
        .map_err(|e| match e {
            // Old builds are pruned upstream; a newer ZeroConverter pins a newer one.
            ureq::Error::StatusCode(404) => err(
                "This FFmpeg build is no longer offered for download. Update ZeroConverter to get a current one.",
            ),
            e => err(format!("FFmpeg download failed: {e}")),
        })?;
    let total = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(PIN.size_bytes);
    let mut reader = response.into_body().into_reader();
    let mut file = std::fs::File::create(to).map_err(|e| AppError::Io(e.to_string()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    let mut done = 0u64;
    let mut last_report = 0u64;
    loop {
        if cancel.is_cancelled() {
            return Err(AppError::Cancelled);
        }
        let n = reader
            .read(&mut buf)
            .map_err(|e| err(format!("FFmpeg download was interrupted: {e}")))?;
        if n == 0 {
            break;
        }
        // A pinned file has a known size; anything longer is not it.
        if done + n as u64 > PIN.size_bytes {
            return Err(err("FFmpeg download is larger than the pinned build"));
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n])
            .map_err(|e| AppError::Io(e.to_string()))?;
        done += n as u64;
        if done - last_report >= 512 * 1024 {
            last_report = done;
            on_progress(Progress {
                phase: Phase::Download,
                done,
                total,
            });
        }
    }
    file.flush().map_err(|e| AppError::Io(e.to_string()))?;
    on_progress(Progress {
        phase: Phase::Download,
        done,
        total: done,
    });
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

fn extract(
    archive: &Path,
    into: &Path,
    on_progress: &dyn Fn(Progress),
    cancel: &CancellationToken,
) -> Result<(), AppError> {
    let file = std::fs::File::open(archive).map_err(|e| AppError::Io(e.to_string()))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| err(format!("FFmpeg archive is unreadable: {e}")))?;
    let mut copied = 0u64;
    for i in 0..zip.len() {
        if cancel.is_cancelled() {
            return Err(AppError::Cancelled);
        }
        let mut entry = zip
            .by_index(i)
            .map_err(|e| err(format!("FFmpeg archive is unreadable: {e}")))?;
        if entry.is_dir() {
            continue;
        }
        let Some(name) = entry.name().rsplit('/').next().map(str::to_string) else {
            continue;
        };
        if !WANTED.contains(&name.as_str()) {
            continue;
        }
        let mut out =
            std::fs::File::create(into.join(&name)).map_err(|e| AppError::Io(e.to_string()))?;
        std::io::copy(&mut entry, &mut out).map_err(|e| AppError::Io(e.to_string()))?;
        copied += 1;
        on_progress(Progress {
            phase: Phase::Extract,
            done: copied,
            total: WANTED.len() as u64,
        });
    }
    if copied < WANTED.len() as u64 {
        return Err(err("The FFmpeg archive is missing expected files"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_is_https_lgpl_and_hash_shaped() {
        assert!(PIN.url.starts_with("https://github.com/"));
        assert!(PIN.url.contains("win64-lgpl"));
        assert!(!PIN.url.contains("-gpl-") && !PIN.url.contains("gpl.zip"));
        assert_eq!(PIN.sha256.len(), 64);
        assert!(PIN.sha256.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn extracts_only_wanted_files_by_basename() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("a.zip");
        {
            let mut w = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
            let opts = zip::write::SimpleFileOptions::default();
            for name in [
                "pkg/bin/ffmpeg.exe",
                "pkg/bin/ffprobe.exe",
                "pkg/bin/ffplay.exe",
                "pkg/LICENSE.txt",
                "../evil/ffmpeg.exe.dll",
            ] {
                w.start_file(name, opts).unwrap();
                w.write_all(b"x").unwrap();
            }
            w.finish().unwrap();
        }
        let out = dir.path().join("out");
        std::fs::create_dir(&out).unwrap();
        extract(&archive, &out, &|_| {}, &CancellationToken::new()).unwrap();
        let mut names: Vec<_> = std::fs::read_dir(&out)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["LICENSE.txt", "ffmpeg.exe", "ffprobe.exe"]);
    }
}
