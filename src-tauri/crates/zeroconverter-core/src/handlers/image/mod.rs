use crate::app_error::AppError;
use crate::conversion::traits::ConverterHandler;
use crate::conversion::types::{
    ConversionCapability, ConversionRequest, ConversionResult, FormatDescriptor, InputFormat,
    OutputFormat,
};
use std::path::Path;

pub mod execute;
pub mod formats;
pub mod options;
pub mod probe;
pub mod svg;
pub mod validate;

/// The two image formats that are not format modules, because they are
/// routes rather than codecs: SVG is rendered at a target size, and PDF is
/// written straight from the input file. Every other format is one file in
/// `formats/` (see `formats/mod.rs`).
pub static SPECIAL_FORMATS: &[FormatDescriptor] = &[
    FormatDescriptor {
        id: "svg",
        canonical_extension: "svg",
        aliases: &[],
        can_read: true,
        can_write: false,
    },
    FormatDescriptor {
        id: "pdf",
        canonical_extension: "pdf",
        aliases: &[],
        can_read: false,
        can_write: true,
    },
];

pub struct ImageConverterHandler;

impl ConverterHandler for ImageConverterHandler {
    fn format_descriptors(&self) -> &'static [FormatDescriptor] {
        formats::all_descriptors()
    }

    // supported_extensions() uses the default trait implementation derived from format_descriptors().

    fn probe(&self, path: &Path) -> Result<InputFormat, AppError> {
        probe::probe_image(path)
    }

    fn capabilities(&self) -> Vec<ConversionCapability> {
        let readable: Vec<_> = self
            .format_descriptors()
            .iter()
            .filter(|d| d.can_read)
            .collect();
        let writable: Vec<_> = self
            .format_descriptors()
            .iter()
            .filter(|d| d.can_write)
            .collect();

        // All readable raster formats (built-ins and registered modules) can
        // produce PDF pages; SVG remains deliberately out of scope.
        readable
            .iter()
            .map(|input| {
                let supports_pdf = input.id != "svg";
                ConversionCapability {
                    input: InputFormat(input.id.to_string()),
                    outputs: writable
                        .iter()
                        .filter(|o| o.id != "pdf" || supports_pdf)
                        .map(|o| OutputFormat(o.id.to_string()))
                        .collect(),
                }
            })
            .collect()
    }

    fn validate(&self, request: &ConversionRequest) -> Result<(), AppError> {
        let path = std::path::Path::new(&request.input_path);
        let input_format = probe::probe_image(path)?;
        validate::validate_image_conversion(request, &input_format)
    }

    fn execute(&self, request: &ConversionRequest) -> Result<ConversionResult, AppError> {
        execute::execute_image_conversion(request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversion::types::find_descriptor;

    #[test]
    fn ico_descriptor_supports_input_and_output() {
        let ico_desc =
            find_descriptor(formats::all_descriptors(), "ico").expect("Missing ICO descriptor");
        assert!(ico_desc.can_read, "ICO must be readable (input)");
        assert!(
            ico_desc.can_write,
            "ICO must be writable (basic single-frame output)"
        );
        assert_eq!(ico_desc.canonical_extension, "ico");
    }

    #[test]
    fn pdf_descriptor_is_write_only() {
        let pdf_desc = find_descriptor(SPECIAL_FORMATS, "pdf").expect("Missing PDF descriptor");
        assert!(
            !pdf_desc.can_read,
            "PDF must NOT be readable (no PDF input)"
        );
        assert!(pdf_desc.can_write, "PDF must be writable (output only)");
        assert_eq!(pdf_desc.canonical_extension, "pdf");
        assert!(pdf_desc.aliases.is_empty(), "PDF should have no aliases");
    }

    #[test]
    fn capabilities_expose_pdf_for_all_raster_inputs() {
        use crate::conversion::traits::ConverterHandler;
        let handler = ImageConverterHandler;
        let caps = handler.capabilities();

        // PDF output is available for all readable raster formats, including PSD and QOI.
        for input_id in &[
            "png", "jpg", "webp", "bmp", "tiff", "ico", "gif", "psd", "qoi",
        ] {
            let cap = caps
                .iter()
                .find(|c| c.input.0 == *input_id)
                .unwrap_or_else(|| panic!("Missing capability for {}", input_id));
            assert!(
                cap.outputs.iter().any(|o| o.0 == "pdf"),
                "{} route must include pdf output",
                input_id
            );
        }

        // PDF remains disabled for vector input until its rendering contract is specified.
        {
            let input_id = &"svg";
            let cap = caps
                .iter()
                .find(|c| c.input.0 == *input_id)
                .unwrap_or_else(|| panic!("Missing capability for {}", input_id));
            assert!(
                !cap.outputs.iter().any(|o| o.0 == "pdf"),
                "{} route must NOT include pdf output",
                input_id
            );
        }
    }

    #[test]
    fn pdf_does_not_appear_as_input_route() {
        use crate::conversion::traits::ConverterHandler;
        let handler = ImageConverterHandler;
        let caps = handler.capabilities();
        assert!(
            !caps.iter().any(|c| c.input.0 == "pdf"),
            "PDF must never appear as an input route (can_read: false)"
        );
    }
}
