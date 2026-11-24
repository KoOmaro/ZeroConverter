// ==============================================================================
// OWNERSHIP:
//   The shell's change notices around rename-to-convert:
//   - Folders getting their name back: Explorer announces its own rename to
//     the shell a few hundred milliseconds after the folder changed on disk.
//     Wait for Explorer's notice, rename, then send ours.
//   - Files landing under a different name than typed (`@N`, `&`, `+`),
//     restoring a kept original alongside a plain rename, or reverting to
//     their old name: Explorer overlooks plain disk moves right after its own
//     rename or may overwrite the view with its own late rename notice, so
//     tell the shell now and once more after a short delay, off the rename
//     thread.
// ==============================================================================
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long a single shell notification call may take before moving on.
#[cfg(any(windows, test))]
const NOTIFY_TIMEOUT: Duration = Duration::from_millis(1000);

/// Delay before resending file conversion notices so Explorer does not restore
/// the typed stale name with a late rename notice of its own.
#[cfg(windows)]
const RESEND_DELAY: Duration = Duration::from_millis(1500);

/// One shell change notice to emit after a rename conversion finishes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellEvent {
    RenameItem { from: PathBuf, to: PathBuf },
    Create(PathBuf),
    UpdateDir(PathBuf),
}

fn same_path(a: &Path, b: &Path) -> bool {
    #[cfg(windows)]
    {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    }
    #[cfg(not(windows))]
    {
        a == b
    }
}

/// Decides whether a finished rename conversion needs shell notices, and which
/// events to emit.
///
/// When the output ended at the exact path Explorer already renamed to
/// (`current == final_path`):
/// - Without keeping the original: empty (Explorer already shows `final_path`).
/// - When keeping the original: `original` reappeared next to `final_path`
///   (`SHCNE_CREATE`, if different from `current`) and the parent folder
///   changed (`SHCNE_UPDATEDIR`).
///
/// When `current != final_path`:
/// - When the original was not kept: `current` became `final_path`
///   (`SHCNE_RENAMEITEM`) and its parent folder changed (`SHCNE_UPDATEDIR`).
/// - When the original was kept (`&` or setting): `current` went back to
///   `original` (`SHCNE_RENAMEITEM`, if different), a new file appeared at
///   `final_path` (`SHCNE_CREATE`, if different from `original`), and the
///   parent folder changed (`SHCNE_UPDATEDIR`).
pub fn plan_file_converted(
    current: &Path,
    final_path: &Path,
    original: &Path,
    keep_original: bool,
) -> Vec<ShellEvent> {
    let mut events = Vec::new();
    if same_path(current, final_path) {
        if !keep_original || same_path(current, original) {
            return events;
        }
        events.push(ShellEvent::Create(original.to_path_buf()));
    } else if keep_original {
        if !same_path(current, original) {
            events.push(ShellEvent::RenameItem {
                from: current.to_path_buf(),
                to: original.to_path_buf(),
            });
        }
        if !same_path(final_path, original) {
            events.push(ShellEvent::Create(final_path.to_path_buf()));
        }
    } else {
        events.push(ShellEvent::RenameItem {
            from: current.to_path_buf(),
            to: final_path.to_path_buf(),
        });
    }
    if let Some(parent) = final_path.parent().filter(|p| !p.as_os_str().is_empty()) {
        events.push(ShellEvent::UpdateDir(parent.to_path_buf()));
    }
    events
}

/// Maximum concurrent shell notification threads so a burst of scripted renames
/// or a hung shell call cannot spawn unbounded background workers.
const MAX_NOTICE_WORKERS: usize = 32;

static ACTIVE_NOTICES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

struct NoticeGuard<'a>(&'a std::sync::atomic::AtomicUsize);

impl<'a> NoticeGuard<'a> {
    fn try_acquire(counter: &'a std::sync::atomic::AtomicUsize, limit: usize) -> Option<Self> {
        use std::sync::atomic::Ordering;
        counter
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < limit).then_some(n + 1)
            })
            .ok()
            .map(|_| Self(counter))
    }
}

impl Drop for NoticeGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
    }
}

#[cfg(any(windows, test))]
fn run_bounded(timeout: Duration, work: impl FnOnce() + Send + 'static) -> bool {
    let Some(guard) = NoticeGuard::try_acquire(&ACTIVE_NOTICES, MAX_NOTICE_WORKERS) else {
        super::debug("shell notify skipped: worker limit reached");
        return false;
    };
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("zc-shell-notice".into())
        .spawn(move || {
            let _guard = guard;
            work();
            let _ = done_tx.send(());
        });
    if let Err(e) = spawned {
        super::debug(&format!("shell notify spawn failed: {e}"));
        return false;
    }
    match done_rx.recv_timeout(timeout) {
        Ok(()) => true,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            super::debug(&format!("shell notify timed out after {timeout:?}"));
            false
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            super::debug("shell notify thread stopped unexpectedly");
            false
        }
    }
}

/// Tells the shell about a finished rename conversion when Explorer needs an
/// update (`current != final_path`, or a kept original reappeared next to a
/// plain rename), then sends the same events once more after [`RESEND_DELAY`]
/// off the rename thread.
pub fn file_converted(current: &Path, final_path: &Path, original: &Path, keep_original: bool) {
    let events = plan_file_converted(current, final_path, original, keep_original);
    if events.is_empty() {
        return;
    }
    #[cfg(windows)]
    {
        let Some(guard) = NoticeGuard::try_acquire(&ACTIVE_NOTICES, MAX_NOTICE_WORKERS) else {
            super::debug("shell notify schedule skipped: worker limit reached");
            return;
        };
        let spawned = std::thread::Builder::new()
            .name("zc-shell-notice".into())
            .spawn(move || {
                let _guard = guard;
                let send = |phase: &str, batch: Vec<ShellEvent>| {
                    super::debug(&format!("shell notify ({phase}): {batch:?}"));
                    run_bounded(NOTIFY_TIMEOUT, move || platform::send_events(&batch));
                };
                send("now", events.clone());
                std::thread::sleep(RESEND_DELAY);
                send("resend", events);
            });
        if let Err(e) = spawned {
            super::debug(&format!("shell notify spawn failed: {e}"));
        }
    }
    #[cfg(not(windows))]
    {
        let _ = events;
    }
}

/// Tells the shell that a renamed file was put back under `to` (`from` → `to`).
pub fn file_restored(from: &Path, to: &Path) {
    file_converted(from, to, to, false);
}

/// Whether a rename wait listens for a file (`SHCNE_RENAMEITEM`) or a folder
/// (`SHCNE_RENAMEFOLDER`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenameKind {
    File,
    Folder,
}

/// An active listener waiting on its own thread for Explorer's shell rename
/// notice, bounded by `max`.
pub struct PendingWait {
    rx: Option<std::sync::mpsc::Receiver<(bool, std::time::Instant)>>,
    deadline: std::time::Instant,
}

impl PendingWait {
    /// Blocks until Explorer's notice arrives or the bounded deadline expires.
    /// Returns `(heard, finished_at)`. Never fails and never blocks past the
    /// deadline plus a small join margin.
    pub fn wait(self) -> (bool, std::time::Instant) {
        match self.rx {
            Some(rx) => {
                let timeout = self
                    .deadline
                    .saturating_duration_since(std::time::Instant::now())
                    + Duration::from_millis(100);
                rx.recv_timeout(timeout)
                    .unwrap_or_else(|_| (false, std::time::Instant::now()))
            }
            None => {
                std::thread::sleep(
                    self.deadline
                        .saturating_duration_since(std::time::Instant::now()),
                );
                (false, std::time::Instant::now())
            }
        }
    }
}

/// Starts listening in the background for the shell notice that `kind` was
/// renamed to `new`, bounded by `max`.
///
/// The shell calls run on a thread of their own: they talk to Explorer and
/// can hang while it is busy, and [`PendingWait::wait`] still ends at `max`.
pub fn start_rename_wait(new: &Path, max: Duration, kind: RenameKind) -> PendingWait {
    let deadline = std::time::Instant::now() + max;
    let (heard, listener) = std::sync::mpsc::channel();
    let path = new.to_path_buf();
    let spawned = std::thread::Builder::new()
        .name("zc-shell-notice".into())
        .spawn(move || {
            let ok = platform::wait(&path, max, kind);
            let _ = heard.send((ok, std::time::Instant::now()));
        });
    PendingWait {
        rx: spawned.ok().map(|_| listener),
        deadline,
    }
}

/// Waits until the shell has been told that `kind` was renamed to `new`
/// (Explorer does that when its rename is done), at most `max`. False when no
/// notice came: the rename was not made in Explorer, or it was already over.
pub fn wait_for_rename(new: &Path, max: Duration, kind: RenameKind) -> bool {
    start_rename_wait(new, max, kind).wait().0
}

/// Waits until the shell has been told that a folder was renamed to `new`, at
/// most `max`.
pub fn wait_for_folder_rename(new: &Path, max: Duration) -> bool {
    wait_for_rename(new, max, RenameKind::Folder)
}

/// Tells the shell that the folder `from` is now `to`, so open windows and
/// the Desktop show the new name. Does not wait for the shell.
pub fn folder_renamed(from: &Path, to: &Path) {
    let Some(guard) = NoticeGuard::try_acquire(&ACTIVE_NOTICES, MAX_NOTICE_WORKERS) else {
        super::debug("folder shell notify skipped: worker limit reached");
        return;
    };
    let (from, to) = (from.to_path_buf(), to.to_path_buf());
    let _ = std::thread::Builder::new()
        .name("zc-shell-notice".into())
        .spawn(move || {
            let _guard = guard;
            platform::renamed(&from, &to);
        });
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
        SHGetPathFromIDListW, SHCNE_CREATE, SHCNE_RENAMEFOLDER, SHCNE_RENAMEITEM, SHCNE_UPDATEDIR,
        SHCNF_FLUSHNOWAIT, SHCNF_PATHW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
        MsgWaitForMultipleObjects, PeekMessageW, SetWindowLongPtrW, GWLP_WNDPROC, MSG, PM_REMOVE,
        QS_ALLINPUT, WM_APP,
    };

    const NOTICE: u32 = WM_APP + 1;

    thread_local! {
        /// The path (lower case) and shell event mask whose rename notice this
        /// thread waits for.
        static WANTED: RefCell<String> = const { RefCell::new(String::new()) };
        static WANTED_EVENT: Cell<u32> = const { Cell::new(0) };
        static HEARD: Cell<bool> = const { Cell::new(false) };
    }

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    pub fn wait(new: &Path, max: Duration, kind: super::RenameKind) -> bool {
        let event_mask = match kind {
            super::RenameKind::File => SHCNE_RENAMEITEM,
            super::RenameKind::Folder => SHCNE_RENAMEFOLDER,
        };
        let deadline = Instant::now() + max;
        let heard = new
            .parent()
            .is_some_and(|parent| listen(parent, new, deadline, event_mask));
        if !heard {
            // No listener, or no notice: the full wait still passes.
            std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
        }
        heard
    }

    /// Listens for rename notices about the children of `parent` until one
    /// matching `event_mask` names `new` or the deadline passes.
    fn listen(parent: &Path, new: &Path, deadline: Instant, event_mask: u32) -> bool {
        WANTED.set(new.to_string_lossy().to_lowercase());
        WANTED_EVENT.set(event_mask);
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
                event_mask as i32,
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
        if event as u32 & WANTED_EVENT.get() != 0 && !items.is_null() {
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

    pub fn send_events(events: &[super::ShellEvent]) {
        let flags = SHCNF_PATHW | SHCNF_FLUSHNOWAIT;
        for event in events {
            match event {
                super::ShellEvent::RenameItem { from, to } => {
                    let (from, to) = (wide(from), wide(to));
                    // SAFETY: both are NUL-terminated UTF-16 paths kept alive for the call.
                    unsafe {
                        SHChangeNotify(
                            SHCNE_RENAMEITEM as i32,
                            flags,
                            from.as_ptr().cast(),
                            to.as_ptr().cast(),
                        );
                    }
                }
                super::ShellEvent::Create(path) => {
                    let path = wide(path);
                    // SAFETY: NUL-terminated UTF-16 path kept alive for the call.
                    unsafe {
                        SHChangeNotify(SHCNE_CREATE as i32, flags, path.as_ptr().cast(), null());
                    }
                }
                super::ShellEvent::UpdateDir(dir) => {
                    let dir = wide(dir);
                    // SAFETY: NUL-terminated UTF-16 path kept alive for the call.
                    unsafe {
                        SHChangeNotify(SHCNE_UPDATEDIR as i32, flags, dir.as_ptr().cast(), null());
                    }
                }
            }
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use std::path::Path;
    use std::time::Duration;

    pub fn wait(_new: &Path, _max: Duration, _kind: super::RenameKind) -> bool {
        false
    }

    pub fn renamed(_from: &Path, _to: &Path) {}
}

#[cfg(test)]
mod pure_tests {
    use super::*;

    #[test]
    fn plan_skips_when_final_matches_current_without_keep() {
        let orig = Path::new("C:/dir/photo.png");
        let cur = Path::new("C:/dir/photo.webp");
        assert!(plan_file_converted(cur, cur, orig, false).is_empty());
        assert!(plan_file_converted(orig, orig, orig, true).is_empty());
        #[cfg(windows)]
        assert!(plan_file_converted(Path::new("C:/dir/photo.WEBP"), cur, orig, false).is_empty());
    }

    #[test]
    fn plan_creates_restored_original_when_plain_rename_keeps_original() {
        let orig = Path::new("C:/dir/photo.png");
        let cur = Path::new("C:/dir/photo.webp");
        assert_eq!(
            plan_file_converted(cur, cur, orig, true),
            vec![
                ShellEvent::Create(PathBuf::from("C:/dir/photo.png")),
                ShellEvent::UpdateDir(PathBuf::from("C:/dir")),
            ]
        );
    }

    #[test]
    fn plan_renames_to_final_and_updates_dir_when_not_kept() {
        let events = plan_file_converted(
            Path::new("C:/dir/photo.jpg@1080"),
            Path::new("C:/dir/photo.jpg"),
            Path::new("C:/dir/photo.png"),
            false,
        );
        assert_eq!(
            events,
            vec![
                ShellEvent::RenameItem {
                    from: PathBuf::from("C:/dir/photo.jpg@1080"),
                    to: PathBuf::from("C:/dir/photo.jpg"),
                },
                ShellEvent::UpdateDir(PathBuf::from("C:/dir")),
            ]
        );

        // Same-format resize without keep ends on original name.
        let resize = plan_file_converted(
            Path::new("C:/dir/photo.png@1080"),
            Path::new("C:/dir/photo.png"),
            Path::new("C:/dir/photo.png"),
            false,
        );
        assert_eq!(
            resize,
            vec![
                ShellEvent::RenameItem {
                    from: PathBuf::from("C:/dir/photo.png@1080"),
                    to: PathBuf::from("C:/dir/photo.png"),
                },
                ShellEvent::UpdateDir(PathBuf::from("C:/dir")),
            ]
        );
    }

    #[test]
    fn plan_restores_original_and_creates_final_when_kept() {
        let events = plan_file_converted(
            Path::new("C:/dir/photo.jpg&"),
            Path::new("C:/dir/photo.jpg"),
            Path::new("C:/dir/photo.png"),
            true,
        );
        assert_eq!(
            events,
            vec![
                ShellEvent::RenameItem {
                    from: PathBuf::from("C:/dir/photo.jpg&"),
                    to: PathBuf::from("C:/dir/photo.png"),
                },
                ShellEvent::Create(PathBuf::from("C:/dir/photo.jpg")),
                ShellEvent::UpdateDir(PathBuf::from("C:/dir")),
            ]
        );

        // Same-format resize with keep writes to a free name next to the original.
        let resize_keep = plan_file_converted(
            Path::new("C:/dir/photo.png&@1080"),
            Path::new("C:/dir/photo (2).png"),
            Path::new("C:/dir/photo.png"),
            true,
        );
        assert_eq!(
            resize_keep,
            vec![
                ShellEvent::RenameItem {
                    from: PathBuf::from("C:/dir/photo.png&@1080"),
                    to: PathBuf::from("C:/dir/photo.png"),
                },
                ShellEvent::Create(PathBuf::from("C:/dir/photo (2).png")),
                ShellEvent::UpdateDir(PathBuf::from("C:/dir")),
            ]
        );
    }

    #[test]
    fn plan_skips_redundant_events_when_paths_match_original() {
        // Already at original: only the new output file appeared.
        let copy_like = plan_file_converted(
            Path::new("C:/dir/photo.png"),
            Path::new("C:/dir/photo.jpg"),
            Path::new("C:/dir/photo.png"),
            true,
        );
        assert_eq!(
            copy_like,
            vec![
                ShellEvent::Create(PathBuf::from("C:/dir/photo.jpg")),
                ShellEvent::UpdateDir(PathBuf::from("C:/dir")),
            ]
        );

        // Final equals original while keeping: no extra Create for the same file.
        let same_as_orig = plan_file_converted(
            Path::new("C:/dir/photo.png&"),
            Path::new("C:/dir/photo.png"),
            Path::new("C:/dir/photo.png"),
            true,
        );
        assert_eq!(
            same_as_orig,
            vec![
                ShellEvent::RenameItem {
                    from: PathBuf::from("C:/dir/photo.png&"),
                    to: PathBuf::from("C:/dir/photo.png"),
                },
                ShellEvent::UpdateDir(PathBuf::from("C:/dir")),
            ]
        );
    }

    #[test]
    fn run_bounded_finishes_fast_work_and_gives_up_on_timeout() {
        assert!(run_bounded(Duration::from_secs(1), || {}));

        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let started = std::time::Instant::now();
        let ok = run_bounded(Duration::from_millis(50), move || {
            let _ = release_rx.recv();
        });
        let elapsed = started.elapsed();
        let _ = release_tx.send(());
        assert!(!ok);
        assert!(elapsed < Duration::from_secs(1), "took {elapsed:?}");
    }

    #[test]
    fn notice_guard_caps_concurrent_workers_and_releases_on_drop() {
        let counter = std::sync::atomic::AtomicUsize::new(0);
        let guards: Vec<_> = (0..4)
            .map(|_| NoticeGuard::try_acquire(&counter, 4).expect("within limit"))
            .collect();
        assert!(NoticeGuard::try_acquire(&counter, 4).is_none());
        drop(guards);
        assert_eq!(counter.load(std::sync::atomic::Ordering::Acquire), 0);
        assert!(NoticeGuard::try_acquire(&counter, 4).is_some());
    }
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
        // Shell notices need a signed-in desktop; CI runners have none.
        if std::env::var_os("CI").is_some() {
            return;
        }
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
    fn file_wait_ends_when_the_rename_is_announced() {
        // Shell notices need a signed-in desktop; CI runners have none.
        if std::env::var_os("CI").is_some() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("photo.png");
        let new = dir.path().join("photo.webp&");
        std::fs::write(&new, b"x").unwrap();
        let (from, to) = (old.clone(), new.clone());
        let announcer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            platform::send_events(&[ShellEvent::RenameItem { from, to }]);
        });
        let started = Instant::now();
        let heard = wait_for_rename(&new, Duration::from_secs(20), RenameKind::File);
        announcer.join().unwrap();
        assert!(heard, "no file rename notice arrived");
        assert!(started.elapsed() < Duration::from_secs(15));
    }

    #[test]
    fn without_a_notice_the_full_wait_passes() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("Holiday.webp");
        std::fs::create_dir(&folder).unwrap();
        let started = Instant::now();
        assert!(!wait_for_folder_rename(&folder, Duration::from_millis(400)));
        assert!(started.elapsed() >= Duration::from_millis(400));

        let file = dir.path().join("photo.webp");
        std::fs::write(&file, b"x").unwrap();
        let started = Instant::now();
        assert!(!wait_for_rename(
            &file,
            Duration::from_millis(400),
            RenameKind::File
        ));
        assert!(started.elapsed() >= Duration::from_millis(400));
    }
}
