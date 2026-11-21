// ==============================================================================
// OWNERSHIP:
//   System tray: ZeroConverter keeps listening for renames after the window
//   closes. Menu: open, rename-to-convert on/off, start with Windows, quit.
// ==============================================================================
use std::sync::atomic::Ordering;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_autostart::ManagerExt;

const TRAY_ID: &str = "zc-tray";

/// How long Quit and an update wait for a file swap under way.
pub const SHUTDOWN_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let enabled = crate::rename_convert::state(app)
        .enabled
        .load(Ordering::Relaxed);
    let autostart = app.autolaunch().is_enabled().unwrap_or(false);
    Menu::with_items(
        app,
        &[
            &MenuItem::with_id(
                app,
                "open",
                crate::i18n::tr("tray.open", &[]),
                true,
                None::<&str>,
            )?,
            &PredefinedMenuItem::separator(app)?,
            &CheckMenuItem::with_id(
                app,
                "rename",
                crate::i18n::tr("tray.rename", &[]),
                true,
                enabled,
                None::<&str>,
            )?,
            &CheckMenuItem::with_id(
                app,
                "autostart",
                crate::i18n::tr("tray.autostart", &[]),
                true,
                autostart,
                None::<&str>,
            )?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(
                app,
                "quit",
                crate::i18n::tr("tray.quit", &[]),
                true,
                None::<&str>,
            )?,
        ],
    )
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_menu(app)?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("ZeroConverter · rename a file to convert it")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main(app),
            "rename" => {
                let rc = crate::rename_convert::state(app);
                let next = !rc.enabled.load(Ordering::Relaxed);
                rc.enabled.store(next, Ordering::Relaxed);
                if let Err(e) = crate::settings::save(app, |s| s.rename_convert = next) {
                    eprintln!("{e}");
                }
                refresh(app);
            }
            "autostart" => {
                let launcher = app.autolaunch();
                let _ = if launcher.is_enabled().unwrap_or(false) {
                    launcher.disable()
                } else {
                    launcher.enable()
                };
                refresh(app);
            }
            "quit" => {
                // Let a file swap under way finish first (a few seconds at most).
                let app = app.clone();
                std::thread::spawn(move || {
                    crate::rename_convert::state(&app).shut_down(SHUTDOWN_WAIT);
                    app.exit(0);
                });
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

/// Rebuilds the menu so check marks follow the real state.
pub fn refresh(app: &AppHandle) {
    if let (Some(tray), Ok(menu)) = (app.tray_by_id(TRAY_ID), build_menu(app)) {
        let _ = tray.set_menu(Some(menu));
    }
}

/// The settings page the window should open next, kept until the window asks
/// (`take_settings_request`): an event alone is lost if the page is still
/// loading.
static SETTINGS_REQUEST: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Brings the window forward with Settings open at `section`.
pub fn open_settings(app: &AppHandle, section: &str) {
    use tauri::Emitter;
    *SETTINGS_REQUEST.lock().unwrap() = Some(section.to_string());
    show_main(app);
    let _ = app.emit_to("main", "settings://open", section);
}

pub fn take_settings_request() -> Option<String> {
    SETTINGS_REQUEST.lock().unwrap().take()
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}
