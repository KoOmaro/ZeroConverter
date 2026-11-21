// ==============================================================================
// OWNERSHIP:
//   The "Convert with ZeroConverter" entry in Explorer's right-click menu.
//   Per user (HKCU), one cascade per file type that lists the formats it can
//   become. Each item runs `zero-converter.exe --convert <format> "<file>"`.
//   On Windows 11 the classic menu shows it under "Show more options".
//
// NOTES:
//   Written again at every start (the exe path changes with updates);
//   removed when the user turns it off and by the uninstaller
//   (`--unregister-menu`).
// ==============================================================================
use std::path::{Path, PathBuf};

/// Argument the menu items pass: `--convert webp "C:\photo.png"`.
pub const CONVERT_ARG: &str = "--convert";
/// Run by the uninstaller: remove the menu and exit.
pub const UNREGISTER_ARG: &str = "--unregister-menu";

const VERB: &str = "ZeroConverter";

/// One file type and the formats it can become, e.g. (`png`, [`jpg`, `webp`]).
pub type Entry = (String, Vec<String>);

/// `--convert webp C:\photo.png` → (`webp`, `C:\photo.png`).
pub fn convert_request(args: &[String]) -> Option<(String, PathBuf)> {
    let at = args.iter().position(|a| a == CONVERT_ARG)?;
    let target = args.get(at + 1)?.to_ascii_lowercase();
    let file = PathBuf::from(args.get(at + 2)?);
    file.is_absolute().then_some((target, file))
}

fn key_for(ext: &str) -> String {
    format!("Software\\Classes\\SystemFileAssociations\\.{ext}\\shell\\{VERB}")
}

/// Adds the menu for these file types, after removing any left over.
/// Writes every key it can; the first failure is returned.
pub fn register(entries: &[Entry], exe: &Path) -> Result<(), String> {
    let mut first = unregister(entries.iter().map(|(ext, _)| ext.as_str())).err();
    let mut set = |path: &str, name: &str, value: &str| {
        if let Err(e) = platform::set(path, name, value) {
            first.get_or_insert(e);
        }
    };
    let exe = exe.to_string_lossy();
    for (ext, targets) in entries {
        if targets.is_empty() {
            continue;
        }
        let base = key_for(ext);
        set(&base, "MUIVerb", &crate::i18n::tr("contextMenu.verb", &[]));
        set(&base, "Icon", &format!("\"{exe}\",0"));
        set(&base, "SubCommands", "");
        // Show it for any number of selected files, not only up to 15.
        set(&base, "MultiSelectModel", "Player");
        for target in targets {
            let item = format!("{base}\\shell\\{target}");
            set(
                &item,
                "MUIVerb",
                &crate::i18n::tr("contextMenu.to", &[("format", &target.to_uppercase())]),
            );
            set(
                &format!("{item}\\command"),
                "",
                &format!("\"{exe}\" {CONVERT_ARG} {target} \"%1\""),
            );
        }
    }
    platform::refresh_explorer();
    first.map_or(Ok(()), Err)
}

/// Removes the menu from these file types. A type that has no menu is fine.
pub fn unregister<'a>(exts: impl Iterator<Item = &'a str>) -> Result<(), String> {
    let mut first = None;
    for ext in exts {
        if let Err(e) = platform::delete_tree(&key_for(ext)) {
            first.get_or_insert(e);
        }
    }
    platform::refresh_explorer();
    first.map_or(Ok(()), Err)
}

/// Every extension the menu may have been added to: the uninstaller does
/// not know which formats this version reads, so it clears a generous list.
pub fn unregister_everywhere(known: &[String]) -> Result<(), String> {
    unregister(known.iter().map(String::as_str))
}

#[cfg(windows)]
mod platform {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
        KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
    };
    use windows_sys::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};

    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    /// Sets a string value (`name` empty: the key's default value).
    fn failed(what: &str, path: &[u16], code: u32) -> String {
        let path = String::from_utf16_lossy(&path[..path.len() - 1]);
        format!(
            "{what} HKCU\\{path}: {}",
            std::io::Error::from_raw_os_error(code as i32)
        )
    }

    pub fn set(path: &str, name: &str, value: &str) -> Result<(), String> {
        let mut key: HKEY = std::ptr::null_mut();
        let path = wide(path);
        // SAFETY: plain Win32 registry calls on HKCU with NUL-terminated
        // UTF-16 strings that outlive the calls; the key is closed below.
        unsafe {
            let code = RegCreateKeyExW(
                HKEY_CURRENT_USER,
                path.as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                std::ptr::null(),
                &mut key,
                std::ptr::null_mut(),
            );
            if code != 0 {
                return Err(failed("Could not create", &path, code));
            }
            let name = wide(name);
            let data = wide(value);
            let code = RegSetValueExW(
                key,
                if name.len() == 1 {
                    std::ptr::null()
                } else {
                    name.as_ptr()
                },
                0,
                REG_SZ,
                data.as_ptr().cast(),
                (data.len() * 2) as u32,
            );
            RegCloseKey(key);
            if code != 0 {
                return Err(failed("Could not write", &path, code));
            }
        }
        Ok(())
    }

    pub fn delete_tree(path: &str) -> Result<(), String> {
        let path = wide(path);
        // SAFETY: see `set`.
        let code = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, path.as_ptr()) };
        // A key that is not there is already removed.
        match code {
            0 | ERROR_FILE_NOT_FOUND => Ok(()),
            code => Err(failed("Could not remove", &path, code)),
        }
    }

    /// Tells Explorer the menus changed, so it does not show a stale one.
    pub fn refresh_explorer() {
        unsafe {
            SHChangeNotify(
                SHCNE_ASSOCCHANGED as i32,
                SHCNF_IDLIST,
                std::ptr::null(),
                std::ptr::null(),
            )
        };
    }
}

#[cfg(not(windows))]
mod platform {
    pub fn set(_path: &str, _name: &str, _value: &str) -> Result<(), String> {
        Ok(())
    }
    pub fn delete_tree(_path: &str) -> Result<(), String> {
        Ok(())
    }
    pub fn refresh_explorer() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convert_request_reads_format_and_file() {
        let args: Vec<String> = [
            "zero-converter.exe",
            "--convert",
            "WEBP",
            "C:\\a\\photo.png",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            convert_request(&args),
            Some(("webp".into(), PathBuf::from("C:\\a\\photo.png")))
        );
        assert_eq!(convert_request(&args[..3]), None);
        assert_eq!(
            convert_request(&[
                "x".into(),
                "--convert".into(),
                "webp".into(),
                "rel.png".into()
            ]),
            None
        );
    }
}
