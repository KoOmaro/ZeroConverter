use crate::updater::Pending;
use serde::Serialize;
use tauri::{AppHandle, Manager};

#[derive(Serialize)]
pub struct UpdateStatus {
    pub auto_update: bool,
    /// A newer version that was found; nothing is downloaded yet.
    pub available: Option<String>,
}

#[tauri::command]
pub fn update_status(app: AppHandle) -> UpdateStatus {
    UpdateStatus {
        auto_update: crate::settings::load(&app).auto_update,
        available: app.state::<Pending>().version(),
    }
}

#[tauri::command]
pub fn update_set_auto(app: AppHandle, enabled: bool) -> Result<(), String> {
    crate::settings::save(&app, |s| s.auto_update = enabled)?;
    if enabled {
        crate::updater::start(&app);
    }
    Ok(())
}

/// "Check now": the newer version, or `None` when up to date. Downloads nothing.
#[tauri::command]
pub async fn update_check(app: AppHandle) -> Result<Option<String>, String> {
    crate::updater::check(&app, false).await
}

/// "Download and restart": the user's yes.
#[tauri::command]
pub async fn update_install(app: AppHandle) -> Result<(), String> {
    crate::updater::download_and_install(&app).await
}
