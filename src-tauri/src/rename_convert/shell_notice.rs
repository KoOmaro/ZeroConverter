// ==============================================================================
// OWNERSHIP:
//   The shell's change notices around a folder that gets its name back.
//   Explorer announces its own rename to the shell a few hundred milliseconds
//   after the folder changed on disk. A folder renamed again before that makes
//   Explorer show "The file or folder does not exist"; one renamed again just
//   after it, without a notice of its own, keeps the old label in the view.
//   So: wait for Explorer's notice, rename, then send ours.
// ==============================================================================
use std::path::Path;
use std::time::Duration;

/// Waits until the shell has been told that a folder was renamed to `new`
/// (Explorer does that when its rename is done), at most `max`. False when no
/// notice came: the rename was not made in Explorer, or it was already over.
pub fn wait_for_folder_rename(new: &Path, max: Duration) -> bool {
    platform::wait(new, max)
}

/// Tells the shell that the folder `from` is now `to`, so open windows and
/// the Desktop show the new name.
pub fn folder_renamed(from: &Path, to: &Path) {
    platform::renamed(from, to)
}

#[cfg(windows)]
mod platform {
    use std::cell::{Cell, RefCell};
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::ptr::{null, null_mut};
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::UI::Shell::Common::ITEMIDLIST;
    use windows_sys::Win32::UI::Shell::{
        ILCreateFromPathW, ILFree, SHCNRF_NewDelivery, SHCNRF_ShellLevel,
        SHChangeNotification_Lock, SHChangeNotification_Unlock, SHChangeNotify,
        SHChangeNotifyDeregister, SHChangeNotifyEntry, SHChangeNotifyRegister,
        SHGetPathFromIDListW, SHCNE_RENAMEFOLDER, SHCNF_PATHW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
        MsgWaitForMultipleObjects, PeekMessageW, SetWindowLongPtrW, GWLP_WNDPROC, MSG, PM_REMOVE,
        QS_ALLINPUT, WM_APP,
    };

    const NOTICE: u32 = WM_APP + 1;

    thread_local! {
        /// The path (lower case) whose rename notice this thread waits for.
        static WANTED: RefCell<String> = const { RefCell::new(String::new()) };
        static HEARD: Cell<bool> = const { Cell::new(false) };
    }

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    pub fn wait(new: &Path, max: Duration) -> bool {
        let deadline = Instant::now() + max;
        let heard = new
            .parent()
            .is_some_and(|parent| listen(parent, new, deadline));
        if !heard {
            // No listener, or no notice: the full wait still passes.
            std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
        }
        heard
    }

    /// Listens for rename notices about the children of `parent` until one
    /// names `new` or the deadline passes.
    fn listen(parent: &Path, new: &Path, deadline: Instant) -> bool {
        WANTED.set(new.to_string_lossy().to_lowercase());
        HEARD.set(false);
        // SAFETY: every pointer passed below outlives its call; the window, the
        // registration and the id list are made and released on this thread,
        // and the window procedure only touches this thread's own cells.
        unsafe {
            let folder = ILCreateFromPathW(wide(parent).as_ptr());
            if folder.is_null() {
                return false;
            }
            // A hidden window of a system class: it only receives the notices.
            let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
            let window = CreateWindowExW(
                0,
                class.as_ptr(),
                null(),
                0,
                0,
                0,
                0,
                0,
                null_mut(),
                null_mut(),
                null_mut(),
                null(),
            );
            if window.is_null() {
                ILFree(folder);
                return false;
            }
            let procedure: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT =
                on_message;
            SetWindowLongPtrW(window, GWLP_WNDPROC, procedure as usize as _);
            let entry = SHChangeNotifyEntry {
                pidl: folder,
                fRecursive: 0,
            };
            // Shell level only: what a program announced, not what the disk did.
            let registration = SHChangeNotifyRegister(
                window,
                SHCNRF_ShellLevel | SHCNRF_NewDelivery,
                SHCNE_RENAMEFOLDER as i32,
                NOTICE,
                1,
                &entry,
            );
            while registration != 0 && !HEARD.get() {
                let mut message: MSG = std::mem::zeroed();
                while PeekMessageW(&mut message, window, 0, 0, PM_REMOVE) != 0 {
                    DispatchMessageW(&message);
                }
                let left = deadline.saturating_duration_since(Instant::now());
                if HEARD.get() || left.is_zero() {
                    break;
                }
                MsgWaitForMultipleObjects(0, null(), 0, left.as_millis() as u32, QS_ALLINPUT);
            }
            if registration != 0 {
                SHChangeNotifyDeregister(registration);
            }
            DestroyWindow(window);
            ILFree(folder);
        }
        HEARD.get()
    }

    unsafe extern "system" fn on_message(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if message != NOTICE {
            return DefWindowProcW(window, message, wparam, lparam);
        }
        let mut items: *mut *mut ITEMIDLIST = null_mut();
        let mut event = 0i32;
        let lock =
            SHChangeNotification_Lock(wparam as HANDLE, lparam as u32, &mut items, &mut event);
        if lock.is_null() {
            return 0;
        }
        if event as u32 & SHCNE_RENAMEFOLDER != 0 && !items.is_null() {
            // The second item of a rename notice is the new name.
            let renamed_to = *items.add(1);
            let mut path = [0u16; 260];
            if !renamed_to.is_null() && SHGetPathFromIDListW(renamed_to, path.as_mut_ptr()) != 0 {
                let len = path.iter().position(|&c| c == 0).unwrap_or(path.len());
                let path = String::from_utf16_lossy(&path[..len]).to_lowercase();
                if WANTED.with_borrow(|wanted| *wanted == path) {
                    HEARD.set(true);
                }
            }
        }
        SHChangeNotification_Unlock(lock);
        0
    }

    pub fn renamed(from: &Path, to: &Path) {
        let (from, to) = (wide(from), wide(to));
        // SAFETY: both are NUL-terminated paths; the shell copies them
        // before the call returns.
        unsafe {
            SHChangeNotify(
                SHCNE_RENAMEFOLDER as i32,
                SHCNF_PATHW,
                from.as_ptr().cast(),
                to.as_ptr().cast(),
            )
        };
    }
}

#[cfg(not(windows))]
mod platform {
    use std::path::Path;
    use std::time::Duration;

    pub fn wait(_new: &Path, _max: Duration) -> bool {
        false
    }

    pub fn renamed(_from: &Path, _to: &Path) {}
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::time::Instant;

    /// The wait ends when the shell is told about the rename, well before
    /// the limit (a fixed pause used to stand here, and Explorer was often
    /// not done by its end).
    #[test]
    fn the_wait_ends_when_the_rename_is_announced() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("Holiday");
        let new = dir.path().join("Holiday.webp");
        std::fs::create_dir(&new).unwrap();
        let (from, to) = (old.clone(), new.clone());
        let announcer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            folder_renamed(&from, &to);
        });
        let started = Instant::now();
        let heard = wait_for_folder_rename(&new, Duration::from_secs(20));
        announcer.join().unwrap();
        assert!(heard, "no notice arrived");
        assert!(started.elapsed() < Duration::from_secs(15));
    }

    #[test]
    fn without_a_notice_the_full_wait_passes() {
        let dir = tempfile::tempdir().unwrap();
        let new = dir.path().join("Holiday.webp");
        std::fs::create_dir(&new).unwrap();
        let started = Instant::now();
        assert!(!wait_for_folder_rename(&new, Duration::from_millis(400)));
        assert!(started.elapsed() >= Duration::from_millis(400));
    }
}
