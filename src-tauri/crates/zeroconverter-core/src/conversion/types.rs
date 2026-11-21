use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AssetFamily {
    Image,
    Audio,
    Video,
    Document,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputFormat(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputFormat(pub String);

/// Declarative metadata for a file format within a handler family.
/// Serves as the single source of truth for canonical extensions, aliases, and read/write capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatDescriptor {
    pub id: &'static str,
    pub canonical_extension: &'static str,
    pub aliases: &'static [&'static str],
    pub can_read: bool,
    pub can_write: bool,
}

impl FormatDescriptor {
    /// Case-insensitive match against id, canonical_extension, or aliases.
    pub fn matches_format(&self, format: &str) -> bool {
        self.id.eq_ignore_ascii_case(format)
            || self.canonical_extension.eq_ignore_ascii_case(format)
            || self.aliases.iter().any(|a| a.eq_ignore_ascii_case(format))
    }
}

/// Find the first descriptor matching a format string within a slice.
pub fn find_descriptor<'a>(
    descriptors: &'a [FormatDescriptor],
    format: &str,
) -> Option<&'a FormatDescriptor> {
    descriptors.iter().find(|d| d.matches_format(format))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversionCapability {
    pub input: InputFormat,
    pub outputs: Vec<OutputFormat>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversionRequest {
    pub family: AssetFamily,
    pub input_path: String,
    pub output_path: String,
    pub output_root: String,
    pub output_format: OutputFormat,
    pub options: serde_json::Value,
}

impl ConversionRequest {
    /// "Overwrite existing files": the one place the flag is read.
    pub fn overwrite(&self) -> bool {
        self.options
            .get("overwrite")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    /// How the finished output may land on an existing file.
    pub fn clobber(&self) -> zero_fs::Clobber {
        clobber(self.overwrite())
    }

    /// The user agreed to convert an input over [`LARGE_INPUT_BYTES`].
    pub fn large_confirmed(&self) -> bool {
        self.options
            .get("confirm_large")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }
}

/// Inputs above this size convert only with the user's consent
/// (`confirm_large: true` in the options). A 4 GB video sent to GIF by
/// mistake should be a question, not an hour of work and a full disk.
pub const LARGE_INPUT_BYTES: u64 = 100 * 1024 * 1024;

/// The input's size when it is over [`LARGE_INPUT_BYTES`].
pub fn large_input(path: &std::path::Path) -> Option<u64> {
    let len = std::fs::metadata(path).ok()?.len();
    (len > LARGE_INPUT_BYTES).then_some(len)
}

pub fn clobber(overwrite: bool) -> zero_fs::Clobber {
    if overwrite {
        zero_fs::Clobber::Replace
    } else {
        zero_fs::Clobber::Never
    }
}

/// File size before and after, shown in the details panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileIntegrityDto {
    pub byte_size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversionResult {
    pub success: bool,
    pub output_path: Option<String>,
    pub message: String,
    pub input_integrity: Option<FileIntegrityDto>,
    pub output_integrity: Option<FileIntegrityDto>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_JPG: FormatDescriptor = FormatDescriptor {
        id: "jpg",
        canonical_extension: "jpg",
        aliases: &["jpeg"],
        can_read: true,
        can_write: true,
    };

    const TEST_PNG: FormatDescriptor = FormatDescriptor {
        id: "png",
        canonical_extension: "png",
        aliases: &[],
        can_read: true,
        can_write: true,
    };

    // ═══════════════════════════════════════════
    // FormatDescriptor::matches_format
    // ═══════════════════════════════════════════

    #[test]
    fn matches_format_by_id() {
        assert!(TEST_JPG.matches_format("jpg"));
    }

    #[test]
    fn matches_format_by_alias() {
        assert!(TEST_JPG.matches_format("jpeg"));
    }

    #[test]
    fn matches_format_case_insensitive_id() {
        assert!(TEST_JPG.matches_format("JPG"));
        assert!(TEST_JPG.matches_format("Jpg"));
    }

    #[test]
    fn matches_format_case_insensitive_alias() {
        assert!(TEST_JPG.matches_format("JPEG"));
        assert!(TEST_JPG.matches_format("Jpeg"));
    }

    #[test]
    fn does_not_match_unrelated_format() {
        assert!(!TEST_JPG.matches_format("png"));
        assert!(!TEST_JPG.matches_format("webp"));
    }

    #[test]
    fn does_not_match_empty_string() {
        assert!(!TEST_JPG.matches_format(""));
    }

    // ═══════════════════════════════════════════
    // find_descriptor
    // ═══════════════════════════════════════════

    static TEST_DESCRIPTORS: &[FormatDescriptor] = &[TEST_PNG, TEST_JPG];

    #[test]
    fn find_descriptor_by_id() {
        let desc = find_descriptor(TEST_DESCRIPTORS, "png").unwrap();
        assert_eq!(desc.id, "png");
    }

    #[test]
    fn find_descriptor_by_alias() {
        let desc = find_descriptor(TEST_DESCRIPTORS, "jpeg").unwrap();
        assert_eq!(desc.id, "jpg");
        assert_eq!(desc.canonical_extension, "jpg");
    }

    #[test]
    fn find_descriptor_case_insensitive() {
        let desc = find_descriptor(TEST_DESCRIPTORS, "PNG").unwrap();
        assert_eq!(desc.id, "png");
    }

    #[test]
    fn find_descriptor_returns_none_for_unknown() {
        assert!(find_descriptor(TEST_DESCRIPTORS, "bmp").is_none());
    }

    #[test]
    fn jpg_jpeg_alias_resolves_to_same_descriptor() {
        let by_jpg = find_descriptor(TEST_DESCRIPTORS, "jpg").unwrap();
        let by_jpeg = find_descriptor(TEST_DESCRIPTORS, "jpeg").unwrap();
        assert_eq!(by_jpg, by_jpeg);
    }

    // ═══════════════════════════════════════════
    // Drift Prevention: No New Formats Exposed
    // ═══════════════════════════════════════════

    #[test]
    fn image_supported_extensions_derived_from_descriptors() {
        use crate::conversion::traits::ConverterHandler;
        use crate::handlers::image::ImageConverterHandler;

        let handler = ImageConverterHandler;
        let exts = handler.supported_extensions();
        // Every readable descriptor id + aliases should be in extensions
        for desc in crate::handlers::image::formats::all_descriptors()
            .iter()
            .filter(|d| d.can_read)
        {
            assert!(
                exts.contains(&desc.canonical_extension),
                "Missing extension '{}' in image supported_extensions",
                desc.canonical_extension
            );
            for alias in desc.aliases {
                assert!(
                    exts.contains(alias),
                    "Missing alias '{}' in image supported_extensions",
                    alias
                );
            }
        }
    }

    #[test]
    fn audio_supported_extensions_derived_from_descriptors() {
        use crate::conversion::traits::ConverterHandler;
        use crate::handlers::audio::{all_descriptors, AudioHandler};

        let handler = AudioHandler;
        let exts = handler.supported_extensions();
        for desc in all_descriptors().iter().filter(|d| d.can_read) {
            assert!(
                exts.contains(&desc.canonical_extension),
                "Missing extension '{}' in audio supported_extensions",
                desc.canonical_extension
            );
        }
    }

    // ═══════════════════════════════════════════
    // Drift Prevention: Image capabilities derive from descriptors
    // ═══════════════════════════════════════════

    #[test]
    fn image_capabilities_outputs_derive_from_writable_descriptors() {
        use crate::conversion::traits::ConverterHandler;
        use crate::handlers::image::ImageConverterHandler;

        let handler = ImageConverterHandler;
        let caps = handler.capabilities();
        let writable_ids: Vec<&str> = handler
            .format_descriptors()
            .iter()
            .filter(|d| d.can_write)
            .map(|d| d.id)
            .collect();
        for cap in &caps {
            for output in &cap.outputs {
                assert!(
                    writable_ids.contains(&output.0.as_str()),
                    "Capability output '{}' is not a writable descriptor",
                    output.0
                );
            }
        }
    }

    #[test]
    fn image_capabilities_do_not_expose_jpeg_as_separate_entry() {
        use crate::conversion::traits::ConverterHandler;
        use crate::handlers::image::ImageConverterHandler;

        let handler = ImageConverterHandler;
        let caps = handler.capabilities();
        for cap in &caps {
            assert_ne!(
                cap.input.0, "jpeg",
                "jpeg must not appear as a separate capability input entry"
            );
            for output in &cap.outputs {
                assert_ne!(
                    output.0, "jpeg",
                    "jpeg must not appear as a separate capability output entry"
                );
            }
        }
    }
}
