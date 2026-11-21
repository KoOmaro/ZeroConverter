use super::traits::{ConverterHandler, ExecContext};
use super::types::{
    find_descriptor, AssetFamily, ConversionRequest, ConversionResult, FormatDescriptor,
    InputFormat,
};
use crate::app_error::AppError;
use std::path::Path;
use std::sync::Arc;

pub struct ConverterRegistry {
    /// In registration order, so lookups are deterministic.
    handlers: Vec<(AssetFamily, Arc<dyn ConverterHandler>)>,
}

impl Default for ConverterRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ConverterRegistry {
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
        }
    }

    /// Image, audio and video: the set the app and the CLI both use.
    pub fn standard() -> Self {
        use crate::handlers::{
            audio::AudioHandler, image::ImageConverterHandler, video::VideoHandler,
        };
        let mut r = Self::new();
        r.register(AssetFamily::Image, Arc::new(ImageConverterHandler));
        r.register(AssetFamily::Audio, Arc::new(AudioHandler));
        r.register(AssetFamily::Video, Arc::new(VideoHandler));
        r
    }

    pub fn register(&mut self, family: AssetFamily, handler: Arc<dyn ConverterHandler>) {
        self.handlers.retain(|(f, _)| *f != family);
        self.handlers.push((family, handler));
    }

    fn handler(&self, family: &AssetFamily) -> Option<&Arc<dyn ConverterHandler>> {
        self.handlers
            .iter()
            .find(|(f, _)| f == family)
            .map(|(_, h)| h)
    }

    /// Resolve which AssetFamily claims a given extension.
    /// Extension is a provisional hint — real truth comes from probe.
    pub fn resolve_family(&self, extension: &str) -> AssetFamily {
        let ext_lower = extension.to_lowercase();
        for (family, handler) in &self.handlers {
            if handler.supported_extensions().contains(&ext_lower.as_str()) {
                return family.clone();
            }
        }
        AssetFamily::Unknown
    }

    pub fn get_handler(&self, family: &AssetFamily) -> Option<Arc<dyn ConverterHandler>> {
        self.handler(family).cloned()
    }

    /// Delegate probing to the handler for the given family.
    pub fn probe(&self, family: AssetFamily, path: &Path) -> Result<InputFormat, AppError> {
        let handler = self
            .handler(&family)
            .ok_or_else(|| AppError::Registry(format!("No handler for family {:?}", family)))?;
        handler.probe(path)
    }

    pub fn validate(
        &self,
        family: AssetFamily,
        request: &ConversionRequest,
    ) -> Result<(), AppError> {
        let handler = self.handler(&family).ok_or_else(|| {
            AppError::Registry(format!("No handler found for family {:?}", family))
        })?;
        handler.validate(request)
    }

    pub fn execute(
        &self,
        family: AssetFamily,
        request: &ConversionRequest,
    ) -> Result<ConversionResult, AppError> {
        let handler = self.handler(&family).ok_or_else(|| {
            AppError::Registry(format!("No handler found for family {:?}", family))
        })?;
        handler.execute(request)
    }

    pub fn execute_with(
        &self,
        family: AssetFamily,
        request: &ConversionRequest,
        ctx: &ExecContext,
    ) -> Result<ConversionResult, AppError> {
        let handler = self.handler(&family).ok_or_else(|| {
            AppError::Registry(format!("No handler found for family {:?}", family))
        })?;
        handler.execute_with(request, ctx)
    }

    /// Family-scoped format descriptor lookup.
    /// Uses the shared find_descriptor() matching semantics (case-insensitive id/canonical/alias).
    pub fn get_format_descriptor(
        &self,
        family: &AssetFamily,
        format: &str,
    ) -> Result<FormatDescriptor, AppError> {
        let handler = self
            .handler(family)
            .ok_or_else(|| AppError::Registry(format!("No handler for family {:?}", family)))?;
        find_descriptor(handler.format_descriptors(), format)
            .copied()
            .ok_or_else(|| AppError::Validate(format!("Unsupported output format: {}", format)))
    }
}

#[cfg(test)]
mod tests {

    /// The app's three families, as `lib.rs` registers them.
    fn full_registry() -> ConverterRegistry {
        use crate::handlers::{
            audio::AudioHandler, image::ImageConverterHandler, video::VideoHandler,
        };
        let mut r = ConverterRegistry::new();
        r.register(AssetFamily::Image, Arc::new(ImageConverterHandler));
        r.register(AssetFamily::Audio, Arc::new(AudioHandler));
        r.register(AssetFamily::Video, Arc::new(VideoHandler));
        r
    }

    #[test]
    fn every_readable_extension_belongs_to_exactly_one_family() {
        let r = full_registry();
        let mut owner: std::collections::HashMap<&str, AssetFamily> = Default::default();
        for (family, handler) in &r.handlers {
            for d in handler.format_descriptors().iter().filter(|d| d.can_read) {
                for ext in std::iter::once(d.canonical_extension).chain(d.aliases.iter().copied()) {
                    if let Some(other) = owner.insert(ext, family.clone()) {
                        assert_eq!(&other, family, ".{ext} is read by {other:?} and {family:?}");
                    }
                }
            }
        }
        // Spellings people type are known without a hand-kept list.
        for spelling in ["jpeg", "tif", "aif"] {
            assert!(owner.contains_key(spelling), ".{spelling}");
        }
    }

    use super::*;
    use crate::conversion::traits::ConverterHandler;
    use crate::conversion::types::*;

    // ── Mock descriptor tables ──────────────────────────────────────────

    static MOCK_IMAGE_DESCRIPTORS: &[FormatDescriptor] = &[
        FormatDescriptor {
            id: "png",
            canonical_extension: "png",
            aliases: &[],
            can_read: true,
            can_write: true,
        },
        FormatDescriptor {
            id: "jpg",
            canonical_extension: "jpg",
            aliases: &["jpeg"],
            can_read: true,
            can_write: true,
        },
        FormatDescriptor {
            id: "webp",
            canonical_extension: "webp",
            aliases: &[],
            can_read: true,
            can_write: true,
        },
    ];

    static MOCK_AUDIO_DESCRIPTORS: &[FormatDescriptor] = &[
        FormatDescriptor {
            id: "wav",
            canonical_extension: "wav",
            aliases: &[],
            can_read: true,
            can_write: true,
        },
        FormatDescriptor {
            id: "mp3",
            canonical_extension: "mp3",
            aliases: &[],
            can_read: true,
            can_write: true,
        },
    ];

    static MOCK_FAIL_DESCRIPTORS: &[FormatDescriptor] = &[FormatDescriptor {
        id: "fail",
        canonical_extension: "fail",
        aliases: &[],
        can_read: true,
        can_write: false,
    }];

    // ── Mock handlers ───────────────────────────────────────────────────

    /// Minimal mock handler for testing registry dispatch.
    /// Uses descriptor-based supported_extensions() default.
    struct MockHandler {
        descriptors: &'static [FormatDescriptor],
    }

    impl MockHandler {
        fn new(descriptors: &'static [FormatDescriptor]) -> Self {
            Self { descriptors }
        }
    }

    impl ConverterHandler for MockHandler {
        fn format_descriptors(&self) -> &'static [FormatDescriptor] {
            self.descriptors
        }

        // supported_extensions() uses the default trait implementation.

        fn probe(&self, _path: &Path) -> Result<InputFormat, crate::app_error::AppError> {
            Ok(InputFormat("mock".to_string()))
        }

        fn capabilities(&self) -> Vec<ConversionCapability> {
            vec![]
        }

        fn validate(&self, _request: &ConversionRequest) -> Result<(), crate::app_error::AppError> {
            Ok(())
        }

        fn execute(
            &self,
            _request: &ConversionRequest,
        ) -> Result<ConversionResult, crate::app_error::AppError> {
            Ok(ConversionResult {
                success: true,
                output_path: None,
                message: "mock execution".to_string(),
                input_integrity: None,
                output_integrity: None,
            })
        }
    }

    /// Failing mock to test error propagation
    struct FailingHandler;

    impl ConverterHandler for FailingHandler {
        fn format_descriptors(&self) -> &'static [FormatDescriptor] {
            MOCK_FAIL_DESCRIPTORS
        }

        fn probe(&self, _path: &Path) -> Result<InputFormat, crate::app_error::AppError> {
            Err(crate::app_error::AppError::Probe(
                "mock probe failure".to_string(),
            ))
        }

        fn capabilities(&self) -> Vec<ConversionCapability> {
            vec![]
        }

        fn validate(&self, _request: &ConversionRequest) -> Result<(), crate::app_error::AppError> {
            Err(crate::app_error::AppError::Validate(
                "mock validation failure".to_string(),
            ))
        }

        fn execute(
            &self,
            _request: &ConversionRequest,
        ) -> Result<ConversionResult, crate::app_error::AppError> {
            Err(crate::app_error::AppError::Execute(
                "mock execution failure".to_string(),
            ))
        }
    }

    fn make_registry() -> ConverterRegistry {
        let mut registry = ConverterRegistry::new();
        registry.register(
            AssetFamily::Image,
            Arc::new(MockHandler::new(MOCK_IMAGE_DESCRIPTORS)),
        );
        registry.register(
            AssetFamily::Audio,
            Arc::new(MockHandler::new(MOCK_AUDIO_DESCRIPTORS)),
        );
        registry
    }

    // ═══════════════════════════════════════════
    // Family Resolution
    // ═══════════════════════════════════════════

    #[test]
    fn resolves_png_to_image_family() {
        let registry = make_registry();
        assert_eq!(registry.resolve_family("png"), AssetFamily::Image);
    }

    #[test]
    fn resolves_jpg_to_image_family() {
        let registry = make_registry();
        assert_eq!(registry.resolve_family("jpg"), AssetFamily::Image);
    }

    #[test]
    fn resolves_webp_to_image_family() {
        let registry = make_registry();
        assert_eq!(registry.resolve_family("webp"), AssetFamily::Image);
    }

    #[test]
    fn resolves_wav_to_audio_family() {
        let registry = make_registry();
        assert_eq!(registry.resolve_family("wav"), AssetFamily::Audio);
    }

    #[test]
    fn resolves_mp3_to_audio_family() {
        let registry = make_registry();
        assert_eq!(registry.resolve_family("mp3"), AssetFamily::Audio);
    }

    #[test]
    fn resolves_unknown_extension_to_unknown_family() {
        let registry = make_registry();
        assert_eq!(registry.resolve_family("mp4"), AssetFamily::Unknown);
    }

    #[test]
    fn resolves_empty_extension_to_unknown() {
        let registry = make_registry();
        assert_eq!(registry.resolve_family(""), AssetFamily::Unknown);
    }

    // ═══════════════════════════════════════════
    // Case Insensitivity
    // ═══════════════════════════════════════════

    #[test]
    fn resolves_uppercase_extension() {
        let registry = make_registry();
        assert_eq!(registry.resolve_family("PNG"), AssetFamily::Image);
    }

    #[test]
    fn resolves_mixed_case_extension() {
        let registry = make_registry();
        assert_eq!(registry.resolve_family("WaV"), AssetFamily::Audio);
    }

    // ═══════════════════════════════════════════
    // Handler Delegation (Probe/Validate/Execute)
    // ═══════════════════════════════════════════

    #[test]
    fn probe_delegates_to_correct_handler() {
        let registry = make_registry();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let result = registry.probe(AssetFamily::Image, tmp.path());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0, "mock");
    }

    #[test]
    fn validate_delegates_to_correct_handler() {
        let registry = make_registry();
        let req = ConversionRequest {
            family: AssetFamily::Image,
            input_path: "/test.png".to_string(),
            output_path: "/test.webp".to_string(),
            output_root: "/".to_string(),
            output_format: OutputFormat("webp".to_string()),
            options: serde_json::json!({}),
        };
        assert!(registry.validate(AssetFamily::Image, &req).is_ok());
    }

    #[test]
    fn execute_delegates_to_correct_handler() {
        let registry = make_registry();
        let req = ConversionRequest {
            family: AssetFamily::Image,
            input_path: "/test.png".to_string(),
            output_path: "/test.webp".to_string(),
            output_root: "/".to_string(),
            output_format: OutputFormat("webp".to_string()),
            options: serde_json::json!({}),
        };
        let result = registry.execute(AssetFamily::Image, &req);
        assert!(result.is_ok());
        assert!(result.unwrap().success);
    }

    // ═══════════════════════════════════════════
    // Error Propagation — Unregistered Family
    // ═══════════════════════════════════════════

    #[test]
    fn probe_errors_for_unregistered_family() {
        let registry = make_registry();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let result = registry.probe(AssetFamily::Video, tmp.path());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("No handler"),
            "Expected registry error, got: {}",
            err
        );
    }

    #[test]
    fn validate_errors_for_unregistered_family() {
        let registry = make_registry();
        let req = ConversionRequest {
            family: AssetFamily::Video,
            input_path: "/test.mp4".to_string(),
            output_path: "/test.avi".to_string(),
            output_root: "/".to_string(),
            output_format: OutputFormat("avi".to_string()),
            options: serde_json::json!({}),
        };
        assert!(registry.validate(AssetFamily::Video, &req).is_err());
    }

    #[test]
    fn execute_errors_for_unregistered_family() {
        let registry = make_registry();
        let req = ConversionRequest {
            family: AssetFamily::Document,
            input_path: "/test.pdf".to_string(),
            output_path: "/test.docx".to_string(),
            output_root: "/".to_string(),
            output_format: OutputFormat("docx".to_string()),
            options: serde_json::json!({}),
        };
        assert!(registry.execute(AssetFamily::Document, &req).is_err());
    }

    // ═══════════════════════════════════════════
    // Error Propagation — Handler Returns Error
    // ═══════════════════════════════════════════

    #[test]
    fn probe_propagates_handler_error() {
        let mut registry = ConverterRegistry::new();
        registry.register(AssetFamily::Video, Arc::new(FailingHandler));
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let result = registry.probe(AssetFamily::Video, tmp.path());
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("mock probe failure"));
    }

    #[test]
    fn validate_propagates_handler_error() {
        let mut registry = ConverterRegistry::new();
        registry.register(AssetFamily::Video, Arc::new(FailingHandler));
        let req = ConversionRequest {
            family: AssetFamily::Video,
            input_path: "/test.mp4".to_string(),
            output_path: "/test.avi".to_string(),
            output_root: "/".to_string(),
            output_format: OutputFormat("avi".to_string()),
            options: serde_json::json!({}),
        };
        let result = registry.validate(AssetFamily::Video, &req);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("mock validation failure"));
    }

    #[test]
    fn execute_propagates_handler_error() {
        let mut registry = ConverterRegistry::new();
        registry.register(AssetFamily::Video, Arc::new(FailingHandler));
        let req = ConversionRequest {
            family: AssetFamily::Video,
            input_path: "/test.mp4".to_string(),
            output_path: "/test.avi".to_string(),
            output_root: "/".to_string(),
            output_format: OutputFormat("avi".to_string()),
            options: serde_json::json!({}),
        };
        let result = registry.execute(AssetFamily::Video, &req);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("mock execution failure"));
    }

    // ═══════════════════════════════════════════
    // Empty Registry
    // ═══════════════════════════════════════════

    #[test]
    fn empty_registry_resolves_everything_to_unknown() {
        let registry = ConverterRegistry::new();
        assert_eq!(registry.resolve_family("png"), AssetFamily::Unknown);
        assert_eq!(registry.resolve_family("wav"), AssetFamily::Unknown);
        assert_eq!(registry.resolve_family(""), AssetFamily::Unknown);
    }

    // ═══════════════════════════════════════════
    // Registration Overwrite
    // ═══════════════════════════════════════════

    static MOCK_OVERRIDE_A: &[FormatDescriptor] = &[FormatDescriptor {
        id: "png",
        canonical_extension: "png",
        aliases: &[],
        can_read: true,
        can_write: true,
    }];

    static MOCK_OVERRIDE_B: &[FormatDescriptor] = &[
        FormatDescriptor {
            id: "bmp",
            canonical_extension: "bmp",
            aliases: &[],
            can_read: true,
            can_write: true,
        },
        FormatDescriptor {
            id: "tiff",
            canonical_extension: "tiff",
            aliases: &[],
            can_read: true,
            can_write: true,
        },
    ];

    #[test]
    fn re_registering_family_replaces_handler() {
        let mut registry = ConverterRegistry::new();
        registry.register(
            AssetFamily::Image,
            Arc::new(MockHandler::new(MOCK_OVERRIDE_A)),
        );
        // Overwrite with different extensions
        registry.register(
            AssetFamily::Image,
            Arc::new(MockHandler::new(MOCK_OVERRIDE_B)),
        );
        // Old extensions should no longer resolve
        assert_eq!(registry.resolve_family("png"), AssetFamily::Unknown);
        // New extensions should resolve
        assert_eq!(registry.resolve_family("bmp"), AssetFamily::Image);
    }

    // ═══════════════════════════════════════════
    // Format Descriptor Lookup
    // ═══════════════════════════════════════════

    #[test]
    fn get_format_descriptor_resolves_image_png() {
        let registry = make_registry();
        let desc = registry
            .get_format_descriptor(&AssetFamily::Image, "png")
            .unwrap();
        assert_eq!(desc.id, "png");
        assert_eq!(desc.canonical_extension, "png");
    }

    #[test]
    fn get_format_descriptor_resolves_jpeg_alias_to_jpg() {
        let registry = make_registry();
        let desc = registry
            .get_format_descriptor(&AssetFamily::Image, "jpeg")
            .unwrap();
        assert_eq!(desc.id, "jpg");
        assert_eq!(desc.canonical_extension, "jpg");
    }

    #[test]
    fn get_format_descriptor_case_insensitive() {
        let registry = make_registry();
        let desc = registry
            .get_format_descriptor(&AssetFamily::Image, "WEBP")
            .unwrap();
        assert_eq!(desc.id, "webp");
    }

    #[test]
    fn get_format_descriptor_unknown_format_rejects() {
        let registry = make_registry();
        let result = registry.get_format_descriptor(&AssetFamily::Image, "bmp");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Unsupported output format"),
            "Expected unsupported format error, got: {}",
            err
        );
    }

    #[test]
    fn get_format_descriptor_unregistered_family_rejects() {
        let registry = make_registry();
        let result = registry.get_format_descriptor(&AssetFamily::Video, "mp4");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("No handler"),
            "Expected registry error, got: {}",
            err
        );
    }

    #[test]
    fn get_format_descriptor_audio_wav() {
        let registry = make_registry();
        let desc = registry
            .get_format_descriptor(&AssetFamily::Audio, "wav")
            .unwrap();
        assert_eq!(desc.id, "wav");
        assert_eq!(desc.canonical_extension, "wav");
    }
}
