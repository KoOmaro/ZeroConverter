// ==============================================================================
// OWNERSHIP:
//   Commands for desktop notification cards: preferences, preview, cards the
//   main window asks for, and the card page's handshake.
// ==============================================================================
use crate::receipt::{self, NotificationPrefs, Receipt, Tone};
use tauri::AppHandle;

#[tauri::command]
pub fn notification_prefs(app: AppHandle) -> NotificationPrefs {
    receipt::prefs(&app)
}

#[tauri::command]
pub fn set_notification_prefs(app: AppHandle, prefs: NotificationPrefs) -> Result<(), String> {
    receipt::set_prefs(&app, prefs)
}

#[tauri::command]
pub fn preview_notification(app: AppHandle) {
    receipt::show(
        &app,
        Receipt {
            tone: Tone::Success,
            title: "photo.png → photo.webp".into(),
            body: crate::i18n::tr("rename.converted", &[("time", "106 ms")]),
        },
    );
}

#[tauri::command]
pub fn show_receipt(app: AppHandle, receipt: Receipt) {
    receipt::show(&app, receipt);
}

#[tauri::command]
pub fn receipt_current() -> Option<Receipt> {
    receipt::current()
}

#[tauri::command]
pub fn receipt_painted() {
    receipt::painted();
}
