//! `zero_fs` — filesystem safety primitives for ZeroConverter.
//!
//! Owns the physical side of file handling:
//! - root containment ([`ensure_within_root`])
//! - one way to put a finished file in place ([`commit`], [`write_output`])
//! - atomic writes ([`write_atomic`], [`save_json_atomic`])
//! - scratch directories that clean up after themselves ([`TempWorkspace`])
//!
//! It has no knowledge of conversion, jobs, Tauri or the frontend.

use std::fmt;
use std::fs::File;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Upper bound for `name (1).ext`, `name (2).ext`, ... auto-rename attempts.
pub const MAX_AUTO_RENAME_ATTEMPTS: u32 = 1000;

// ── Unique names ────────────────────────────────────────────────────────────

static UNIQUE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Process-unique token: pid + wall-clock nanos + monotonic counter.
/// Collision-free within a process; across processes the pid separates them.
fn unique_token() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let n = UNIQUE_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{:x}{:x}{:x}", std::process::id(), nanos, n)
}

// ── Root containment ────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum RootPathError {
    RootMissing(PathBuf),
    RootNotDirectory(PathBuf),
    AncestorNotDirectory(PathBuf),
    EscapesRoot { root: PathBuf, candidate: PathBuf },
    Io(io::Error),
}

impl fmt::Display for RootPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RootMissing(p) => write!(f, "root does not exist: {}", p.display()),
            Self::RootNotDirectory(p) => write!(f, "root is not a directory: {}", p.display()),
            Self::AncestorNotDirectory(p) => {
                write!(f, "path component is not a directory: {}", p.display())
            }
            Self::EscapesRoot { root, candidate } => write!(
                f,
                "path {} escapes root {}",
                candidate.display(),
                root.display()
            ),
            Self::Io(e) => write!(f, "io error: {}", e),
        }
    }
}

impl std::error::Error for RootPathError {}

/// Verifies that `candidate` resolves to a location inside `root` and returns the
/// resolved path. `candidate` does not have to exist yet; `root` does.
///
/// Symlinks and `..` are resolved through the deepest existing ancestor, so a
/// link inside the root that points outside it is rejected.
pub fn ensure_within_root(root: &Path, candidate: &Path) -> Result<PathBuf, RootPathError> {
    let canon_root = match dunce::canonicalize(root) {
        Ok(p) => p,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Err(RootPathError::RootMissing(root.to_path_buf()))
        }
        Err(e) => return Err(RootPathError::Io(e)),
    };
    if !canon_root.is_dir() {
        return Err(RootPathError::RootNotDirectory(root.to_path_buf()));
    }

    let absolute = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        canon_root.join(candidate)
    };

    // Split into the deepest existing ancestor and the not-yet-existing tail.
    let mut existing = absolute.clone();
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    let canon_existing = loop {
        match dunce::canonicalize(&existing) {
            Ok(p) => break p,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let escapes = || RootPathError::EscapesRoot {
                    root: canon_root.clone(),
                    candidate: candidate.to_path_buf(),
                };
                match existing.components().next_back() {
                    Some(Component::Normal(name)) => tail.push(name.to_os_string()),
                    // `..` / `.` below a missing directory cannot be resolved safely.
                    _ => return Err(escapes()),
                }
                if !existing.pop() {
                    return Err(escapes());
                }
            }
            Err(e) => return Err(RootPathError::Io(e)),
        }
    };

    if !tail.is_empty() && !canon_existing.is_dir() {
        return Err(RootPathError::AncestorNotDirectory(canon_existing));
    }

    let mut resolved = canon_existing;
    for name in tail.iter().rev() {
        resolved.push(name);
    }

    if !resolved.starts_with(&canon_root) {
        return Err(RootPathError::EscapesRoot {
            root: canon_root,
            candidate: candidate.to_path_buf(),
        });
    }
    Ok(resolved)
}

// ── Atomic writes ───────────────────────────────────────────────────────────

/// Durability requested for an atomic write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncPolicy {
    /// Records that must survive a crash (manifests): fsync before rename.
    Authoritative,
    /// Regenerable data (conversion outputs, thumbnails): no fsync. The rename
    /// still guarantees readers never observe a half-written file.
    Disposable,
}

/// Removes the temp file unless the write was committed.
struct TempFileGuard {
    path: PathBuf,
    committed: bool,
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        if !self.committed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// What [`commit`] does when the destination already exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clobber {
    /// Fail with `AlreadyExists`. Decided at the moment of the move, so a file
    /// that appeared after the output name was chosen is never replaced.
    Never,
    /// Replace it (the user ticked "Overwrite existing files").
    Replace,
}

/// The one way a finished temporary file becomes the output. `temp` and
/// `dest` must be on the same volume.
pub fn commit(temp: &Path, dest: &Path, clobber: Clobber) -> io::Result<()> {
    match clobber {
        Clobber::Replace => std::fs::rename(temp, dest),
        Clobber::Never => rename_no_replace(temp, dest),
    }
}

/// Moves `from` to `to` on the same volume, failing with `AlreadyExists` if
/// `to` exists. The check and the move are one step, so a file that appears
/// in between is never replaced. Works for files and folders.
#[cfg(windows)]
pub fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;
    let wide = |p: &Path| -> Vec<u16> { p.as_os_str().encode_wide().chain(Some(0)).collect() };
    let (from_w, to_w) = (wide(from), wide(to));
    // No MOVEFILE_REPLACE_EXISTING: the call fails when `to` exists.
    // No MOVEFILE_COPY_ALLOWED: a cross-volume move fails instead of copying.
    if unsafe { MoveFileExW(from_w.as_ptr(), to_w.as_ptr(), 0) } != 0 {
        return Ok(());
    }
    let e = io::Error::last_os_error();
    if matches!(e.raw_os_error(), Some(80 | 183)) {
        // ERROR_FILE_EXISTS, ERROR_ALREADY_EXISTS
        return Err(already_exists(to));
    }
    Err(e)
}

/// Elsewhere: a hard link cannot be made over an existing name, so files get
/// the same one-step check. Where hard links are not possible (some volumes,
/// folders) it fails instead of checking first and renaming after, which
/// could replace a file that appeared in between.
#[cfg(not(windows))]
pub fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    match std::fs::hard_link(from, to) {
        Ok(()) => std::fs::remove_file(from),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Err(already_exists(to)),
        Err(e) => Err(io::Error::new(
            e.kind(),
            format!(
                "{} cannot be moved here without risking another file: {e}",
                from.display()
            ),
        )),
    }
}

// ── Input guard ─────────────────────────────────────────────────────────────

/// Holds an input file open for as long as a job needs it. On Windows others
/// may still read it, but nothing can delete, rename or replace it until the
/// guard is dropped, so no other job, process or alias path can overwrite an
/// input that is waiting to be converted. Elsewhere it is a plain open file.
pub struct InputGuard {
    _file: File,
}

impl InputGuard {
    /// `None` when the file cannot be opened that way (missing, or already
    /// open for writing elsewhere); the job then runs unguarded, as before.
    pub fn hold(path: &Path) -> Option<Self> {
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            const FILE_SHARE_READ: u32 = 0x1;
            options.share_mode(FILE_SHARE_READ);
        }
        options.open(path).ok().map(|file| Self { _file: file })
    }
}

fn already_exists(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("{} already exists", path.display()),
    )
}

/// A converted file: written next to `path` under a temporary name, then
/// [`commit`]ted. No fsync: outputs can be made again.
pub fn write_output(path: &Path, bytes: &[u8], clobber: Clobber) -> io::Result<()> {
    write_then_commit(path, bytes, SyncPolicy::Disposable, clobber)
}

/// Writes `bytes` to `<name>.tmp.<unique>` next to `path`, then renames it over
/// `path`. On any failure the temp file is removed and `path` is left untouched.
pub fn write_atomic(path: &Path, bytes: &[u8], policy: SyncPolicy) -> io::Result<()> {
    write_then_commit(path, bytes, policy, Clobber::Replace)
}

fn write_then_commit(
    path: &Path,
    bytes: &[u8],
    policy: SyncPolicy,
    clobber: Clobber,
) -> io::Result<()> {
    let file_name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
    let mut temp_name = file_name.to_os_string();
    temp_name.push(format!(".tmp.{}", unique_token()));
    let temp_path = path.with_file_name(temp_name);

    let mut guard = TempFileGuard {
        path: temp_path,
        committed: false,
    };

    {
        let mut file = File::create(&guard.path)?;
        file.write_all(bytes)?;
        if policy == SyncPolicy::Authoritative {
            file.sync_all()?;
        }
    } // handle released before rename (required on Windows)

    commit(&guard.path, path, clobber)?;
    guard.committed = true;
    Ok(())
}

/// Serializes `value` as pretty JSON and writes it with [`write_atomic`].
pub fn save_json_atomic<T: serde::Serialize>(
    path: &Path,
    value: &T,
    policy: SyncPolicy,
) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    write_atomic(path, &bytes, policy)
}

// ── Temp workspace ──────────────────────────────────────────────────────────

/// A `zero_tmp_<unique>` scratch directory, removed (with its contents) on drop.
/// Create it next to the final output so the committing rename stays on one volume.
#[derive(Debug)]
pub struct TempWorkspace {
    path: PathBuf,
}

impl TempWorkspace {
    pub fn create(parent: &Path) -> io::Result<Self> {
        let path = parent.join(format!("zero_tmp_{}", unique_token()));
        std::fs::create_dir(&path)?;
        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempWorkspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn commit_never_refuses_a_file_that_appeared_meanwhile() {
        let dir = tempfile::tempdir().unwrap();
        let temp = dir.path().join("out.tmp");
        let dest = dir.path().join("out.png");
        std::fs::write(&temp, b"new").unwrap();
        std::fs::write(&dest, b"someone else's").unwrap();
        let err = commit(&temp, &dest, Clobber::Never).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&dest).unwrap(), b"someone else's");
        // Replace is the user's explicit choice.
        commit(&temp, &dest, Clobber::Replace).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"new");
    }

    #[test]
    fn commit_never_moves_into_a_free_name() {
        let dir = tempfile::tempdir().unwrap();
        let temp = dir.path().join("out.tmp");
        let dest = dir.path().join("out.png");
        std::fs::write(&temp, b"new").unwrap();
        commit(&temp, &dest, Clobber::Never).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"new");
        assert!(!temp.exists());
    }

    #[test]
    fn write_output_never_leaves_the_temp_file_behind() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("a.webp");
        std::fs::write(&dest, b"old").unwrap();
        assert!(write_output(&dest, b"new", Clobber::Never).is_err());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    use super::*;

    #[test]
    fn containment_accepts_missing_file_inside_root() {
        let root = tempfile::tempdir().unwrap();
        let p = ensure_within_root(root.path(), &root.path().join("sub").join("a.png")).unwrap();
        assert!(p.ends_with(Path::new("sub").join("a.png")));
    }

    #[test]
    fn containment_rejects_parent_escape() {
        let root = tempfile::tempdir().unwrap();
        let inner = root.path().join("inner");
        std::fs::create_dir(&inner).unwrap();
        let err = ensure_within_root(&inner, &inner.join("..").join("x.png")).unwrap_err();
        assert!(matches!(err, RootPathError::EscapesRoot { .. }));
        // `..` below a directory that does not exist yet
        let err = ensure_within_root(&inner, &inner.join("nope").join("..").join("..").join("x"))
            .unwrap_err();
        assert!(matches!(err, RootPathError::EscapesRoot { .. }));
    }

    #[test]
    fn containment_reports_root_and_ancestor_problems() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("f.txt");
        std::fs::write(&file, b"x").unwrap();
        assert!(matches!(
            ensure_within_root(&root.path().join("missing"), &file),
            Err(RootPathError::RootMissing(_))
        ));
        assert!(matches!(
            ensure_within_root(&file, &file),
            Err(RootPathError::RootNotDirectory(_))
        ));
        assert!(matches!(
            ensure_within_root(root.path(), &file.join("child.png")),
            Err(RootPathError::AncestorNotDirectory(_))
        ));
    }

    #[test]
    fn write_atomic_replaces_and_leaves_no_debris() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("out.bin");
        write_atomic(&target, b"one", SyncPolicy::Disposable).unwrap();
        write_atomic(&target, b"two", SyncPolicy::Authoritative).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"two");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn write_atomic_cleans_up_when_rename_fails() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("is_a_dir");
        std::fs::create_dir(&target).unwrap();
        assert!(write_atomic(&target, b"x", SyncPolicy::Disposable).is_err());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn temp_workspace_removes_itself() {
        let dir = tempfile::tempdir().unwrap();
        let kept;
        {
            let ws = TempWorkspace::create(dir.path()).unwrap();
            kept = ws.path().to_path_buf();
            std::fs::write(ws.path().join("a.tmp"), b"x").unwrap();
            assert!(kept
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("zero_tmp_"));
        }
        assert!(!kept.exists());
    }
}

#[cfg(test)]
mod rename_no_replace_tests {
    use super::*;

    #[test]
    fn refuses_an_existing_file_or_folder_and_moves_otherwise() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("a.txt");
        let taken = dir.path().join("b.txt");
        std::fs::write(&from, b"a").unwrap();
        std::fs::write(&taken, b"b").unwrap();
        let err = rename_no_replace(&from, &taken).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&taken).unwrap(), b"b");

        let free = dir.path().join("c.txt");
        rename_no_replace(&from, &free).unwrap();
        assert!(!from.exists());
        assert_eq!(std::fs::read(&free).unwrap(), b"a");

        let folder = dir.path().join("folder");
        std::fs::create_dir(&folder).unwrap();
        assert_eq!(
            rename_no_replace(&free, &folder).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
    }
}

#[cfg(all(test, windows))]
mod input_guard_tests {
    use super::*;

    #[test]
    fn a_guarded_input_can_be_read_but_not_replaced_or_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("in.png");
        let other = dir.path().join("other.png");
        std::fs::write(&input, b"input").unwrap();
        std::fs::write(&other, b"other").unwrap();
        let guard = InputGuard::hold(&input).expect("guard");
        assert_eq!(std::fs::read(&input).unwrap(), b"input");
        assert!(std::fs::rename(&other, &input).is_err(), "replace refused");
        assert!(commit(&other, &input, Clobber::Replace).is_err());
        assert!(std::fs::remove_file(&input).is_err(), "delete refused");
        assert_eq!(std::fs::read(&input).unwrap(), b"input");
        drop(guard);
        commit(&other, &input, Clobber::Replace).unwrap();
        assert_eq!(std::fs::read(&input).unwrap(), b"other");
    }
}
