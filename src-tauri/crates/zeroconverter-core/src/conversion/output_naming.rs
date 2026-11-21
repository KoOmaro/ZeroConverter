use crate::app_error::AppError;
use crate::conversion::types::FormatDescriptor;
use crate::jobs::types::OutputReservationAttempt;
use std::path::{Path, PathBuf};
use zero_fs::ensure_within_root;

/// Plan-level invariant check. Guarantees that the generated path has the correct extension
/// for the requested format descriptor.
/// Uses FormatDescriptor::matches_format() for alias-aware matching.
pub fn ensure_output_extension_matches_format(
    path: &Path,
    descriptor: &FormatDescriptor,
) -> Result<(), AppError> {
    let ext = match path.extension().and_then(|e| e.to_str()) {
        Some(e) => e.to_lowercase(),
        None => {
            return Err(AppError::Validate(format!(
                "Generated output path has no extension. Expected format: {}",
                descriptor.id
            )));
        }
    };

    if !descriptor.matches_format(&ext) {
        return Err(AppError::Validate(format!(
            "Generated output path extension \".{}\" does not match format \"{}\".",
            ext, descriptor.id
        )));
    }
    Ok(())
}

/// Generates a safe, deduplicated output path.
/// Evaluates containment, canonical extensions, disk existence, and active reservations.
///
/// The caller must resolve the FormatDescriptor from the registry before calling this function.
/// This function does not perform its own format lookup — it relies on the pre-resolved descriptor.
pub fn generate_output_path<F>(
    input_path: &Path,
    output_root: &Path,
    descriptor: &FormatDescriptor,
    overwrite: bool,
    mut reserve_callback: F,
) -> Result<PathBuf, AppError>
where
    F: FnMut(&str) -> Result<OutputReservationAttempt, AppError>,
{
    let stem = input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| AppError::Validate("Input file has no name stem".into()))?;

    let ext = descriptor.canonical_extension;
    let base_name = format!("{}.{}", stem, ext);

    // The bare name: ensure_within_root joins it onto the resolved root, so a
    // relative `output_root` is not applied twice.
    let contained =
        ensure_within_root(output_root, Path::new(&base_name)).map_err(|e| match e {
            zero_fs::RootPathError::RootMissing(_) => {
                AppError::Validate("Output folder does not exist".to_string())
            }
            zero_fs::RootPathError::RootNotDirectory(_) => {
                AppError::Validate("Output folder path is not a directory".to_string())
            }
            zero_fs::RootPathError::AncestorNotDirectory(_) => {
                AppError::Validate("A parent path component is not a directory".to_string())
            }
            zero_fs::RootPathError::EscapesRoot { .. } => {
                AppError::Validate("Output path is outside the selected output folder".to_string())
            }
            zero_fs::RootPathError::Io(io_err) => AppError::Io(io_err.to_string()),
        })?;

    // Invariant check
    ensure_output_extension_matches_format(&contained, descriptor)?;

    // "Overwrite" replaces other files, never the source itself (PNG → PNG
    // into the same folder). That case gets a new name like any clash.
    if overwrite && !is_same_file(input_path, &contained) {
        if contained.exists() && contained.is_dir() {
            return Err(AppError::Validate(format!(
                "Output path is a directory: {}",
                contained.display()
            )));
        }

        // Fail only on active concurrent reservation or internal errors.
        // Another job's input is never replaced: it gets a new name below.
        match reserve_callback(&contained.to_string_lossy())? {
            OutputReservationAttempt::Reserved => return Ok(contained),
            OutputReservationAttempt::AlreadyReserved => {
                return Err(AppError::Validate(format!(
                    "Output path is already reserved by another active conversion job: {}",
                    contained.display()
                )))
            }
            OutputReservationAttempt::Protected => {}
        }
    }

    // overwrite=false: unified disk + reservation dedupe
    if !contained.exists()
        && !contained.is_dir()
        && matches!(
            reserve_callback(&contained.to_string_lossy())?,
            OutputReservationAttempt::Reserved
        )
    {
        return Ok(contained);
    }

    // Dedupe loop
    for i in 1..=zero_fs::MAX_AUTO_RENAME_ATTEMPTS {
        let deduped_name = format!("{} ({}).{}", stem, i, ext);

        // Let ensure_within_root catch if it somehow escapes, though unlikely here
        let deduped_contained =
            ensure_within_root(output_root, Path::new(&deduped_name)).map_err(|e| {
                AppError::Validate(format!("Containment check failed during rename: {}", e))
            })?;

        if deduped_contained.exists() || deduped_contained.is_dir() {
            continue;
        }

        if matches!(
            reserve_callback(&deduped_contained.to_string_lossy())?,
            OutputReservationAttempt::Reserved
        ) {
            return Ok(deduped_contained);
        }
    }

    Err(AppError::Validate(format!(
        "Could not find an available output filename after {} attempts: {}",
        zero_fs::MAX_AUTO_RENAME_ATTEMPTS,
        contained.display()
    )))
}

/// True when both paths name the same file on disk (case, `..` and
/// links resolved where the files exist).
fn is_same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a
            .to_string_lossy()
            .eq_ignore_ascii_case(&b.to_string_lossy()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversion::types::FormatDescriptor;

    // ── Test descriptor fixtures ─────────────────────────────────────────

    const PNG_DESC: FormatDescriptor = FormatDescriptor {
        id: "png",
        canonical_extension: "png",
        aliases: &[],
        can_read: true,
        can_write: true,
    };

    const JPG_DESC: FormatDescriptor = FormatDescriptor {
        id: "jpg",
        canonical_extension: "jpg",
        aliases: &["jpeg"],
        can_read: true,
        can_write: true,
    };

    const WEBP_DESC: FormatDescriptor = FormatDescriptor {
        id: "webp",
        canonical_extension: "webp",
        aliases: &[],
        can_read: true,
        can_write: true,
    };

    const TIFF_DESC: FormatDescriptor = FormatDescriptor {
        id: "tiff",
        canonical_extension: "tiff",
        aliases: &["tif"],
        can_read: true,
        can_write: true,
    };

    const MP3_DESC: FormatDescriptor = FormatDescriptor {
        id: "mp3",
        canonical_extension: "mp3",
        aliases: &[],
        can_read: true,
        can_write: true,
    };

    const WAV_DESC: FormatDescriptor = FormatDescriptor {
        id: "wav",
        canonical_extension: "wav",
        aliases: &[],
        can_read: true,
        can_write: true,
    };

    // ═══════════════════════════════════════════
    // Invariant: Extension Matching
    // ═══════════════════════════════════════════

    #[test]
    fn invariant_accepts_valid_extensions() {
        assert!(ensure_output_extension_matches_format(Path::new("f.png"), &PNG_DESC).is_ok());
        assert!(ensure_output_extension_matches_format(Path::new("f.jpg"), &JPG_DESC).is_ok());
        assert!(ensure_output_extension_matches_format(Path::new("f.jpeg"), &JPG_DESC).is_ok());
        assert!(ensure_output_extension_matches_format(Path::new("f.webp"), &WEBP_DESC).is_ok());
        assert!(ensure_output_extension_matches_format(Path::new("f.mp3"), &MP3_DESC).is_ok());
        assert!(ensure_output_extension_matches_format(Path::new("f.wav"), &WAV_DESC).is_ok());
    }

    #[test]
    fn invariant_rejects_missing_extension() {
        let err = ensure_output_extension_matches_format(Path::new("f"), &JPG_DESC).unwrap_err();
        assert!(err.to_string().contains("has no extension"));
    }

    #[test]
    fn invariant_rejects_mismatch() {
        let err =
            ensure_output_extension_matches_format(Path::new("f.png"), &JPG_DESC).unwrap_err();
        assert!(err.to_string().contains("does not match format"));
    }

    #[test]
    fn ensure_output_extension_matches_format_accepts_tif_alias() {
        let result = ensure_output_extension_matches_format(Path::new("image.tif"), &TIFF_DESC);
        assert!(
            result.is_ok(),
            "Should accept .tif alias for TIFF descriptor"
        );
    }

    // ═══════════════════════════════════════════
    // Generate Output Path
    // ═══════════════════════════════════════════

    #[test]
    fn generate_output_path_base_success() {
        let root = tempfile::tempdir().unwrap();
        let input = Path::new("file.png");

        let result = generate_output_path(input, root.path(), &WEBP_DESC, false, |_| {
            Ok(OutputReservationAttempt::Reserved)
        })
        .unwrap();

        assert_eq!(result.file_name().unwrap(), "file.webp");
    }

    #[test]
    fn generate_output_path_jpeg_request_generates_jpg() {
        // Even though the descriptor was resolved from "jpeg", its canonical_extension is "jpg"
        let root = tempfile::tempdir().unwrap();
        let input = Path::new("photo.jpeg");

        let result = generate_output_path(input, root.path(), &JPG_DESC, false, |_| {
            Ok(OutputReservationAttempt::Reserved)
        })
        .unwrap();

        assert_eq!(result.file_name().unwrap(), "photo.jpg");
    }

    #[test]
    fn generate_output_path_tif_request_generates_tiff() {
        let root = tempfile::tempdir().unwrap();
        let input = Path::new("photo.tif");

        let result = generate_output_path(input, root.path(), &TIFF_DESC, false, |_| {
            Ok(OutputReservationAttempt::Reserved)
        })
        .unwrap();

        assert_eq!(result.file_name().unwrap(), "photo.tiff");
    }

    #[test]
    fn generate_output_path_dedupes_on_conflict() {
        let root = tempfile::tempdir().unwrap();
        let input = Path::new("file.png");

        let mut attempts = 0;
        let result = generate_output_path(input, root.path(), &WEBP_DESC, false, |_| {
            attempts += 1;
            if attempts == 1 {
                Ok(OutputReservationAttempt::AlreadyReserved)
            } else {
                Ok(OutputReservationAttempt::Reserved)
            }
        })
        .unwrap();

        assert_eq!(result.file_name().unwrap(), "file (1).webp");
        assert_eq!(attempts, 2);
    }

    #[test]
    fn generate_output_path_aborts_on_internal_error() {
        let root = tempfile::tempdir().unwrap();
        let input = Path::new("file.png");

        let result = generate_output_path(input, root.path(), &WEBP_DESC, false, |_| {
            Err(AppError::Execute("db down".into()))
        });

        match result {
            Err(AppError::Execute(msg)) => assert_eq!(msg, "db down"),
            _ => panic!("Expected Execute error"),
        }
    }

    #[test]
    fn generate_output_path_fails_overwrite_on_conflict() {
        let root = tempfile::tempdir().unwrap();
        let input = Path::new("file.png");

        let result = generate_output_path(
            input,
            root.path(),
            &WEBP_DESC,
            true, // overwrite
            |_| Ok(OutputReservationAttempt::AlreadyReserved),
        );

        match result {
            Err(AppError::Validate(msg)) => assert!(msg.contains("already reserved")),
            _ => panic!("Expected Validate error"),
        }
    }

    #[test]
    fn overwrite_never_targets_the_source_file() {
        let root = tempfile::tempdir().unwrap();
        let input = root.path().join("photo.png");
        std::fs::write(&input, b"source").unwrap();

        let out = generate_output_path(&input, root.path(), &PNG_DESC, true, |_| {
            Ok(OutputReservationAttempt::Reserved)
        })
        .unwrap();

        assert_ne!(
            std::fs::canonicalize(&out).ok(),
            std::fs::canonicalize(&input).ok()
        );
        assert_ne!(out.file_name(), input.file_name());
        assert_eq!(std::fs::read(&input).unwrap(), b"source");
    }

    #[test]
    fn overwrite_still_replaces_other_files() {
        let root = tempfile::tempdir().unwrap();
        let input = root.path().join("photo.png");
        std::fs::write(&input, b"source").unwrap();
        std::fs::write(root.path().join("photo.webp"), b"old").unwrap();

        let out = generate_output_path(&input, root.path(), &WEBP_DESC, true, |_| {
            Ok(OutputReservationAttempt::Reserved)
        })
        .unwrap();

        assert_eq!(out.file_name().unwrap(), "photo.webp");
    }

    #[test]
    fn overwrite_never_replaces_another_jobs_input() {
        let root = tempfile::tempdir().unwrap();
        let input = root.path().join("photo.png");
        std::fs::write(&input, b"source").unwrap();
        let other_input = root.path().join("photo.webp");
        std::fs::write(&other_input, b"other").unwrap();

        let out = generate_output_path(&input, root.path(), &WEBP_DESC, true, |p| {
            Ok(if p.ends_with("photo.webp") {
                OutputReservationAttempt::Protected
            } else {
                OutputReservationAttempt::Reserved
            })
        })
        .unwrap();

        assert_eq!(out.file_name().unwrap(), "photo (1).webp");
    }
}
