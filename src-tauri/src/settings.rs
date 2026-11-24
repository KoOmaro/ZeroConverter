// ==============================================================================
// OWNERSHIP:
//   The few settings the Rust side needs before any window exists
//   (the app can start hidden in the tray).
// ==============================================================================
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Settings {
    pub rename_convert: bool,
    /// Folders the user added for rename-to-convert, on top of their own
    /// Desktop, Downloads, Documents, Pictures, Videos and Music.
    pub rename_folders: Vec<String>,
    /// Keep the original file by default on rename-to-convert (`&` reverses it).
    #[serde(default)]
    pub keep_original: bool,
    /// "Convert with ZeroConverter" in Explorer's right-click menu.
    pub context_menu: bool,
    /// The "still running in the tray" hint was shown once.
    pub tray_hint_shown: bool,
    /// Desktop notification cards.
    pub notify: bool,
    pub notify_sound: bool,
    pub notify_sound_id: String,
    pub notify_volume: u8,
    /// Look for a new version on GitHub once a day. Nothing is downloaded
    /// until the user says yes.
    pub auto_update: bool,
    /// The language the window runs in ("en", "tr"…), as the window last said;
    /// empty until it has. The tray and the right-click menu use it at start.
    pub language: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            rename_convert: true,
            rename_folders: Vec::new(),
            keep_original: false,
            context_menu: true,
            tray_hint_shown: false,
            notify: true,
            notify_sound: true,
            notify_sound_id: "silver".into(),
            notify_volume: 80,
            auto_update: true,
            language: String::new(),
        }
    }
}

fn path(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("app-settings.json"))
}

pub fn load(app: &AppHandle) -> Settings {
    match path(app) {
        Some(p) => from_read(std::fs::read(p)),
        None => from_read(Err(std::io::Error::other("no settings folder"))),
    }
}

/// No file yet: defaults. A file that cannot be read, or not as settings,
/// keeps rename-to-convert OFF, so a damaged or locked file never turns back
/// on a feature that changes the user's files.
fn from_read(read: std::io::Result<Vec<u8>>) -> Settings {
    match read {
        Ok(bytes) => parse(Some(&bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => parse(None),
        Err(_) => safe_defaults(),
    }
}

fn parse(bytes: Option<&[u8]>) -> Settings {
    match bytes {
        None => Settings::default(),
        Some(b) => serde_json::from_slice(b).unwrap_or_else(|_| safe_defaults()),
    }
}

/// Everything that acts on its own starts off: rename-to-convert, the
/// right-click menu and the update check. The user turns back on what they
/// want; a choice they made is never undone by a damaged file.
fn safe_defaults() -> Settings {
    Settings {
        rename_convert: false,
        context_menu: false,
        auto_update: false,
        ..Settings::default()
    }
}

/// Changes and writes the settings. The error says why the change will not
/// survive a restart, for the caller to show.
pub fn save(app: &AppHandle, change: impl FnOnce(&mut Settings)) -> Result<(), String> {
    let mut s = load(app);
    change(&mut s);
    let p = path(app).ok_or("The settings folder could not be found.")?;
    let write = || -> std::io::Result<()> {
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir)?;
        }
        zero_fs::save_json_atomic(&p, &s, zero_fs::SyncPolicy::Authoritative)
    };
    write().map_err(|e| format!("The setting could not be saved: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_uses_defaults() {
        let s = parse(None);
        assert!(s.rename_convert);
        assert!(!s.keep_original);
    }

    #[test]
    fn damaged_file_keeps_rename_convert_off() {
        let damaged = parse(Some(b"{ not json"));
        assert!(
            !damaged.rename_convert
                && !damaged.context_menu
                && !damaged.auto_update
                && !damaged.keep_original
        );
        assert!(!parse(Some(br#"{"rename_convert": "yes"}"#)).rename_convert);
    }

    #[test]
    fn an_unreadable_file_keeps_rename_convert_off() {
        let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert!(!from_read(Err(denied)).rename_convert);
        let missing = std::io::Error::from(std::io::ErrorKind::NotFound);
        assert!(from_read(Err(missing)).rename_convert);
    }

    #[test]
    fn saved_choice_is_kept() {
        assert!(!parse(Some(br#"{"rename_convert": false}"#)).rename_convert);
        assert!(parse(Some(br#"{"notify": false}"#)).rename_convert);
    }

    #[test]
    fn old_settings_without_keep_original_default_to_false() {
        let s = parse(Some(br#"{"rename_convert": true, "notify": false}"#));
        assert!(!s.keep_original);
        assert!(s.rename_convert);
        assert!(!s.notify);
        assert!(parse(Some(br#"{"keep_original": true}"#)).keep_original);
    }
}
