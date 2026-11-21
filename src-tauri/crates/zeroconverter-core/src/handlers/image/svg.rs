// ==============================================================================
// OWNERSHIP:
//   SVG rasterization module for the Image handler family.
//   Responsible for SVG-specific parsing, validation, dimension extraction,
//   security policy enforcement, and rasterization to RGBA raster buffer.
//
// DO NOT ADD:
//   - SVG output/optimization logic
//   - Raster-to-SVG tracing
//   - System font loading
//   - Network or local file resource resolving
//   - .svgz / gzip decompression
//
// NOTES:
//   This module is dispatched by execute.rs when input_format == "svg".
//   It returns a DynamicImage::ImageRgba8 that feeds into the existing
//   raster encoding pipeline.
// ==============================================================================

use crate::app_error::AppError;
use crate::image_limits::{self, ImageDimensions};
use std::io::Read;
use std::path::Path;

/// Real-world SVGs are kilobytes to a few megabytes. Anything past this is
/// refused before it is read, so a huge file cannot fill memory while parsing.
pub(crate) const MAX_SVG_BYTES: u64 = 32 * 1024 * 1024;

/// Reads at most `MAX_SVG_BYTES`; a longer file is refused, even one that
/// grows while it is being read.
fn read_svg(path: &Path) -> Result<Vec<u8>, AppError> {
    let file = std::fs::File::open(path).map_err(|e| AppError::Io(e.to_string()))?;
    let mut data = Vec::new();
    file.take(MAX_SVG_BYTES + 1)
        .read_to_end(&mut data)
        .map_err(|e| AppError::Io(e.to_string()))?;
    if data.len() as u64 > MAX_SVG_BYTES {
        return Err(AppError::Validate(format!(
            "SVG file is larger than {} MB.",
            MAX_SVG_BYTES / (1024 * 1024)
        )));
    }
    Ok(data)
}

/// Text-like SVG element local names that must be rejected in strict v1.
const REJECTED_TEXT_ELEMENTS: &[&str] = &["text", "tspan", "textPath"];

/// Embedded raster image element local name rejected in strict v1.
const REJECTED_IMAGE_ELEMENT: &str = "image";

// ═══════════════════════════════════════════
// Probe Helper
// ═══════════════════════════════════════════

/// Cheap heuristic check: does this byte buffer look like SVG content?
///
/// This is classification only, not a correctness boundary.
/// Validation must parse with the SVG parser to confirm.
pub fn probe_is_svg(buffer: &[u8]) -> bool {
    // Read up to 4KB for preamble scanning
    let scan_len = buffer.len().min(4096);
    let text = match std::str::from_utf8(&buffer[..scan_len]) {
        Ok(s) => s,
        Err(_) => return false, // Binary content cannot be SVG
    };

    // Strip leading whitespace, XML declaration, and comments
    let trimmed = skip_xml_preamble(text);
    trimmed.starts_with("<svg") || trimmed.starts_with("<SVG")
}

/// Skip XML preamble elements (whitespace, <?xml ...?>, <!-- ... -->) to find
/// the first real tag.
fn skip_xml_preamble(text: &str) -> &str {
    let mut remaining = text.trim_start();

    loop {
        if remaining.starts_with("<?") {
            // Skip XML declaration: find closing ?>
            if let Some(end) = remaining.find("?>") {
                remaining = remaining[end + 2..].trim_start();
                continue;
            }
            break;
        }
        if remaining.starts_with("<!--") {
            // Skip XML comment: find closing -->
            if let Some(end) = remaining.find("-->") {
                remaining = remaining[end + 3..].trim_start();
                continue;
            }
            break;
        }
        if remaining.starts_with("<!DOCTYPE") || remaining.starts_with("<!doctype") {
            // Skip DOCTYPE: find closing >
            if let Some(end) = remaining.find('>') {
                remaining = remaining[end + 1..].trim_start();
                continue;
            }
            break;
        }
        break;
    }

    remaining
}

// ═══════════════════════════════════════════
// SVG Content Validation
// ═══════════════════════════════════════════

/// Validate SVG content for strict v1 policy compliance.
///
/// Rejects:
/// - Text-like elements (`<text>`, `<tspan>`, `<textPath>`)
/// - Embedded raster images (`<image>`)
/// - Malformed/non-SVG XML
///
/// Returns the raw SVG bytes on success for downstream parsing.
fn validate_svg_content(data: &[u8]) -> Result<(), AppError> {
    let text = std::str::from_utf8(data)
        .map_err(|_| AppError::Validate("SVG file contains invalid UTF-8".to_string()))?;

    let doc = roxmltree::Document::parse(text)
        .map_err(|e| AppError::Validate(format!("Malformed SVG: {}", e)))?;

    // Verify root element is <svg>
    let root = doc.root_element();
    if root.tag_name().name() != "svg" {
        return Err(AppError::Validate(
            "Not an SVG file: root element is not <svg>".to_string(),
        ));
    }

    let has_viewbox = root.has_attribute("viewBox");
    let has_width = root.has_attribute("width");
    let has_height = root.has_attribute("height");

    if !(has_viewbox || (has_width && has_height)) {
        return Err(AppError::Validate(
            "SVG is missing intrinsic dimensions (width/height or viewBox).".to_string(),
        ));
    }

    // Reject text-like elements
    for node in doc.descendants() {
        if node.is_element() {
            let local_name = node.tag_name().name();

            if REJECTED_TEXT_ELEMENTS.contains(&local_name) {
                return Err(AppError::Validate(
                    "SVG text rendering is not supported yet. Convert text to paths and try again."
                        .to_string(),
                ));
            }

            if local_name == REJECTED_IMAGE_ELEMENT {
                return Err(AppError::Validate(
                    "Embedded raster images inside SVG are not supported in this version."
                        .to_string(),
                ));
            }
        }
    }

    Ok(())
}

// ═══════════════════════════════════════════
// Dimension Extraction
// ═══════════════════════════════════════════

/// Parse SVG and extract intrinsic dimensions.
///
/// Uses `usvg::Tree` with locked-down options.
/// Rejects SVGs with missing/invalid/zero dimensions.
/// Applies CEIL rounding for fractional viewBox sizes.
/// Enforces the pixel budget from `image_limits`.
pub fn validate_svg_dimensions(path: &Path) -> Result<ImageDimensions, AppError> {
    let data = read_svg(path)?;

    // Content validation (text/image rejection, XML correctness)
    validate_svg_content(&data)?;

    // Parse with locked-down usvg options
    let opt = build_usvg_options();
    let tree = usvg::Tree::from_data(&data, &opt)
        .map_err(|e| AppError::Validate(format!("Failed to parse SVG: {}", e)))?;

    let size = tree.size();
    let width = size.width().ceil() as u32;
    let height = size.height().ceil() as u32;

    if width == 0 || height == 0 {
        return Err(AppError::Validate(
            "SVG has zero or invalid intrinsic dimensions.".to_string(),
        ));
    }

    let dims = image_limits::dimensions_from_pair(width, height);
    image_limits::check_pixel_budget(&dims)?;

    Ok(dims)
}

// ═══════════════════════════════════════════
// Rasterization
// ═══════════════════════════════════════════

/// Rasterize an SVG file into a `DynamicImage::ImageRgba8`.
///
/// Applies vector-level scaling when resize options specify target dimensions.
/// Returns an RGBA raster buffer suitable for the existing image encode pipeline.
pub fn rasterize_svg(
    path: &Path,
    target_size: Option<(u32, u32)>,
) -> Result<image::DynamicImage, AppError> {
    let data = read_svg(path)?;

    // Content validation
    validate_svg_content(&data)?;

    let opt = build_usvg_options();
    let tree = usvg::Tree::from_data(&data, &opt)
        .map_err(|e| AppError::Execute(format!("Failed to parse SVG for rasterization: {}", e)))?;

    let svg_size = tree.size();
    let svg_w = svg_size.width();
    let svg_h = svg_size.height();

    // Determine final raster dimensions
    let (raster_w, raster_h) = match target_size {
        Some((tw, th)) => (tw, th),
        None => (svg_w.ceil() as u32, svg_h.ceil() as u32),
    };

    if raster_w == 0 || raster_h == 0 {
        return Err(AppError::Execute(
            "SVG rasterization target dimensions are zero.".to_string(),
        ));
    }

    // A resize target can be larger than the SVG's own size.
    image_limits::check_pixel_budget(&image_limits::dimensions_from_pair(raster_w, raster_h))?;

    // Create pixmap for rendering
    let mut pixmap = tiny_skia::Pixmap::new(raster_w, raster_h).ok_or_else(|| {
        AppError::Execute(format!(
            "Failed to allocate pixmap for {}x{} rasterization",
            raster_w, raster_h
        ))
    })?;

    // Compute scale transform from SVG viewport to target raster size
    let scale_x = raster_w as f32 / svg_w;
    let scale_y = raster_h as f32 / svg_h;
    let transform = tiny_skia::Transform::from_scale(scale_x, scale_y);

    resvg::render(&tree, transform, &mut pixmap.as_mut());

    // tiny_skia stores premultiplied alpha; the image crate expects straight
    // alpha. Undo it in place on the pixmap's own buffer (no second copy).
    let mut rgba = pixmap.take();
    for px in rgba.chunks_exact_mut(4) {
        let a = px[3];
        if a == 0 {
            px.copy_from_slice(&[0, 0, 0, 0]);
        } else if a < 255 {
            let a = a as f32 / 255.0;
            for c in &mut px[..3] {
                *c = (*c as f32 / a).min(255.0) as u8;
            }
        }
    }
    let img = image::RgbaImage::from_raw(raster_w, raster_h, rgba).ok_or_else(|| {
        AppError::Execute("Failed to convert SVG rasterization buffer to RgbaImage".to_string())
    })?;

    // For formats without alpha support (JPG/BMP), the caller will composite
    // over white.
    Ok(image::DynamicImage::ImageRgba8(img))
}

// ═══════════════════════════════════════════
// Security Configuration
// ═══════════════════════════════════════════

/// Build locked-down usvg options.
///
/// Security policy:
/// - No network access (http/https blocked)
/// - No local file access (file:// blocked)
/// - No relative resource loading
/// - No data URI image loading (rejected at validation)
/// - No system fonts (text feature disabled at compile time)
/// - No scripts (inherently ignored by resvg/roxmltree)
fn build_usvg_options() -> usvg::Options<'static> {
    // Block all external and data image href resolution.
    // Since we reject <image> elements at validation, this is defense-in-depth.
    usvg::Options {
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_string: Box::new(|_href, _opt| None),
            resolve_data: Box::new(|_mime, _data, _opt| None),
        },
        ..Default::default()
    }
}

// ═══════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_svg(content: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::Builder::new().suffix(".svg").tempfile().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f
    }

    // ═══════════════════════════════════════════
    // Probe Tests
    // ═══════════════════════════════════════════

    #[test]
    fn probe_detects_direct_svg_root() {
        let buf = b"<svg xmlns=\"http://www.w3.org/2000/svg\"><rect/></svg>";
        assert!(probe_is_svg(buf));
    }

    #[test]
    fn probe_detects_svg_with_xml_declaration() {
        let buf = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\"><rect/></svg>";
        assert!(probe_is_svg(buf));
    }

    #[test]
    fn probe_detects_svg_with_comments_and_whitespace() {
        let buf = b"  \n  <?xml version=\"1.0\"?>  \n  <!-- a comment -->  \n  <svg><rect/></svg>";
        assert!(probe_is_svg(buf));
    }

    #[test]
    fn probe_detects_svg_with_doctype() {
        let buf = b"<?xml version=\"1.0\"?><!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \"http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd\"><svg><rect/></svg>";
        assert!(probe_is_svg(buf));
    }

    #[test]
    fn probe_rejects_non_svg_xml() {
        let buf = b"<?xml version=\"1.0\"?><html><body></body></html>";
        assert!(!probe_is_svg(buf));
    }

    #[test]
    fn probe_rejects_binary_content() {
        let buf = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]; // PNG header
        assert!(!probe_is_svg(&buf));
    }

    #[test]
    fn probe_rejects_empty_buffer() {
        assert!(!probe_is_svg(b""));
    }

    #[test]
    fn probe_rejects_svgz_magic_bytes() {
        // gzip magic: 1F 8B
        let buf = [0x1F, 0x8B, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00];
        assert!(!probe_is_svg(&buf));
    }

    // ═══════════════════════════════════════════
    // Content Validation Tests
    // ═══════════════════════════════════════════

    #[test]
    fn validate_rejects_text_element() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><text x="10" y="20">Hello</text></svg>"#;
        let result = validate_svg_content(svg.as_bytes());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("text rendering is not supported"));
    }

    #[test]
    fn validate_rejects_tspan_element() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><text><tspan>Hi</tspan></text></svg>"#;
        let result = validate_svg_content(svg.as_bytes());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("text rendering is not supported"));
    }

    #[test]
    fn validate_rejects_textpath_element() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><defs><path id="p" d="M10,80 C40,10 65,10 95,80"/></defs><text><textPath href="#p">Path</textPath></text></svg>"##;
        let result = validate_svg_content(svg.as_bytes());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("text rendering is not supported"));
    }

    #[test]
    fn validate_rejects_image_element() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><image href="data:image/png;base64,iVBOR" width="100" height="100"/></svg>"#;
        let result = validate_svg_content(svg.as_bytes());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Embedded raster images"));
    }

    #[test]
    fn validate_rejects_image_with_xlink_href() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="100" height="100"><image xlink:href="http://example.com/img.png" width="100" height="100"/></svg>"#;
        let result = validate_svg_content(svg.as_bytes());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Embedded raster images"));
    }

    #[test]
    fn validate_rejects_non_svg_xml() {
        let xml = r#"<?xml version="1.0"?><html><body>Not SVG</body></html>"#;
        let result = validate_svg_content(xml.as_bytes());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Not an SVG file"));
    }

    #[test]
    fn validate_rejects_malformed_xml() {
        let broken = r#"<svg><rect></svg"#;
        let result = validate_svg_content(broken.as_bytes());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Malformed SVG"));
    }

    #[test]
    fn validate_accepts_clean_svg() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><rect x="10" y="10" width="80" height="80" fill="red"/></svg>"#;
        assert!(validate_svg_content(svg.as_bytes()).is_ok());
    }

    // ═══════════════════════════════════════════
    // Dimension / Validation Tests
    // ═══════════════════════════════════════════

    #[test]
    fn dimensions_with_explicit_width_height() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="150"><rect fill="red" width="200" height="150"/></svg>"#;
        let f = temp_svg(svg);
        let dims = validate_svg_dimensions(f.path()).unwrap();
        assert_eq!(dims.width, 200);
        assert_eq!(dims.height, 150);
    }

    #[test]
    fn dimensions_with_viewbox_only() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 50"><rect fill="blue" width="100" height="50"/></svg>"#;
        let f = temp_svg(svg);
        let dims = validate_svg_dimensions(f.path()).unwrap();
        assert_eq!(dims.width, 100);
        assert_eq!(dims.height, 50);
    }

    #[test]
    fn dimensions_with_fractional_viewbox_uses_ceil() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 99.5 49.3"><rect fill="green" width="100" height="50"/></svg>"#;
        let f = temp_svg(svg);
        let dims = validate_svg_dimensions(f.path()).unwrap();
        assert_eq!(dims.width, 100); // ceil(99.5)
        assert_eq!(dims.height, 50); // ceil(49.3)
    }

    #[test]
    fn dimensions_rejects_missing_size_and_viewbox() {
        // SVG with no width, height, or viewBox.
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"><rect fill="red" width="50" height="50"/></svg>"#;
        let f = temp_svg(svg);
        let result = validate_svg_dimensions(f.path());
        assert!(
            result.is_err(),
            "Expected an error for SVG missing intrinsic dimensions"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("missing intrinsic dimensions"),
            "Expected error message to mention missing intrinsic dimensions"
        );
    }

    #[test]
    fn dimensions_rejects_svg_with_text() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><text>Hello</text></svg>"#;
        let f = temp_svg(svg);
        let result = validate_svg_dimensions(f.path());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("text rendering is not supported"));
    }

    #[test]
    fn dimensions_rejects_svg_with_image() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><image href="file:///etc/passwd" width="100" height="100"/></svg>"#;
        let f = temp_svg(svg);
        let result = validate_svg_dimensions(f.path());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Embedded raster images"));
    }

    // ═══════════════════════════════════════════
    // Rasterization Tests
    // ═══════════════════════════════════════════

    #[test]
    fn rasterize_simple_svg_to_rgba() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><rect x="0" y="0" width="64" height="64" fill="red"/></svg>"#;
        let f = temp_svg(svg);
        let img = rasterize_svg(f.path(), None).unwrap();
        assert_eq!(img.width(), 64);
        assert_eq!(img.height(), 64);
    }

    #[test]
    fn rasterize_svg_with_target_size() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100"><rect fill="blue" width="200" height="100"/></svg>"#;
        let f = temp_svg(svg);
        let img = rasterize_svg(f.path(), Some((100, 50))).unwrap();
        assert_eq!(img.width(), 100);
        assert_eq!(img.height(), 50);
    }

    #[test]
    fn oversized_svg_file_is_refused_before_parsing() {
        let f = temp_svg(r#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"></svg>"#);
        f.as_file().set_len(MAX_SVG_BYTES + 1).unwrap();
        let err = validate_svg_dimensions(f.path()).unwrap_err().to_string();
        assert!(err.contains("larger than"), "{err}");
        assert!(rasterize_svg(f.path(), None).is_err());
    }

    #[test]
    fn resize_target_past_the_pixel_budget_is_refused() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"/>"#;
        let f = temp_svg(svg);
        let err = rasterize_svg(f.path(), Some((16_384, 16_384)))
            .unwrap_err()
            .to_string();
        assert!(err.contains("exceed safe limit"), "{err}");
    }

    #[test]
    fn rasterize_keeps_straight_alpha_colour() {
        // Half-transparent pure red stays pure red after un-premultiplying.
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="2"><rect width="2" height="2" fill="#ff0000" fill-opacity="0.5"/></svg>"##;
        let f = temp_svg(svg);
        let img = rasterize_svg(f.path(), None).unwrap();
        let px = *img.as_rgba8().unwrap().get_pixel(0, 0);
        assert!(px[0] >= 250 && px[1] == 0 && px[2] == 0, "{px:?}");
    }

    #[test]
    fn rasterize_svg_preserves_alpha() {
        // SVG with transparent background and a semi-transparent shape
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"><rect x="0" y="0" width="4" height="4" fill="red" fill-opacity="0.5"/></svg>"#;
        let f = temp_svg(svg);
        let img = rasterize_svg(f.path(), None).unwrap();
        let rgba = img.as_rgba8().unwrap();
        let px = rgba.get_pixel(0, 0);
        // Alpha should not be 255 (fully opaque) since fill-opacity is 0.5
        assert!(
            px[3] < 200,
            "Expected semi-transparent pixel, got alpha={}",
            px[3]
        );
    }

    #[test]
    fn rasterize_rejects_svg_with_text() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><text>No</text></svg>"#;
        let f = temp_svg(svg);
        let result = rasterize_svg(f.path(), None);
        assert!(result.is_err());
    }

    #[test]
    fn rasterize_rejects_svg_with_image() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><image href="http://evil.com/img.png" width="64" height="64"/></svg>"#;
        let f = temp_svg(svg);
        let result = rasterize_svg(f.path(), None);
        assert!(result.is_err());
    }

    // ═══════════════════════════════════════════
    // Security Tests
    // ═══════════════════════════════════════════

    #[test]
    fn security_rejects_http_image_ref() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><image href="http://example.com/img.png" width="100" height="100"/></svg>"#;
        let result = validate_svg_content(svg.as_bytes());
        assert!(result.is_err());
    }

    #[test]
    fn security_rejects_https_image_ref() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><image href="https://example.com/img.png" width="100" height="100"/></svg>"#;
        let result = validate_svg_content(svg.as_bytes());
        assert!(result.is_err());
    }

    #[test]
    fn security_rejects_file_image_ref() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><image href="file:///etc/passwd" width="100" height="100"/></svg>"#;
        let result = validate_svg_content(svg.as_bytes());
        assert!(result.is_err());
    }

    #[test]
    fn security_rejects_relative_image_ref() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><image href="../secret.png" width="100" height="100"/></svg>"#;
        let result = validate_svg_content(svg.as_bytes());
        assert!(result.is_err());
    }

    #[test]
    fn security_rejects_data_uri_image() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><image href="data:image/png;base64,iVBORw0KGgo=" width="100" height="100"/></svg>"#;
        let result = validate_svg_content(svg.as_bytes());
        assert!(result.is_err());
    }

    // ═══════════════════════════════════════════
    // SVG → PNG End-to-End
    // ═══════════════════════════════════════════

    #[test]
    fn svg_to_png_decodes_back_with_expected_dimensions() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><rect x="0" y="0" width="32" height="32" fill="#ff0000"/></svg>"##;
        let f = temp_svg(svg);
        let img = rasterize_svg(f.path(), None).unwrap();

        // Encode to PNG
        let out = tempfile::Builder::new().suffix(".png").tempfile().unwrap();
        img.save(out.path()).unwrap();

        // Decode back
        let decoded = image::open(out.path()).unwrap();
        assert_eq!(decoded.width(), 32);
        assert_eq!(decoded.height(), 32);
    }
}
