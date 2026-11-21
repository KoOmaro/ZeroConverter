// OWNERSHIP:
//   Updates. A few seconds after start (release builds, when the setting is
//   on) the app asks GitHub Releases for `latest.json`. If a newer version is
//   there it only says so. Nothing is downloaded or installed until the user
//   chooses "Download and restart" in Settings > About.
//
// NOTES:
//   The installer is checked against the public key in tauri.conf.json before
//   it runs. The check request carries nothing but the app version and
//   platform.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

/// A newer version that was found, not yet downloaded.
#[derive(Default)]
pub struct Pending(Mutex<Option<Update>>);

impl Pending {
    pub fn version(&self) -> Option<String> {
        self.0.lock().ok()?.as_ref().map(|u| u.version.clone())
    }
}

/// Starts the background check when the setting is on. Debug builds never check.
pub fn start(app: &AppHandle) {
    static RUNNING: AtomicBool = AtomicBool::new(false);
    if cfg!(debug_assertions) || !crate::settings::load(app).auto_update {
        return;
    }
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(20)).await;
        loop {
            // Read again before every check: the user may have turned it off
            // since (also during the first 20 seconds).
            if !crate::settings::load(&app).auto_update {
                RUNNING.store(false, Ordering::SeqCst);
                return;
            }
            let _ = check(&app, true).await;
            // A tray app can run for weeks; look again once a day.
            tokio::time::sleep(Duration::from_secs(24 * 60 * 60)).await;
        }
    });
}

/// Asks GitHub whether a newer version exists. Returns that version, or
/// `None` when this one is the latest. Downloads nothing.
pub async fn check(app: &AppHandle, announce: bool) -> Result<Option<String>, String> {
    let update = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?;
    let Some(update) = update else {
        return Ok(None);
    };
    let version = update.version.clone();
    let already_known = app.state::<Pending>().version().as_deref() == Some(version.as_str());
    if let Ok(mut slot) = app.state::<Pending>().0.lock() {
        *slot = Some(update);
    }
    if announce && !already_known {
        crate::receipt::show(
            app,
            crate::receipt::Receipt {
                tone: crate::receipt::Tone::Success,
                title: crate::i18n::tr("update.title", &[("version", &version)]),
                body: crate::i18n::tr("update.body", &[]),
            },
        );
    }
    Ok(Some(version))
}

/// The user said yes: download the new installer, check its signature and
/// run it. The installer closes the app, updates it and starts it again.
/// A failed download keeps the update waiting, so "Try again" really does.
pub async fn download_and_install(app: &AppHandle) -> Result<(), String> {
    let update = app
        .state::<Pending>()
        .0
        .lock()
        .ok()
        .and_then(|s| s.as_ref().cloned());
    let Some(update) = update else {
        return Err("No update is waiting. Check for updates again.".into());
    };
    let bytes = update
        .download(|_, _| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    // The installer closes the app: let a file swap under way finish first.
    let rc = crate::rename_convert::state(app);
    let closing = rc.clone();
    let _ =
        tauri::async_runtime::spawn_blocking(move || closing.shut_down(crate::tray::SHUTDOWN_WAIT))
            .await;
    update.install(bytes).map_err(|e| {
        // Still running: go on converting.
        rc.resume();
        e.to_string()
    })
}
