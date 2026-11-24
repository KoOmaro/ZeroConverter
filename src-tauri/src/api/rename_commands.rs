// ==============================================================================
// OWNERSHIP:
//   IPC for rename-to-convert: the on/off switch, start-with-Windows, and the
//   `+` options window.
// ==============================================================================
use crate::rename_convert::{self, apply::QuickOptions, QuickJobInfo, WatchedFolder};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::ManagerExt;

#[derive(serde::Serialize)]
pub struct RenameConvertStatus {
    pub enabled: bool,
    pub autostart: bool,
    pub folders: Vec<WatchedFolder>,
    pub context_menu: bool,
    pub keep_original: bool,
}

#[tauri::command]
pub fn rename_convert_status(app: AppHandle) -> RenameConvertStatus {
    let state = rename_convert::state(&app);
    RenameConvertStatus {
        enabled: state.enabled.load(Ordering::Relaxed),
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        folders: state.folders(&app),
        context_menu: crate::settings::load(&app).context_menu,
        keep_original: state.keep_original.load(Ordering::Relaxed),
    }
}

/// Adds or removes "Convert with ZeroConverter" in Explorer's menu.
#[tauri::command]
pub fn context_menu_set(app: AppHandle, enabled: bool) -> Result<(), String> {
    let saved = crate::settings::save(&app, |s| s.context_menu = enabled);
    sync_context_menu(&app);
    saved
}

/// Makes Explorer's menu match the saved setting (adds it for this exe, or
/// removes it), off the calling thread. One sync at a time, and each reads
/// the setting when it runs, so quick on/off clicks end in the last choice.
pub fn sync_context_menu(app: &AppHandle) {
    static SYNC: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let app = app.clone();
    std::thread::spawn(move || {
        let _one_at_a_time = SYNC.lock().unwrap_or_else(|e| e.into_inner());
        let state = rename_convert::state(&app);
        let wanted = crate::settings::load(&app).context_menu;
        let result = if wanted {
            std::env::current_exe()
                .map_err(|e| e.to_string())
                .and_then(|exe| crate::context_menu::register(&state.menu_entries(), &exe))
        } else {
            crate::context_menu::unregister_everywhere(&state.known_extensions())
        };
        // The switch shows the saved choice; say so when Explorer's menu
        // could not be made to match it.
        if let Err(e) = result {
            crate::receipt::show(
                &app,
                crate::receipt::Receipt {
                    tone: crate::receipt::Tone::Failure,
                    title: crate::i18n::tr(
                        if wanted {
                            "menuError.addTitle"
                        } else {
                            "menuError.removeTitle"
                        },
                        &[],
                    ),
                    body: e,
                },
            );
        }
    });
}

/// Adds a folder to rename-to-convert (from the folder picker).
#[tauri::command]
pub fn rename_convert_add_folder(app: AppHandle, path: String) -> Result<(), String> {
    let dir = std::path::Path::new(&path);
    if !dir.is_absolute() || !dir.is_dir() {
        return Err("Choose an existing folder.".into());
    }
    let mut extra = crate::settings::load(&app).rename_folders;
    if !extra.iter().any(|p| p.eq_ignore_ascii_case(&path)) {
        extra.push(path);
    }
    rename_convert::state(&app).set_extra_folders(&app, extra)
}

#[tauri::command]
pub fn rename_convert_remove_folder(app: AppHandle, path: String) -> Result<(), String> {
    let mut extra = crate::settings::load(&app).rename_folders;
    extra.retain(|p| !p.eq_ignore_ascii_case(&path));
    rename_convert::state(&app).set_extra_folders(&app, extra)
}

#[tauri::command]
pub fn rename_convert_set_enabled(app: AppHandle, enabled: bool) -> Result<(), String> {
    // Applies now either way; the error says it will not survive a restart.
    rename_convert::state(&app)
        .enabled
        .store(enabled, Ordering::Relaxed);
    let saved = crate::settings::save(&app, |s| s.rename_convert = enabled);
    crate::tray::refresh(&app);
    saved
}

#[tauri::command]
pub fn rename_convert_set_keep_original(app: AppHandle, enabled: bool) -> Result<(), String> {
    // Applies now either way; the error says it will not survive a restart.
    rename_convert::state(&app)
        .keep_original
        .store(enabled, Ordering::Relaxed);
    crate::settings::save(&app, |s| s.keep_original = enabled)
}

#[tauri::command]
pub fn rename_convert_set_autostart(app: AppHandle, enabled: bool) -> Result<(), String> {
    let launcher = app.autolaunch();
    let result = if enabled {
        launcher.enable()
    } else {
        launcher.disable()
    };
    crate::tray::refresh(&app);
    result.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn quick_job_info(app: AppHandle, id: String) -> Option<QuickJobInfo> {
    rename_convert::state(&app).quick_info(&id)
}

#[tauri::command]
pub fn quick_job_confirm(app: AppHandle, id: String, options: QuickOptions) {
    rename_convert::state(&app).quick_confirm(&app, &id, options);
    close_quick(&app, &id);
}

#[tauri::command]
pub fn quick_job_cancel(app: AppHandle, id: String) {
    rename_convert::state(&app).quick_cancel(&id);
    close_quick(&app, &id);
}

fn close_quick(app: &AppHandle, id: &str) {
    if let Some(w) = app.get_webview_window(&format!("quick-{id}")) {
        let _ = w.close();
    }
}
