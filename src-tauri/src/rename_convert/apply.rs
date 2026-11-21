// ==============================================================================
// OWNERSHIP:
//   Turns a "convert" decision into files on disk: the renamed file moves to a
//   free name (its original one when possible), the converted file takes the
//   final name, and the original goes to the Recycle Bin. Every move refuses
//   an existing name, and a file that was replaced meanwhile is left alone.
// ==============================================================================
use crate::core::app_error::AppError;
use crate::core::conversion::registry::ConverterRegistry;
use crate::core::conversion::types::{ConversionRequest, OutputFormat};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Options from the `+` window, passed to the converter as they are (the
/// format family checks them). Without the window: the ZeroConverter
/// quality standard.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct QuickOptions(pub serde_json::Map<String, serde_json::Value>);

impl Default for QuickOptions {
    fn default() -> Self {
        let mut map = serde_json::Map::new();
        map.insert("quality".into(), 90.into());
        map.insert("resize_mode".into(), "none".into());
        Self(map)
    }
}

#[cfg(test)]
impl QuickOptions {
    fn with_quality(quality: u8) -> Self {
        let mut options = Self::default();
        options.0.insert("quality".into(), quality.into());
        options
    }
}

#[derive(Debug, Clone)]
pub struct Job {
    /// Where the file is now (the name the user typed, `+` included).
    pub current: PathBuf,
    /// The name it had before the rename.
    pub original: PathBuf,
    /// Where the converted file should end up.
    pub final_path: PathBuf,
    pub source: String,
    pub target: String,
    /// `&`: the original stays where it was instead of going to the bin.
    pub keep_original: bool,
    /// `@N`: longest side at most N pixels (images only).
    pub max_size: Option<u32>,
    /// From the right-click menu: a copy, not a rename. The rename-to-convert
    /// switch does not stop it; only quitting does.
    pub from_menu: bool,
}

#[derive(Debug, Clone)]
pub struct Done {
    pub final_path: PathBuf,
    pub elapsed: Duration,
    pub original_recycled: bool,
}

/// Converts and swaps. On failure the file is renamed back to its original
/// name so nothing is left looking broken.
pub fn run(
    job: &Job,
    registry: &ConverterRegistry,
    options: &QuickOptions,
) -> Result<Done, AppError> {
    run_unless(job, registry, options, &|| false)
}

/// Like [`run`], but `stop` is asked before starting and again right before
/// the files are swapped: when it says yes (rename-to-convert was turned off,
/// or the app is quitting) nothing is changed and the name goes back.
pub fn run_unless(
    job: &Job,
    registry: &ConverterRegistry,
    options: &QuickOptions,
    stop: &dyn Fn() -> bool,
) -> Result<Done, AppError> {
    let started = Instant::now();
    // Which file this is: if something else is put under the name meanwhile,
    // that file is left alone (not swapped, binned or renamed back).
    let mut identity = None;
    let result = wait_until_readable(&job.current).and_then(|()| {
        super::debug(&format!("readable after {:?}", started.elapsed()));
        if stop() {
            return Err(AppError::Cancelled);
        }
        identity = file_id(&job.current);
        convert_and_swap(job, registry, options, identity, stop)
    });
    // Never identified (it was never touched), or still the same file: it
    // gets its old name back. A file that took the name is left alone.
    if result.is_err() && (identity.is_none() || still_ours(&job.current, identity)) {
        restore_name(job);
    }
    result.map(|recycled| Done {
        final_path: job.final_path.clone(),
        elapsed: started.elapsed(),
        original_recycled: recycled,
    })
}

/// Puts the file back under the name it had before the rename, never over
/// a file that took that name meanwhile (then it keeps its current name).
pub fn restore_name(job: &Job) {
    let _ = zero_fs::rename_no_replace(&job.current, &job.original);
}

/// Which file this is: what the file system says (volume, index, size, last
/// write) plus a fingerprint of its first and last bytes. The fingerprint
/// matters on FAT drives, where a file put under the same name can get the
/// same index and the last write is kept only to two seconds.
type FileId = (platform::FileId, u64);

fn file_id(path: &Path) -> Option<FileId> {
    Some((platform::file_id(path)?, sample(path)?))
}

/// Hash of the first and last 4 KB.
fn sample(path: &Path) -> Option<u64> {
    use std::hash::Hasher;
    use std::io::{Read, Seek, SeekFrom};
    const PART: u64 = 4096;
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let mut buf = vec![0u8; PART as usize];
    let n = file.read(&mut buf).ok()?;
    hasher.write(&buf[..n]);
    if len > PART {
        file.seek(SeekFrom::Start(len.saturating_sub(PART).max(PART)))
            .ok()?;
        let n = file.read(&mut buf).ok()?;
        hasher.write(&buf[..n]);
    }
    Some(hasher.finish())
}

/// True when `path` is still the file first seen, unchanged. False when no
/// identity is known: nothing is swapped or binned on a guess.
fn still_ours(path: &Path, identity: Option<FileId>) -> bool {
    identity.is_some() && file_id(path) == identity
}

fn convert_and_swap(
    job: &Job,
    registry: &ConverterRegistry,
    options: &QuickOptions,
    identity: Option<FileId>,
    stop: &dyn Fn() -> bool,
) -> Result<bool, AppError> {
    // Every later step checks that the file is still this one. With no way
    // to tell, nothing is touched.
    if identity.is_none() {
        return Err(AppError::Io(format!(
            "{} could not be checked, so it was left as it is",
            file_name(&job.current)
        )));
    }
    let dir = job
        .current
        .parent()
        .ok_or_else(|| AppError::Io("File has no parent folder".into()))?;
    let work = WorkDir::create(dir)?;

    // The converter reads formats by extension, so give it a correctly named
    // view of the same bytes: a hard link costs nothing; copy if the volume
    // has no hard links (FAT, some network shares).
    let source = work.path.join(format!("src.{}", job.source));
    let linked = std::fs::hard_link(&job.current, &source).is_ok();
    if !linked {
        std::fs::copy(&job.current, &source)
            .map_err(|e| AppError::Io(format!("Could not read the file: {e}")))?;
    }

    // Where the original is as far as this job knows; `None` once it is in
    // the Recycle Bin. `source` goes with the work folder only while the
    // original is confirmed there.
    let mut original_at = Some(job.current.clone());
    let result = convert_then_swap(
        job,
        registry,
        options,
        identity,
        stop,
        &work.path,
        &source,
        &mut original_at,
    );

    let only_copy = match &original_at {
        None => false,
        // Our link is the last name the original's bytes have.
        Some(_) if linked => !matches!(platform::link_count(&source), Some(n) if n > 1),
        Some(at) => !still_ours(at, identity),
    };
    if !only_copy {
        return result;
    }
    // Something else took the original's place (another program, or a queue
    // job writing there): put the original back in the folder, visibly.
    let rescued = free_name_besides(&job.original, &job.final_path)
        .filter(|to| zero_fs::rename_no_replace(&source, to).is_ok());
    let kept = match &rescued {
        Some(to) => format!("the original is now named {}", file_name(to)),
        None => {
            // The scratch folder holds the only copy: never delete it.
            std::mem::forget(work);
            format!("the original was kept in {}", source.display())
        }
    };
    Err(AppError::Io(match result {
        Err(e) => format!("{}; {kept}", plain(&e)),
        Ok(_) => format!(
            "{} was converted, but the original was changed by another program before it could go to the Recycle Bin; {kept}",
            file_name(&job.final_path)
        ),
    }))
}

/// Converts `source` in the work folder, then swaps the result in. Keeps
/// `original_at` current at every step.
#[allow(clippy::too_many_arguments)]
fn convert_then_swap(
    job: &Job,
    registry: &ConverterRegistry,
    options: &QuickOptions,
    identity: Option<FileId>,
    stop: &dyn Fn() -> bool,
    work: &Path,
    source: &Path,
    original_at: &mut Option<PathBuf>,
) -> Result<bool, AppError> {
    let stem = job
        .final_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file")
        .to_string();
    let output = work.join(format!("{stem}.{}", job.target));

    let family = registry.resolve_family(&job.source);
    let request = ConversionRequest {
        family: family.clone(),
        input_path: source.to_string_lossy().into_owned(),
        output_path: output.to_string_lossy().into_owned(),
        output_root: work.to_string_lossy().into_owned(),
        output_format: OutputFormat(job.target.clone()),
        options: {
            let mut map = options.0.clone();
            // The work folder is new and ours: the output lands there freely.
            map.insert("overwrite".into(), true.into());
            // `@N` in the name wins over the window's resize choice.
            if let Some(n) = job.max_size {
                map.insert("resize_mode".into(), "fit_box".into());
                map.insert("resize_width".into(), n.into());
                map.insert("resize_height".into(), n.into());
            }
            serde_json::Value::Object(map)
        },
    };
    let t = Instant::now();
    registry.validate(family, &request)?;
    let result = crate::core::conversion::pipeline::convert(
        registry,
        &request,
        &crate::core::conversion::traits::ExecContext::default(),
    )?;
    super::debug(&format!("convert {:?}", t.elapsed()));
    if !result.success {
        return Err(AppError::Execute(result.message));
    }
    if stop() {
        return Err(AppError::Cancelled);
    }

    // On the Desktop the new file would land in the first free slot; note
    // where the icon is now. Tests have no Desktop to ask.
    let spot = if cfg!(test) || job.keep_original {
        None
    } else {
        super::desktop::remember(&job.current)
    };
    let started_swap = Instant::now();
    let replaced = || {
        AppError::Io(format!(
            "{} was replaced or changed by another program while converting; nothing was changed",
            file_name(&job.current)
        ))
    };
    if !still_ours(&job.current, identity) {
        return Err(replaced());
    }
    // A file converted where it lies (folder conversion) keeps its own name.
    // Otherwise the original goes back to its old name, unless the result
    // takes that name (`photo.png` → `photo.png@1080`): then it moves aside
    // first, so it is never overwritten by the result or binned in its place.
    let backup = if job.current == job.original {
        job.current.clone()
    } else {
        free_name_besides(&job.original, &job.final_path).ok_or_else(|| {
            AppError::Io("No free name is left for the original next to it".into())
        })?
    };
    // Two moves: the original to `backup`, the result to the final name.
    // Each refuses an existing name at the moment of the move, so a file that
    // appeared since the names were chosen is never replaced.
    let refused = |path: &Path, e: std::io::Error| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            AppError::Io(format!("{} already exists", file_name(path)))
        } else {
            AppError::Io(e.to_string())
        }
    };
    if backup != job.current {
        zero_fs::rename_no_replace(&job.current, &backup).map_err(|e| refused(&backup, e))?;
        *original_at = Some(backup.clone());
        // Another file may have taken the name in the instant before the
        // move: put it back and change nothing.
        if !still_ours(&backup, identity) {
            if zero_fs::rename_no_replace(&backup, &job.current).is_ok() {
                *original_at = Some(job.current.clone());
            }
            return Err(replaced());
        }
    }
    if let Err(e) = zero_fs::rename_no_replace(&output, &job.final_path) {
        let err = refused(&job.final_path, e);
        if backup != job.current {
            if zero_fs::rename_no_replace(&backup, &job.current).is_err() {
                // Say where the original is when it cannot go back.
                return Err(AppError::Io(format!(
                    "{}; the original is now named {}",
                    plain(&err),
                    file_name(&backup)
                )));
            }
            *original_at = Some(job.current.clone());
        }
        return Err(err);
    }
    super::debug(&format!("swapped {:?}", started_swap.elapsed()));
    if job.keep_original {
        // The original is back under its own name, next to the new file.
        return Ok(false);
    }
    if let Some(spot) = spot {
        super::desktop::restore(job.final_path.clone(), spot);
    }
    // Only the file that was converted goes to the bin.
    if !still_ours(&backup, identity) {
        return Ok(false);
    }
    let t = Instant::now();
    let recycled = recycle(&backup);
    super::debug(&format!("recycle {:?}", t.elapsed()));
    if recycled {
        *original_at = None;
    }
    Ok(recycled)
}

fn file_name(path: &Path) -> std::borrow::Cow<'_, str> {
    path.file_name().unwrap_or_default().to_string_lossy()
}

/// The message without the "IO error:" kind in front, for joining.
fn plain(e: &AppError) -> String {
    match e {
        AppError::Io(m)
        | AppError::Execute(m)
        | AppError::Validate(m)
        | AppError::Probe(m)
        | AppError::Internal(m)
        | AppError::Registry(m) => m.clone(),
        AppError::Cancelled => "stopped before the files were swapped".into(),
        other => other.to_string(),
    }
}

/// Sends the original to the Recycle Bin. Tests delete instead so runs do
/// not fill the developer's bin.
fn recycle(path: &Path) -> bool {
    #[cfg(not(test))]
    {
        trash::delete(path).is_ok()
    }
    #[cfg(test)]
    {
        std::fs::remove_file(path).is_ok()
    }
}

/// Explorer and download managers may still hold the file for a moment.
fn wait_until_readable(path: &Path) -> Result<(), AppError> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match std::fs::OpenOptions::new().read(true).open(path) {
            Ok(_) => return Ok(()),
            Err(e) if Instant::now() < deadline => {
                let _ = e;
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(AppError::Io(format!("File is in use: {e}"))),
        }
    }
}

/// Whether two paths name the same file (Windows names ignore case).
fn same_name(a: &Path, b: &Path) -> bool {
    a.to_string_lossy()
        .eq_ignore_ascii_case(&b.to_string_lossy())
}

/// Like [`free_name`], but `taken` counts as in use even when no file has
/// that name yet. `None` when every candidate is in use.
pub(crate) fn free_name_besides(path: &Path, taken: &Path) -> Option<PathBuf> {
    if !path.exists() && !same_name(path, taken) {
        return Some(path.to_path_buf());
    }
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    (2..1000)
        .map(|i| path.with_file_name(format!("{stem} ({i}).{ext}")))
        .find(|p| !p.exists() && !same_name(p, taken))
}

/// Where the converted file goes. When it would take the original's own name
/// (same format with `@N` or `+`) and the original is kept, the new file gets
/// a free name next to it instead.
pub(crate) fn final_path_for(clean: &Path, original: &Path, keep_original: bool) -> PathBuf {
    if keep_original && same_name(clean, original) {
        // With every name taken the move later refuses; nothing is replaced.
        free_name_besides(clean, original).unwrap_or_else(|| clean.to_path_buf())
    } else {
        clean.to_path_buf()
    }
}

/// `name.ext`, or `name (2).ext` if that is taken.
pub(crate) fn free_name(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    (2..1000)
        .map(|i| path.with_file_name(format!("{stem} ({i}).{ext}")))
        .find(|p| !p.exists())
        .unwrap_or_else(|| path.to_path_buf())
}

/// Hidden scratch folder next to the file (same volume, so the final swap is
/// a rename, never a copy).
struct WorkDir {
    path: PathBuf,
}

impl WorkDir {
    fn create(dir: &Path) -> Result<Self, AppError> {
        let path = dir.join(format!(".zc-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir(&path)
            .map_err(|e| AppError::Io(format!("Cannot write in this folder: {e}")))?;
        platform::hide(&path);
        Ok(Self { path })
    }
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[cfg(windows)]
mod platform {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileAttributesW, GetFileInformationByHandle, SetFileAttributesW,
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_HIDDEN, INVALID_FILE_ATTRIBUTES,
    };

    /// Volume serial, file index, size and last write: the same file under
    /// any name, and not written to since.
    pub type FileId = (u32, u64, u64, u64);

    fn info(path: &Path) -> Option<BY_HANDLE_FILE_INFORMATION> {
        use std::os::windows::io::AsRawHandle;
        let file = std::fs::File::open(path).ok()?;
        // SAFETY: a plain-data struct, valid when zeroed; `file` keeps the
        // handle open for the call, and `info` outlives it.
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut info) };
        (ok != 0).then_some(info)
    }

    pub fn file_id(path: &Path) -> Option<FileId> {
        let i = info(path)?;
        let wide = |high: u32, low: u32| (u64::from(high) << 32) | u64::from(low);
        Some((
            i.dwVolumeSerialNumber,
            wide(i.nFileIndexHigh, i.nFileIndexLow),
            wide(i.nFileSizeHigh, i.nFileSizeLow),
            wide(
                i.ftLastWriteTime.dwHighDateTime,
                i.ftLastWriteTime.dwLowDateTime,
            ),
        ))
    }

    /// How many names the file has.
    pub fn link_count(path: &Path) -> Option<u32> {
        info(path).map(|i| i.nNumberOfLinks)
    }

    fn wide(p: &Path) -> Vec<u16> {
        p.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    pub fn hide(p: &Path) {
        let w = wide(p);
        unsafe {
            let attrs = GetFileAttributesW(w.as_ptr());
            if attrs != INVALID_FILE_ATTRIBUTES {
                SetFileAttributesW(w.as_ptr(), attrs | FILE_ATTRIBUTE_HIDDEN);
            }
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use std::os::unix::fs::MetadataExt;
    use std::path::Path;

    /// Device, inode, size and last write: the same file under any name, and
    /// not written to since.
    pub type FileId = (u64, u64, u64, i64, i64);

    pub fn file_id(path: &Path) -> Option<FileId> {
        std::fs::metadata(path)
            .ok()
            .map(|m| (m.dev(), m.ino(), m.len(), m.mtime(), m.mtime_nsec()))
    }

    /// How many names the file has.
    pub fn link_count(path: &Path) -> Option<u32> {
        std::fs::metadata(path).ok().map(|m| m.nlink() as u32)
    }

    pub fn hide(_p: &Path) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::conversion::types::AssetFamily;
    use std::sync::Arc;

    fn registry() -> ConverterRegistry {
        let mut r = ConverterRegistry::new();
        r.register(
            AssetFamily::Image,
            Arc::new(crate::core::handlers::image::ImageConverterHandler),
        );
        r.register(
            AssetFamily::Audio,
            Arc::new(crate::core::handlers::audio::AudioHandler),
        );
        r
    }

    fn png_bytes() -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(8, 8, image::Rgba([200, 30, 30, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn renamed_png_becomes_a_real_webp_and_leaves_no_debris() {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("photo.png");
        let current = dir.path().join("photo.webp");
        std::fs::write(&current, png_bytes()).unwrap(); // the user's rename already happened
        let job = Job {
            current: current.clone(),
            original: original.clone(),
            final_path: current.clone(),
            source: "png".into(),
            target: "webp".into(),
            keep_original: false,
            max_size: None,
            from_menu: false,
        };
        let done = run(&job, &registry(), &QuickOptions::default()).unwrap();
        assert_eq!(done.final_path, current);
        assert_eq!(crate::core::sniff::sniff_file(&current), Some("webp"));
        // Only the converted file remains (the original went to the bin, or
        // stays as photo.png where no bin exists, e.g. on CI temp volumes).
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n != "photo.png")
            .collect();
        assert_eq!(names, vec!["photo.webp".to_string()]);
    }

    /// The old name was taken by another file while converting: a failed
    /// conversion leaves both files as they are instead of renaming over it.
    #[test]
    fn rename_back_never_replaces_a_file_that_took_the_old_name() {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("broken.png");
        let current = dir.path().join("broken.webp");
        std::fs::write(&current, b"\x89PNG\r\n\x1a\nnot really").unwrap();
        std::fs::write(&original, b"someone else's").unwrap();
        let job = Job {
            current: current.clone(),
            original: original.clone(),
            final_path: current.clone(),
            source: "png".into(),
            target: "webp".into(),
            keep_original: false,
            max_size: None,
            from_menu: false,
        };
        assert!(run(&job, &registry(), &QuickOptions::default()).is_err());
        assert_eq!(std::fs::read(&original).unwrap(), b"someone else's");
        assert!(current.exists(), "the renamed file keeps its name");
    }

    /// Turned off (or quitting) before the swap: nothing changes and the
    /// file gets its old name back.
    #[test]
    fn a_stopped_job_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let current = dir.path().join("photo.webp");
        std::fs::write(&current, png_bytes()).unwrap();
        let before = std::fs::read(&current).unwrap();
        let job = Job {
            current: current.clone(),
            original: dir.path().join("photo.png"),
            final_path: current.clone(),
            source: "png".into(),
            target: "webp".into(),
            keep_original: false,
            max_size: None,
            from_menu: false,
        };
        let result = run_unless(&job, &registry(), &QuickOptions::default(), &|| true);
        assert!(matches!(result, Err(AppError::Cancelled)));
        assert_eq!(std::fs::read(dir.path().join("photo.png")).unwrap(), before);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    /// Runs a `photo.png` → `photo.webp` job; `meanwhile` runs once the
    /// conversion is done, right before the swap (`stop` answers with it).
    fn run_with_meanwhile(
        dir: &Path,
        meanwhile: impl Fn(&Path) -> bool,
    ) -> (Result<Done, AppError>, PathBuf, Vec<u8>) {
        let current = dir.join("photo.webp");
        std::fs::write(&current, png_bytes()).unwrap();
        let before = std::fs::read(&current).unwrap();
        let job = Job {
            current: current.clone(),
            original: dir.join("photo.png"),
            final_path: current.clone(),
            source: "png".into(),
            target: "webp".into(),
            keep_original: false,
            max_size: None,
            from_menu: false,
        };
        let calls = std::cell::Cell::new(0);
        let stop = || {
            calls.set(calls.get() + 1);
            calls.get() > 1 && meanwhile(&current)
        };
        let result = run_unless(&job, &registry(), &QuickOptions::default(), &stop);
        (result, current, before)
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// Another file took the name while converting, then the job stopped:
    /// the work folder held the original's last name, so it comes back.
    #[test]
    fn a_stopped_job_never_deletes_the_last_copy() {
        let dir = tempfile::tempdir().unwrap();
        let (result, current, before) = run_with_meanwhile(dir.path(), |current| {
            std::fs::remove_file(current).unwrap();
            std::fs::write(current, b"someone else's").unwrap();
            true
        });
        assert!(result.is_err());
        assert_eq!(std::fs::read(&current).unwrap(), b"someone else's");
        assert_eq!(std::fs::read(dir.path().join("photo.png")).unwrap(), before);
        assert_eq!(names(dir.path()), ["photo.png", "photo.webp"]);
    }

    /// Same, but the job goes on: the swap is refused and the original
    /// comes back next to the file that took its name.
    #[test]
    fn a_replaced_file_is_not_swapped_and_the_original_comes_back() {
        let dir = tempfile::tempdir().unwrap();
        let (result, current, before) = run_with_meanwhile(dir.path(), |current| {
            std::fs::remove_file(current).unwrap();
            std::fs::write(current, b"someone else's").unwrap();
            false
        });
        assert!(result.is_err());
        assert_eq!(std::fs::read(&current).unwrap(), b"someone else's");
        assert_eq!(std::fs::read(dir.path().join("photo.png")).unwrap(), before);
        assert_eq!(names(dir.path()), ["photo.png", "photo.webp"]);
    }

    /// Written to while converting: the result would be stale, so nothing is
    /// swapped or binned, and no second copy is made.
    #[test]
    fn a_file_changed_while_converting_is_left_as_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let (result, current, before) = run_with_meanwhile(dir.path(), |current| {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(current)
                .unwrap();
            f.write_all(b"more").unwrap();
            false
        });
        assert!(result.is_err());
        let mut edited = before.clone();
        edited.extend_from_slice(b"more");
        assert_eq!(std::fs::read(&current).unwrap(), edited);
        assert_eq!(names(dir.path()), ["photo.webp"]);
    }

    /// Same name, same size, same last write, other bytes: still another file.
    #[test]
    fn identity_sees_other_bytes_of_the_same_size() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.png");
        std::fs::write(&path, vec![1u8; 10_000]).unwrap();
        let id = file_id(&path);
        let mut other = vec![1u8; 10_000];
        other[9_999] = 2;
        let when = std::fs::metadata(&path).unwrap().modified().unwrap();
        std::fs::write(&path, other).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(when)
            .unwrap();
        assert!(!still_ours(&path, id));
    }

    #[test]
    fn file_identity_changes_when_another_file_takes_the_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.png");
        std::fs::write(&path, b"first").unwrap();
        let id = file_id(&path);
        assert!(id.is_some());
        assert!(still_ours(&path, id));
        let other = dir.path().join("b.png");
        std::fs::write(&other, b"second").unwrap();
        std::fs::remove_file(&path).unwrap();
        std::fs::rename(&other, &path).unwrap();
        assert!(!still_ours(&path, id));
    }

    #[test]
    fn failed_conversion_restores_the_original_name() {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("broken.png");
        let current = dir.path().join("broken.webp");
        std::fs::write(&current, b"\x89PNG\r\n\x1a\nnot really").unwrap();
        let job = Job {
            current: current.clone(),
            original: original.clone(),
            final_path: current.clone(),
            source: "png".into(),
            target: "webp".into(),
            keep_original: false,
            max_size: None,
            from_menu: false,
        };
        assert!(run(&job, &registry(), &QuickOptions::default()).is_err());
        assert!(original.exists());
        assert!(!current.exists());
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            1,
            "work folder removed"
        );
    }

    #[test]
    fn plus_rename_lands_on_the_clean_name() {
        let dir = tempfile::tempdir().unwrap();
        let current = dir.path().join("photo.jpg+");
        std::fs::write(&current, png_bytes()).unwrap();
        let job = Job {
            current: current.clone(),
            original: dir.path().join("photo.png"),
            final_path: dir.path().join("photo.jpg"),
            source: "png".into(),
            target: "jpg".into(),
            keep_original: false,
            max_size: None,
            from_menu: false,
        };
        run(&job, &registry(), &QuickOptions::with_quality(70)).unwrap();
        assert_eq!(
            crate::core::sniff::sniff_file(&dir.path().join("photo.jpg")),
            Some("jpg")
        );
        assert!(!current.exists());
    }
}

#[cfg(test)]
mod marks {
    use super::*;
    use crate::core::conversion::types::AssetFamily;
    use std::sync::Arc;

    fn registry() -> ConverterRegistry {
        let mut r = ConverterRegistry::new();
        r.register(
            AssetFamily::Image,
            Arc::new(crate::core::handlers::image::ImageConverterHandler),
        );
        r
    }

    fn png(dir: &Path, name: &str, side: u32) -> PathBuf {
        let path = dir.join(name);
        image::RgbaImage::from_pixel(side, side / 2, image::Rgba([10, 120, 200, 255]))
            .save_with_format(&path, image::ImageFormat::Png)
            .unwrap();
        path
    }

    /// `photo.png` → `photo.png@32`: same name in and out. The smaller copy
    /// takes the name; the original must not be overwritten or binned in
    /// its place (it once was: "done", and the photo was gone).
    #[test]
    fn same_format_resize_keeps_a_picture_under_the_name() {
        let dir = tempfile::tempdir().unwrap();
        let current = png(dir.path(), "photo.png@32", 64);
        let job = Job {
            current,
            original: dir.path().join("photo.png"),
            final_path: dir.path().join("photo.png"),
            source: "png".into(),
            target: "png".into(),
            keep_original: false,
            max_size: Some(32),
            from_menu: false,
        };
        run(&job, &registry(), &QuickOptions::default()).unwrap();
        let out = image::open(dir.path().join("photo.png")).unwrap();
        assert_eq!((out.width(), out.height()), (32, 16), "the smaller copy");
        // Tests delete instead of recycling; only the new file is left.
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["photo.png".to_string()]);
    }

    #[test]
    fn same_format_resize_with_keep_leaves_both() {
        let dir = tempfile::tempdir().unwrap();
        let current = png(dir.path(), "photo.png&@32", 64);
        let job = Job {
            current,
            original: dir.path().join("photo.png"),
            final_path: final_path_for(
                &dir.path().join("photo.png"),
                &dir.path().join("photo.png"),
                true,
            ),
            source: "png".into(),
            target: "png".into(),
            keep_original: true,
            max_size: Some(32),
            from_menu: false,
        };
        let done = run(&job, &registry(), &QuickOptions::default()).unwrap();
        let original = image::open(dir.path().join("photo.png")).unwrap();
        assert_eq!(original.width(), 64, "the original is untouched");
        assert_ne!(done.final_path, dir.path().join("photo.png"));
        assert_eq!(image::open(&done.final_path).unwrap().width(), 32);
    }

    #[test]
    fn keep_mark_leaves_the_original_next_to_the_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let current = png(dir.path(), "photo.webp&", 64);
        let job = Job {
            current,
            original: dir.path().join("photo.png"),
            final_path: dir.path().join("photo.webp"),
            source: "png".into(),
            target: "webp".into(),
            keep_original: true,
            max_size: None,
            from_menu: false,
        };
        let done = run(&job, &registry(), &QuickOptions::default()).unwrap();
        assert!(!done.original_recycled);
        assert_eq!(
            crate::core::sniff::sniff_file(&dir.path().join("photo.png")),
            Some("png")
        );
        assert_eq!(
            crate::core::sniff::sniff_file(&dir.path().join("photo.webp")),
            Some("webp")
        );
        assert!(!dir.path().join("photo.webp&").exists());
    }

    #[test]
    fn size_mark_shrinks_the_longest_side() {
        let dir = tempfile::tempdir().unwrap();
        let current = png(dir.path(), "photo.jpg@100", 400);
        let job = Job {
            current,
            original: dir.path().join("photo.png"),
            final_path: dir.path().join("photo.jpg"),
            source: "png".into(),
            target: "jpg".into(),
            keep_original: false,
            max_size: Some(100),
            from_menu: false,
        };
        run(&job, &registry(), &QuickOptions::default()).unwrap();
        let out = image::image_dimensions(dir.path().join("photo.jpg")).unwrap();
        assert_eq!(out, (100, 50));
    }

    #[test]
    fn a_file_converted_in_place_keeps_or_bins_its_own_name() {
        for keep in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let file = png(dir.path(), "a.png", 32);
            let job = Job {
                current: file.clone(),
                original: file.clone(),
                final_path: dir.path().join("a.webp"),
                source: "png".into(),
                target: "webp".into(),
                keep_original: keep,
                max_size: None,
                from_menu: false,
            };
            run(&job, &registry(), &QuickOptions::default()).unwrap();
            assert!(dir.path().join("a.webp").exists());
            assert_eq!(file.exists(), keep, "keep={keep}");
            assert!(!dir.path().join("a (2).png").exists());
        }
    }
}

#[cfg(test)]
mod local_media {
    use super::*;
    use crate::core::conversion::types::AssetFamily;
    use std::sync::Arc;

    /// Runs only with ZC_MP4=<path to an mp4 with audio>.
    #[test]
    fn mp4_with_video_becomes_mp3() {
        let Ok(src) = std::env::var("ZC_MP4") else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let current = dir.path().join("clip.mp3");
        std::fs::copy(src, &current).unwrap();
        let mut r = ConverterRegistry::new();
        r.register(
            AssetFamily::Image,
            Arc::new(crate::core::handlers::image::ImageConverterHandler),
        );
        r.register(
            AssetFamily::Audio,
            Arc::new(crate::core::handlers::audio::AudioHandler),
        );
        let job = Job {
            current: current.clone(),
            original: dir.path().join("clip.mp4"),
            final_path: current.clone(),
            source: "m4a".into(),
            target: "mp3".into(),
            keep_original: false,
            max_size: None,
            from_menu: false,
        };
        let res = run(&job, &r, &QuickOptions::default());
        assert!(res.is_ok(), "{res:?}");
    }
}
