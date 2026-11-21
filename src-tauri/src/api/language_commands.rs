// ==============================================================================
// OWNERSHIP:
//   The window tells Rust which language it runs in, so the tray menu,
//   Explorer's right-click entry and the notices Rust makes follow it.
// ==============================================================================
use tauri::AppHandle;

/// Called by every window start and whenever the language changes. `language`
/// is what the window resolved ("tr", "pt-BR"…, never "system").
#[tauri::command]
pub fn set_ui_language(app: AppHandle, language: String) -> Result<(), String> {
    let Some(code) = crate::i18n::match_language(&language) else {
        return Err(format!("unknown language: {language}"));
    };
    let changed = crate::i18n::current() != code;
    crate::i18n::set(code);
    crate::settings::save(&app, |s| s.language = code.to_string())?;
    if changed {
        crate::tray::refresh(&app);
        // Re-writes the menu entries in the new language (or removes them when off).
        crate::api::rename_commands::sync_context_menu(&app);
    }
    Ok(())
}
