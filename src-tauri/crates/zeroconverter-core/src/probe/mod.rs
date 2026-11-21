pub mod types;

use self::types::{AudioProbeData, FileProbeResult, ProbeStatus};
use crate::app_error::AppError;
use crate::conversion::registry::ConverterRegistry;
use crate::conversion::types::AssetFamily;
use std::path::Path;

/// Thin dispatcher for file probing. NOT a god-module.
///
/// Responsibilities:
/// - Derive provisional family from extension
/// - Delegate deep probing to the correct handler via registry
/// - Assemble FileProbeResult
///
/// Does NOT contain family-specific logic. That lives in handlers.
pub fn probe_file(path: &Path, registry: &ConverterRegistry) -> Result<FileProbeResult, AppError> {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();

    let size_bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);

    // Derive provisional family from extension via registry
    let family = registry.resolve_family(&extension);

    // Attempt deep probe through the handler
    let (probe_status, audio_meta) = match &family {
        AssetFamily::Image | AssetFamily::Audio | AssetFamily::Video => {
            match registry.probe(family.clone(), path) {
                Ok(_) => {
                    // If audio, extract cheap metadata
                    let audio = if family == AssetFamily::Audio {
                        extract_audio_meta(path)
                    } else {
                        None
                    };
                    (ProbeStatus::Recognized, audio)
                }
                Err(_) => (ProbeStatus::Failed, None),
            }
        }
        _ => (ProbeStatus::Failed, None),
    };

    Ok(FileProbeResult {
        path: path.to_string_lossy().to_string(),
        name,
        extension,
        size_bytes,
        family,
        probe_status,
        audio_meta,
    })
}

/// Cheap audio metadata extraction for probe results.
/// Separated from the probe dispatcher to keep it thin.
fn extract_audio_meta(path: &Path) -> Option<AudioProbeData> {
    use crate::handlers::audio::probe::probe_audio;
    match probe_audio(path) {
        Ok(meta) => Some(AudioProbeData {
            duration_seconds: meta.duration_seconds,
            sample_rate: meta.sample_rate,
            channels: meta.channels,
        }),
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::image::ImageConverterHandler;
    use std::io::Write;
    use std::sync::Arc;

    fn make_registry_with_image() -> ConverterRegistry {
        let mut registry = ConverterRegistry::new();
        registry.register(AssetFamily::Image, Arc::new(ImageConverterHandler));
        registry
    }

    fn temp_file_with_bytes(ext: &str, bytes: &[u8]) -> tempfile::NamedTempFile {
        let mut file = tempfile::Builder::new()
            .suffix(&format!(".{}", ext))
            .tempfile()
            .expect("Failed to create temp file");
        file.write_all(bytes).expect("Failed to write bytes");
        file
    }

    // ═══════════════════════════════════════════
    // Image Probing Through Dispatcher
    // ═══════════════════════════════════════════

    #[test]
    fn probe_file_identifies_png_as_image_verified() {
        let registry = make_registry_with_image();
        let png_bytes = [
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
        ];
        let file = temp_file_with_bytes("png", &png_bytes);
        let result = probe_file(file.path(), &registry);
        assert!(result.is_ok());
        let pr = result.unwrap();
        assert_eq!(pr.family, AssetFamily::Image);
        assert!(matches!(pr.probe_status, ProbeStatus::Recognized));
        assert_eq!(pr.extension, "png");
        assert!(pr.audio_meta.is_none());
    }

    #[test]
    fn probe_file_identifies_jpg_as_image_verified() {
        let registry = make_registry_with_image();
        let jpeg_bytes = [
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01,
        ];
        let file = temp_file_with_bytes("jpg", &jpeg_bytes);
        let result = probe_file(file.path(), &registry);
        assert!(result.is_ok());
        let pr = result.unwrap();
        assert_eq!(pr.family, AssetFamily::Image);
        assert!(matches!(pr.probe_status, ProbeStatus::Recognized));
    }

    #[test]
    fn probe_file_extracts_correct_name() {
        let registry = make_registry_with_image();
        let png_bytes = [
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
        ];
        let file = temp_file_with_bytes("png", &png_bytes);
        let result = probe_file(file.path(), &registry).unwrap();
        assert!(result.name.ends_with(".png"));
    }

    #[test]
    fn probe_file_extracts_nonzero_size() {
        let registry = make_registry_with_image();
        let png_bytes = [
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
        ];
        let file = temp_file_with_bytes("png", &png_bytes);
        let result = probe_file(file.path(), &registry).unwrap();
        assert!(result.size_bytes > 0);
    }

    // ═══════════════════════════════════════════
    // Unknown File Type
    // ═══════════════════════════════════════════

    #[test]
    fn probe_file_returns_unknown_for_unsupported_extension() {
        let registry = make_registry_with_image();
        let file = temp_file_with_bytes("mp4", b"fake video content here");
        let result = probe_file(file.path(), &registry);
        assert!(result.is_ok());
        let pr = result.unwrap();
        assert_eq!(pr.family, AssetFamily::Unknown);
        assert!(matches!(pr.probe_status, ProbeStatus::Failed));
    }

    // ═══════════════════════════════════════════
    // Probe Failure (Corrupt Image)
    // ═══════════════════════════════════════════

    #[test]
    fn probe_file_marks_corrupt_image_with_failed_status() {
        let registry = make_registry_with_image();
        // .png extension but only 2 bytes (too small for probe)
        let file = temp_file_with_bytes("png", &[0x00, 0x01]);
        let result = probe_file(file.path(), &registry);
        assert!(result.is_ok()); // probe_file doesn't error, it returns Failed status
        let pr = result.unwrap();
        assert_eq!(pr.family, AssetFamily::Image); // family still resolved by extension
        assert!(matches!(pr.probe_status, ProbeStatus::Failed));
    }

    // ═══════════════════════════════════════════
    // Empty Registry
    // ═══════════════════════════════════════════

    #[test]
    fn probe_file_with_empty_registry_returns_unknown() {
        let registry = ConverterRegistry::new();
        let png_bytes = [
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
        ];
        let file = temp_file_with_bytes("png", &png_bytes);
        let result = probe_file(file.path(), &registry);
        assert!(result.is_ok());
        let pr = result.unwrap();
        assert_eq!(pr.family, AssetFamily::Unknown);
        assert!(matches!(pr.probe_status, ProbeStatus::Failed));
    }

    // ═══════════════════════════════════════════
    // File With No Extension
    // ═══════════════════════════════════════════

    #[test]
    fn probe_file_with_no_extension() {
        let registry = make_registry_with_image();
        let mut f = tempfile::Builder::new()
            .suffix("") // no extension
            .tempfile()
            .unwrap();
        f.write_all(b"some content").unwrap();
        let result = probe_file(f.path(), &registry);
        assert!(result.is_ok());
        let pr = result.unwrap();
        assert_eq!(pr.family, AssetFamily::Unknown);
    }
}
