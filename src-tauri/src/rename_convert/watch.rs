// ==============================================================================
// OWNERSHIP:
//   Listens for renames on every fixed drive, and decides which ones a person
//   made:
//   - In the user's own folders (Desktop, Downloads, Documents, Pictures,
//     Videos, Music) and any folder they add, every rename counts.
//   - Anywhere else, only renames made while File Explorer (or another file
//     manager) is the window in front. Tools and game engines rename files in
//     the background all the time; those are left alone.
//   Asks Windows for name changes only, so the cost is one blocked thread
//   per drive and a string compare per rename.
// ==============================================================================
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct Rename {
    pub old: PathBuf,
    pub new: PathBuf,
}

/// Folders where renames are machine noise, never a person converting a file:
/// the system, package and build output, and game engine caches.
const SKIP: &[&str] = &[
    "\\windows\\",
    "\\program files",
    "\\programdata\\",
    "\\$recycle.bin\\",
    "\\system volume information\\",
    "\\appdata\\",
    "\\node_modules\\",
    "\\.git\\",
    "\\target\\",
    "\\.zc-",
    // Unity
    "\\library\\",
    "\\temp\\",
    "\\obj\\",
    // Unreal
    "\\intermediate\\",
    "\\deriveddatacache\\",
    "\\saved\\",
    // Godot, editors
    "\\.godot\\",
    "\\.import\\",
    "\\.vs\\",
    "\\.cache\\",
];

pub fn skipped(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    SKIP.iter().any(|s| lower.contains(s))
}

/// The user's own folders, as Windows reports them (they may be moved, for
/// example into OneDrive).
pub fn user_folders() -> Vec<PathBuf> {
    platform::user_folders()
}

/// Should a rename at `full` reach the rule? `trusted` holds the keys of the
/// user's folders. Inside one, yes unless it is in an engine or build folder
/// below it (a folder the user picked is never skipped as a whole).
/// Elsewhere, only outside system and tool folders and only when a person
/// did it in a file manager (`by_person` is asked last, it costs a syscall).
pub fn admits(trusted: &[String], full: &str, by_person: impl FnOnce() -> bool) -> bool {
    let k = full.replace('/', "\\").to_lowercase();
    let inside = trusted
        .iter()
        .filter(|t| k.starts_with(t.as_str()))
        .max_by_key(|t| t.len());
    match inside {
        Some(t) => !skipped(&format!("\\{}", &k[t.len()..])),
        None => !skipped(&k) && by_person(),
    }
}

/// `C:\Users\me\Pictures` → `c:\users\me\pictures\`: one spelling for comparing.
fn key(path: &Path) -> String {
    let mut s = path.to_string_lossy().replace('/', "\\").to_lowercase();
    if !s.ends_with('\\') {
        s.push('\\');
    }
    s
}

/// Drops duplicates and folders inside another folder in the list (their
/// renames already arrive from the outer one).
pub fn outermost(folders: &[PathBuf]) -> Vec<PathBuf> {
    let mut sorted: Vec<(String, &PathBuf)> = folders.iter().map(|p| (key(p), p)).collect();
    sorted.sort_by(|a, b| a.0.len().cmp(&b.0.len()).then(a.0.cmp(&b.0)));
    let mut kept: Vec<(String, &PathBuf)> = Vec::new();
    for (k, p) in sorted {
        if !kept.iter().any(|(outer, _)| k.starts_with(outer.as_str())) {
            kept.push((k, p));
        }
    }
    kept.into_iter().map(|(_, p)| p.clone()).collect()
}

#[derive(Default)]
struct Roots {
    /// Folders that should be listened to now (keys).
    active: HashSet<String>,
    /// Folders that have a running thread (keys).
    running: HashSet<String>,
    /// The user's folders (keys): every rename in them counts.
    trusted: Vec<String>,
}

/// The set of watched folders; can change while the app runs.
pub struct Watcher {
    tx: Sender<Rename>,
    quick_filter: fn(&str) -> bool,
    roots: Arc<Mutex<Roots>>,
}

impl Watcher {
    pub fn new(tx: Sender<Rename>, quick_filter: fn(&str) -> bool) -> Self {
        Self {
            tx,
            quick_filter,
            roots: Arc::default(),
        }
    }

    /// `trusted` are the user's folders. Every fixed drive is listened to,
    /// plus any trusted folder on another drive (USB, network). A removed
    /// folder's thread stops at its next event and sends nothing more.
    pub fn set_folders(&self, trusted: &[PathBuf]) {
        let mut all: Vec<PathBuf> = platform::fixed_drives();
        all.extend(trusted.iter().cloned());
        let folders = outermost(&all);
        crate::rename_convert::debug(&format!("folders {folders:?}"));
        let Ok(mut roots) = self.roots.lock() else {
            return;
        };
        roots.trusted = trusted.iter().map(|p| key(p)).collect();
        roots.active = folders.iter().map(|p| key(p)).collect();
        for folder in folders {
            let k = key(&folder);
            if roots.running.contains(&k) || !folder.is_dir() {
                continue;
            }
            roots.running.insert(k.clone());
            let (tx, quick_filter, shared) =
                (self.tx.clone(), self.quick_filter, Arc::clone(&self.roots));
            let root = folder.to_string_lossy().trim_end_matches('\\').to_string() + "\\";
            let thread_key = k.clone();
            let spawned = std::thread::Builder::new()
                .name("zc-watch".into())
                .spawn(move || {
                    let k = thread_key;
                    // Checked and, if gone, deregistered under one lock, so a
                    // folder added back at the same moment is never left unwatched.
                    let still_wanted = || {
                        shared.lock().is_ok_and(|mut r| {
                            let wanted = r.active.contains(&k);
                            if !wanted {
                                r.running.remove(&k);
                            }
                            wanted
                        })
                    };
                    let admit = |full: &str| {
                        let trusted = shared.lock().map(|r| r.trusted.clone()).unwrap_or_default();
                        admits(&trusted, full, platform::file_manager_in_front)
                    };
                    let dropped =
                        platform::watch_root(&root, &tx, quick_filter, &admit, &still_wanted);
                    if !dropped {
                        // Stopped on an error (folder deleted, drive gone).
                        if let Ok(mut r) = shared.lock() {
                            r.running.remove(&k);
                        }
                    }
                });
            if spawned.is_err() {
                roots.running.remove(&k);
            }
        }
    }
}

#[cfg(windows)]
mod platform {
    use super::Rename;
    use std::os::windows::ffi::OsStrExt;
    use std::path::PathBuf;
    use std::sync::mpsc::Sender;
    use windows_sys::core::GUID;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, GetDriveTypeW, GetLogicalDrives, ReadDirectoryChangesW,
        FILE_ACTION_RENAMED_NEW_NAME, FILE_ACTION_RENAMED_OLD_NAME, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_LIST_DIRECTORY, FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME,
        FILE_NOTIFY_INFORMATION, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId,
    };

    /// File managers whose renames are a person's: Explorer (also the
    /// desktop) and the common alternatives.
    const FILE_MANAGERS: &[&str] = &[
        "explorer.exe",
        "dopus.exe",
        "totalcmd64.exe",
        "totalcmd.exe",
        "files.exe",
        "onecommander.exe",
        "xyplorer.exe",
        "freecommander.exe",
    ];

    /// Is the window in front right now a file manager?
    pub fn file_manager_in_front() -> bool {
        unsafe {
            let window = GetForegroundWindow();
            if window.is_null() {
                return false;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(window, &mut pid);
            if pid == 0 {
                return false;
            }
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process.is_null() {
                return false;
            }
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(process, 0, buf.as_mut_ptr(), &mut len);
            CloseHandle(process);
            if ok == 0 {
                return false;
            }
            let path = String::from_utf16_lossy(&buf[..len as usize]).to_lowercase();
            let exe = path.rsplit('\\').next().unwrap_or("");
            crate::rename_convert::debug(&format!("in front: {exe}"));
            FILE_MANAGERS.contains(&exe)
        }
    }

    /// `C:\`, `D:\`, ... for local hard drives. USB sticks are left out: an
    /// open watch would stop Windows from ejecting them safely.
    pub fn fixed_drives() -> Vec<PathBuf> {
        const DRIVE_FIXED: u32 = 3;
        let mask = unsafe { GetLogicalDrives() };
        (0..26u8)
            .filter(|i| mask & (1 << i) != 0)
            .map(|i| format!("{}:\\", (b'A' + i) as char))
            .filter(|root| unsafe { GetDriveTypeW(wide(root).as_ptr()) } == DRIVE_FIXED)
            .map(PathBuf::from)
            .collect()
    }
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{
        FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Music,
        FOLDERID_Pictures, FOLDERID_Videos, SHGetKnownFolderPath,
    };

    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    fn known_folder(id: &GUID) -> Option<PathBuf> {
        let mut raw: *mut u16 = std::ptr::null_mut();
        // SAFETY: Windows writes a CoTaskMem-allocated, NUL-terminated string
        // to `raw` (even on failure, when it may be null); it is freed below.
        let hr = unsafe { SHGetKnownFolderPath(id, 0, std::ptr::null_mut(), &mut raw) };
        if raw.is_null() {
            return None;
        }
        let path = if hr >= 0 {
            let len = (0..).take_while(|&i| unsafe { *raw.add(i) } != 0).count();
            Some(PathBuf::from(String::from_utf16_lossy(unsafe {
                std::slice::from_raw_parts(raw, len)
            })))
        } else {
            None
        };
        unsafe { CoTaskMemFree(raw.cast()) };
        path
    }

    pub fn user_folders() -> Vec<PathBuf> {
        [
            &FOLDERID_Desktop,
            &FOLDERID_Downloads,
            &FOLDERID_Documents,
            &FOLDERID_Pictures,
            &FOLDERID_Videos,
            &FOLDERID_Music,
        ]
        .into_iter()
        .filter_map(known_folder)
        .collect()
    }

    /// Blocks until the folder is gone or `still_wanted` says stop. Returns
    /// true when it stopped because the folder was taken off the list.
    pub fn watch_root(
        root: &str,
        tx: &Sender<Rename>,
        quick_filter: fn(&str) -> bool,
        admit: &dyn Fn(&str) -> bool,
        still_wanted: &dyn Fn() -> bool,
    ) -> bool {
        let handle = unsafe {
            CreateFileW(
                wide(root).as_ptr(),
                FILE_LIST_DIRECTORY,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            crate::rename_convert::debug(&format!(
                "watch {root} could not open: {}",
                std::io::Error::last_os_error()
            ));
            return false;
        }
        crate::rename_convert::debug(&format!("watching {root}"));
        let mut dropped = false;
        // DWORD-aligned buffer, as ReadDirectoryChangesW requires. A whole
        // drive is busy; 256 KB rides out bursts (a build, an update).
        let mut buf = vec![0u32; 64 * 1024];
        let mut pending_old: Option<PathBuf> = None;
        loop {
            let mut bytes = 0u32;
            let ok = unsafe {
                ReadDirectoryChangesW(
                    handle,
                    buf.as_mut_ptr().cast(),
                    (buf.len() * 4) as u32,
                    1,
                    // Folder renames too: `Holiday` → `Holiday.webp`.
                    FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_DIR_NAME,
                    &mut bytes,
                    std::ptr::null_mut(),
                    None,
                )
            };
            if ok == 0 {
                crate::rename_convert::debug(&format!(
                    "watch {root} stopped: {}",
                    std::io::Error::last_os_error()
                ));
                break;
            }
            if !still_wanted() {
                dropped = true;
                break;
            }
            if bytes == 0 {
                // Buffer overflowed during a burst; renames in it are lost.
                pending_old = None;
                continue;
            }
            let base = buf.as_ptr() as *const u8;
            let mut offset = 0usize;
            let filled = bytes as usize;
            loop {
                // A record must lie inside what the call wrote.
                if offset + std::mem::offset_of!(FILE_NOTIFY_INFORMATION, FileName) > filled {
                    break;
                }
                // SAFETY: `offset` is 0 or the sum of the kernel's
                // `NextEntryOffset`s, so it starts a record inside the first
                // `filled` bytes of `buf`; records are DWORD-aligned and `buf` is
                // a `u32` buffer. `buf` is not written again until this loop ends.
                let info = unsafe { &*(base.add(offset) as *const FILE_NOTIFY_INFORMATION) };
                let len = info.FileNameLength as usize / 2;
                if offset + std::mem::offset_of!(FILE_NOTIFY_INFORMATION, FileName) + len * 2
                    > filled
                {
                    break;
                }
                // SAFETY: the record's name is `FileNameLength` bytes of UTF-16
                // right after the header, inside the written part (checked above).
                let name = unsafe { std::slice::from_raw_parts(info.FileName.as_ptr(), len) };
                let rel = String::from_utf16_lossy(name);
                match info.Action {
                    FILE_ACTION_RENAMED_OLD_NAME => {
                        pending_old = Some(PathBuf::from(root).join(&rel))
                    }
                    FILE_ACTION_RENAMED_NEW_NAME => {
                        if let Some(old) = pending_old.take() {
                            let full = format!("{root}{rel}");
                            crate::rename_convert::debug(&format!(
                                "rename {} -> {full}",
                                old.display()
                            ));
                            if quick_filter(&rel) && admit(&full) {
                                let _ = tx.send(Rename {
                                    old,
                                    new: PathBuf::from(full),
                                });
                            }
                        }
                    }
                    _ => {}
                }
                if info.NextEntryOffset == 0 {
                    break;
                }
                offset += info.NextEntryOffset as usize;
            }
        }
        unsafe { CloseHandle(handle) };
        dropped
    }
}

#[cfg(not(windows))]
mod platform {
    use super::Rename;
    use std::path::PathBuf;
    use std::sync::mpsc::Sender;

    pub fn user_folders() -> Vec<PathBuf> {
        Vec::new()
    }

    pub fn fixed_drives() -> Vec<PathBuf> {
        Vec::new()
    }

    pub fn file_manager_in_front() -> bool {
        false
    }

    pub fn watch_root(
        _root: &str,
        _tx: &Sender<Rename>,
        _quick_filter: fn(&str) -> bool,
        _admit: &dyn Fn(&str) -> bool,
        _still_wanted: &dyn Fn() -> bool,
    ) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_and_tool_folders_are_skipped() {
        assert!(skipped("C:\\Windows\\Temp\\a.png"));
        assert!(skipped("C:\\Users\\me\\AppData\\Local\\x.webp"));
        assert!(skipped("D:\\proj\\node_modules\\x.png"));
        assert!(!skipped("C:\\Users\\me\\Desktop\\photo.webp"));
        assert!(skipped("\\MyGame\\Library\\Artifacts\\a.png"));
        assert!(skipped("\\Shooter\\Intermediate\\x.png"));
        assert!(!skipped("\\Art\\Characters\\hero.png"));
    }

    #[test]
    fn user_folders_count_every_rename_elsewhere_only_a_persons() {
        let trusted = vec![
            key(Path::new("C:\\Users\\me\\Desktop")),
            key(Path::new("D:\\Unity\\Game\\Library")),
        ];
        let never = || -> bool { panic!("must not ask who renamed inside a user folder") };
        // Inside a user folder: no need to ask.
        assert!(admits(&trusted, "C:\\Users\\me\\Desktop\\a.webp", never));
        // A folder the user picked counts even if its name looks like a tool folder.
        assert!(admits(&trusted, "D:\\Unity\\Game\\Library\\a.webp", never));
        // ...but engine folders below a user folder are still skipped.
        assert!(!admits(
            &trusted,
            "C:\\Users\\me\\Desktop\\Game\\Library\\a.webp",
            never
        ));
        // Elsewhere: only when a person did it in a file manager.
        assert!(admits(&trusted, "Q:\\Random\\a.jpg", || true));
        assert!(!admits(&trusted, "Q:\\Random\\a.jpg", || false));
        // System and tool folders never, whoever is in front.
        assert!(!admits(&trusted, "C:\\Windows\\Temp\\a.png", || true));
        assert!(!admits(&trusted, "E:\\proj\\node_modules\\a.png", || true));
    }

    #[cfg(windows)]
    #[test]
    fn fixed_drives_include_the_system_drive() {
        let drives = platform::fixed_drives();
        let system = std::env::var("SystemDrive")
            .unwrap_or("C:".into())
            .to_lowercase();
        assert!(
            drives.iter().any(|d| key(d) == format!("{system}\\")),
            "{drives:?}"
        );
    }

    #[test]
    fn nested_and_repeated_folders_are_watched_once() {
        let folders = [
            PathBuf::from("C:\\Users\\me\\Documents\\Art"),
            PathBuf::from("C:\\Users\\me\\Documents"),
            PathBuf::from("c:/users/me/documents/"),
            PathBuf::from("C:\\Users\\me\\Documents2"),
            PathBuf::from("D:\\Assets"),
        ];
        let kept = outermost(&folders);
        let keys: Vec<String> = kept.iter().map(|p| key(p)).collect();
        assert_eq!(
            keys,
            [
                "d:\\assets\\",
                "c:\\users\\me\\documents\\",
                "c:\\users\\me\\documents2\\"
            ]
        );
    }

    #[cfg(windows)]
    #[test]
    fn user_folders_come_from_windows() {
        let folders = user_folders();
        assert!(!folders.is_empty());
        assert!(folders.iter().all(|p| p.is_absolute()));
    }
}
