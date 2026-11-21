// ==============================================================================
// OWNERSHIP:
//   Executes image conversions using the Rust image pipeline and format-specific encoding behavior.
//
// DO NOT ADD:
//   - Backend output path generation
//   - General extension/naming policy
//   - Thumbnail routing
//   - Frontend DTO mapping
//
// NOTES:
//   This file receives a planned output path; it does not decide where outputs belong.
// ==============================================================================
use super::options::{compute_resize_target, ImageOptions};
use crate::app_error::AppError;
use crate::conversion::types::{ConversionRequest, ConversionResult};
use image::ImageReader;
use std::path::Path;

/// Alpha-composite an RGBA image over a solid white background, returning raw RGB bytes.
/// This is an explicit helper to prevent transparent regions from becoming black in PDF.
fn flatten_rgba_to_rgb_over_white(rgba: &image::RgbaImage) -> Vec<u8> {
    let (w, h) = (rgba.width() as usize, rgba.height() as usize);
    let mut rgb = Vec::with_capacity(w * h * 3);
    for pixel in rgba.pixels() {
        let [r, g, b, a] = pixel.0;
        let alpha = a as f32 / 255.0;
        let inv = 1.0 - alpha;
        // Composite over white (255, 255, 255)
        rgb.push((r as f32 * alpha + 255.0 * inv).round() as u8);
        rgb.push((g as f32 * alpha + 255.0 * inv).round() as u8);
        rgb.push((b as f32 * alpha + 255.0 * inv).round() as u8);
    }
    rgb
}

/// Room for EXIF, ICC and XMP on top of raw pixel size. A JPEG bigger than its
/// own uncompressed pixels plus this is not embedded as is: it is decoded and
/// re-encoded instead, so the read stays bounded by the pixel budget.
const JPEG_EMBED_EXTRA_BYTES: u64 = 4 * 1024 * 1024;

/// Generate one PDF page from any readable raster input.
/// Without resize, grayscale/RGB JPEG is embedded directly as /DCTDecode.
/// Other raster formats are decoded once (including registered modules),
/// resized when requested, flattened over white, and written as RGB /FlateDecode.
/// Page policy: one output pixel equals one PDF point.
fn convert_to_pdf(
    input_path: &Path,
    output_path: &Path,
    options: &ImageOptions,
) -> Result<(), AppError> {
    use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref};

    let probed_format = super::probe::probe_image(input_path)?;
    if probed_format.0 == "svg" || probed_format.0 == "pdf" {
        return Err(AppError::Execute(format!(
            "{} to PDF conversion is not supported",
            probed_format.0.to_uppercase()
        )));
    }
    // Protect direct calls to the executor, not just the validate -> execute pipeline.
    if probed_format.0 == "gif" {
        super::validate::validate_static_gif(input_path)?;
    }

    let mut direct_jpeg = None;
    if probed_format.0 == "jpg" {
        // Parse dimensions before loading the entire JPEG into memory.
        let file = std::fs::File::open(input_path)
            .map_err(|e| AppError::Execute(format!("Failed to open JPEG: {}", e)))?;
        let mut decoder = image::codecs::jpeg::JpegDecoder::new(std::io::BufReader::new(file))
            .map_err(|e| AppError::Execute(format!("Failed to parse JPEG header: {}", e)))?;
        let (w, h) = image::ImageDecoder::dimensions(&decoder);
        crate::image_limits::check_pixel_budget(&crate::image_limits::dimensions_from_pair(w, h))?;
        // PDF readers ignore EXIF: a rotated or mirrored photo is decoded
        // (which applies the orientation) instead of embedded as is.
        let upright = super::formats::raster::orientation(&mut decoder)
            == image::metadata::Orientation::NoTransforms;
        if upright && compute_resize_target(w, h, options).is_none() {
            let color = image::ImageDecoder::color_type(&decoder);
            let grayscale = match color {
                image::ColorType::Rgb8 => false,
                image::ColorType::L8 => true,
                _ => {
                    return Err(AppError::Execute(format!(
                        "Unsupported JPEG color type {:?} for PDF embedding",
                        color
                    )));
                }
            };
            let channels = if grayscale { 1 } else { 3 };
            let cap = (w as u64) * (h as u64) * channels + JPEG_EMBED_EXTRA_BYTES;
            let fits = std::fs::metadata(input_path)
                .map(|m| m.len() <= cap)
                .unwrap_or(false);
            if fits {
                use std::io::Read;
                let mut bytes = Vec::new();
                std::fs::File::open(input_path)
                    .and_then(|f| f.take(cap + 1).read_to_end(&mut bytes))
                    .map_err(|e| AppError::Execute(format!("Failed to read JPEG: {}", e)))?;
                // The bytes that go into the PDF must be the JPEG that was
                // checked: same header, same size and color, not grown since.
                let same = bytes.len() as u64 <= cap
                    && image::codecs::jpeg::JpegDecoder::new(std::io::Cursor::new(&bytes))
                        .is_ok_and(|d| {
                            image::ImageDecoder::dimensions(&d) == (w, h)
                                && image::ImageDecoder::color_type(&d) == color
                        });
                if same {
                    direct_jpeg = Some((w, h, bytes, grayscale));
                }
            }
        }
    }

    let (width, height, image_data, filter, is_grayscale) =
        if let Some((w, h, bytes, grayscale)) = direct_jpeg {
            (w, h, bytes, Filter::DctDecode, grayscale)
        } else {
            // Defense in depth even when callers bypass the handler's validate step.
            crate::image_limits::validate_image_dimensions(input_path)?;
            let mut image = match super::formats::sniff_file(input_path).and_then(|m| m.decode) {
                Some(decode) => decode(input_path)?,
                None => ImageReader::open(input_path)
                    .map_err(|e| AppError::Execute(format!("Failed to open image: {}", e)))?
                    .decode()
                    .map_err(|e| AppError::Execute(format!("Failed to decode image: {}", e)))?,
            };
            if let Some((w, h)) = compute_resize_target(image.width(), image.height(), options) {
                image = image.resize_exact(w, h, image::imageops::FilterType::Lanczos3);
            }
            let (w, h) = (image.width(), image.height());
            let rgb = flatten_rgba_to_rgb_over_white(&image.to_rgba8());
            let compressed = miniz_oxide::deflate::compress_to_vec_zlib(&rgb, 6);
            (w, h, compressed, Filter::FlateDecode, false)
        };

    // Exactly five indirect PDF objects: catalog, pages, page, image, content.
    let catalog_id = Ref::new(1);
    let pages_id = Ref::new(2);
    let page_id = Ref::new(3);
    let image_id = Ref::new(4);
    let content_id = Ref::new(5);

    let mut pdf = Pdf::new();
    pdf.catalog(catalog_id).pages(pages_id);
    pdf.pages(pages_id).kids([page_id]).count(1);

    let mut page = pdf.page(page_id);
    page.parent(pages_id);
    page.media_box(Rect::new(0.0, 0.0, width as f32, height as f32));
    page.contents(content_id);
    page.resources().x_objects().pair(Name(b"Im1"), image_id);
    page.finish();

    let mut image_obj = pdf.image_xobject(image_id, &image_data);
    image_obj.width(width as i32);
    image_obj.height(height as i32);
    if is_grayscale {
        image_obj.color_space().device_gray();
    } else {
        image_obj.color_space().device_rgb();
    }
    image_obj.bits_per_component(8);
    image_obj.filter(filter);
    image_obj.finish();

    let mut content = Content::new();
    content.save_state();
    content.transform([width as f32, 0.0, 0.0, height as f32, 0.0, 0.0]);
    content.x_object(Name(b"Im1"));
    content.restore_state();
    pdf.stream(content_id, &content.finish());

    zero_fs::write_output(
        output_path,
        &pdf.finish(),
        crate::conversion::types::clobber(options.overwrite),
    )
    .map_err(|e| AppError::Io(format!("Failed to write PDF: {}", e)))?;
    Ok(())
}

pub fn execute_image_conversion(request: &ConversionRequest) -> Result<ConversionResult, AppError> {
    let input_path = Path::new(&request.input_path);
    let output_path = Path::new(&request.output_path);

    let options: ImageOptions = serde_json::from_value(request.options.clone())
        .map_err(|e| AppError::Execute(format!("Invalid options: {}", e)))?;

    // PDF output uses a dedicated generation path — it reads the input itself,
    // so intercept before the generic decode to avoid decoding the image twice.
    if request.output_format.0.to_lowercase() == "pdf" {
        convert_to_pdf(input_path, output_path, &options)?;
        return Ok(ConversionResult {
            success: true,
            output_path: Some(request.output_path.clone()),
            message: "Image to PDF conversion completed successfully".to_string(),
            input_integrity: None,
            output_integrity: None,
        });
    }

    // Determine if input is SVG based on resolved input format or extension fallback
    let input_ext = input_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let is_svg = input_ext == "svg";

    // Load image: SVG uses the dedicated rasterizer, raster uses the standard path
    let mut img = if is_svg {
        // Compute target dimensions for SVG vector scaling
        let svg_dims = super::svg::validate_svg_dimensions(input_path)?;
        let target_size = compute_resize_target(svg_dims.width, svg_dims.height, &options)
            .or(Some((svg_dims.width, svg_dims.height)));
        super::svg::rasterize_svg(input_path, target_size)?
    } else {
        // Defense-in-depth: reject oversized images before full pixel decode
        crate::image_limits::validate_image_dimensions(input_path)?;

        // Load raster image: a format module if one claims the file, else the image crate
        match super::formats::sniff_file(input_path).and_then(|m| m.decode) {
            Some(decode) => decode(input_path)?,
            None => ImageReader::open(input_path)
                .map_err(|e| AppError::Execute(format!("Failed to open image: {}", e)))?
                .decode()
                .map_err(|e| AppError::Execute(format!("Failed to decode image: {}", e)))?,
        }
    };

    // Apply Resizing if needed (SVG already scaled during rasterization)
    if !is_svg {
        if let Some((target_w, target_h)) =
            compute_resize_target(img.width(), img.height(), &options)
        {
            img = img.resize_exact(target_w, target_h, image::imageops::FilterType::Lanczos3);
        }
    }

    // Transparent SVG areas would turn black in formats without alpha.
    let target = request.output_format.0.to_lowercase();
    if is_svg && matches!(target.as_str(), "jpg" | "jpeg" | "bmp") {
        let rgba = img.to_rgba8();
        let (w, h) = (rgba.width(), rgba.height());
        let mut white_bg = image::RgbaImage::from_pixel(w, h, image::Rgba([255, 255, 255, 255]));
        image::imageops::overlay(&mut white_bg, &rgba, 0, 0);
        img = image::DynamicImage::ImageRgba8(white_bg);
    }

    // Every writable format is a module: it encodes to bytes, the write is shared.
    let encode = super::formats::find(&target)
        .and_then(|m| m.encode)
        .ok_or_else(|| AppError::Execute(format!("Unsupported output format: {}", target)))?;
    let buffer = encode(&img, &options)?;
    zero_fs::write_output(output_path, &buffer, request.clobber())
        .map_err(|e| AppError::Io(format!("Failed to write output image: {}", e)))?;
    Ok(ConversionResult {
        success: true,
        output_path: Some(request.output_path.clone()),
        message: "Image conversion completed successfully".to_string(),
        input_integrity: None,
        output_integrity: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversion::types::{AssetFamily, OutputFormat};
    use image::{ImageBuffer, RgbImage};
    use serde_json::json;

    /// Assert no temporary debris (zero_fs .tmp.UUID pattern) exists in a directory.
    fn assert_no_temp_debris(dir: &std::path::Path) {
        let debris: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp."))
            .collect();
        assert!(
            debris.is_empty(),
            "Expected no temp debris in {:?}, found: {:?}",
            dir,
            debris.iter().map(|e| e.file_name()).collect::<Vec<_>>()
        );
    }

    /// Create a real 4x4 PNG image in a temp file
    fn create_test_png() -> tempfile::NamedTempFile {
        let img: RgbImage = ImageBuffer::from_fn(4, 4, |x, y| {
            image::Rgb([(x * 64) as u8, (y * 64) as u8, 128])
        });
        let f = tempfile::Builder::new().suffix(".png").tempfile().unwrap();
        img.save(f.path()).unwrap();
        f
    }

    fn make_request(
        input: &str,
        output: &str,
        format: &str,
        quality: u8,
        overwrite: bool,
    ) -> ConversionRequest {
        ConversionRequest {
            family: AssetFamily::Image,
            input_path: input.to_string(),
            output_path: output.to_string(),
            output_root: "/".to_string(),
            output_format: OutputFormat(format.to_string()),
            options: json!({ "quality": quality, "overwrite": overwrite }),
        }
    }

    // ═══════════════════════════════════════════
    // Successful Conversion Paths
    // ═══════════════════════════════════════════

    /// Create a real 4x4 ICO image in a temp file.
    /// ICO PNG frames must be RGBA — the image crate ICO decoder enforces this.
    fn create_test_ico() -> tempfile::NamedTempFile {
        let img: image::RgbaImage = image::ImageBuffer::from_fn(4, 4, |x, y| {
            image::Rgba([(x * 64) as u8, (y * 64) as u8, 128, 255])
        });
        let f = tempfile::Builder::new().suffix(".ico").tempfile().unwrap();
        img.save_with_format(f.path(), image::ImageFormat::Ico)
            .unwrap();
        f
    }

    #[test]
    fn execute_converts_ico_to_webp_successfully() {
        let input = create_test_ico();
        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("result.webp");
        let req = make_request(
            input.path().to_str().unwrap(),
            output.to_str().unwrap(),
            "webp",
            80,
            false,
        );
        let result = execute_image_conversion(&req);
        assert!(result.is_ok());
        let res = result.unwrap();
        assert!(res.success);
        assert!(output.exists(), "Output file should exist after conversion");
        assert!(
            output.metadata().unwrap().len() > 0,
            "Output file should not be empty"
        );
    }
    #[test]
    fn execute_converts_png_to_webp_successfully() {
        let input = create_test_png();
        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("result.webp");
        let req = make_request(
            input.path().to_str().unwrap(),
            output.to_str().unwrap(),
            "webp",
            80,
            false,
        );
        let result = execute_image_conversion(&req);
        assert!(result.is_ok());
        let res = result.unwrap();
        assert!(res.success);
        assert!(output.exists(), "Output file should exist after conversion");
        assert!(
            output.metadata().unwrap().len() > 0,
            "Output file should not be empty"
        );
    }

    #[test]
    fn execute_converts_with_resize_max_width() {
        // create_test_png creates a 4x4 image
        let input = create_test_png();
        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("resized.png");
        let mut req = make_request(
            input.path().to_str().unwrap(),
            output.to_str().unwrap(),
            "png",
            80,
            false,
        );
        req.options = json!({
            "quality": 80,
            "overwrite": false,
            "resize_mode": "max_width",
            "resize_width": 2
        });

        let result = execute_image_conversion(&req);
        assert!(result.is_ok());

        // Verify dimensions are now 2x2
        let img = ImageReader::open(&output).unwrap().decode().unwrap();
        assert_eq!(img.width(), 2);
        assert_eq!(img.height(), 2);
    }

    #[test]
    fn execute_converts_png_to_jpg_successfully() {
        let input = create_test_png();
        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("result.jpg");
        let req = make_request(
            input.path().to_str().unwrap(),
            output.to_str().unwrap(),
            "jpg",
            90,
            false,
        );
        let result = execute_image_conversion(&req);
        assert!(result.is_ok());
        assert!(output.exists());
    }

    #[test]
    fn execute_jpeg_quality_produces_meaningful_size_difference() {
        // Create a test image with high entropy so JPEG compression has a visible effect
        let img: RgbImage = ImageBuffer::from_fn(64, 64, |x, y| {
            // A gradient combined with high frequency noise pattern
            let r = (x.wrapping_mul(17) ^ y.wrapping_mul(23)) as u8;
            let g = (x.wrapping_mul(31) ^ y.wrapping_mul(11)) as u8;
            let b = (x.wrapping_mul(7) ^ y.wrapping_mul(41)) as u8;
            image::Rgb([r, g, b])
        });
        let f = tempfile::Builder::new().suffix(".png").tempfile().unwrap();
        img.save(f.path()).unwrap();

        let output_dir = tempfile::tempdir().unwrap();

        let output_low = output_dir.path().join("result_low.jpg");
        let req_low = make_request(
            f.path().to_str().unwrap(),
            output_low.to_str().unwrap(),
            "jpg",
            10,
            false,
        );

        let output_high = output_dir.path().join("result_high.jpg");
        let req_high = make_request(
            f.path().to_str().unwrap(),
            output_high.to_str().unwrap(),
            "jpg",
            90,
            false,
        );

        assert!(execute_image_conversion(&req_low).is_ok());
        assert!(execute_image_conversion(&req_high).is_ok());

        assert!(output_low.exists());
        assert!(output_high.exists());

        let size_low = output_low.metadata().unwrap().len();
        let size_high = output_high.metadata().unwrap().len();

        // Assert that low quality produces a meaningfully smaller file than high quality
        assert!(
            size_low < size_high,
            "Low quality JPEG ({}) should be smaller than high quality JPEG ({})",
            size_low,
            size_high
        );

        // Assert bytes differ
        let bytes_low = std::fs::read(&output_low).unwrap();
        let bytes_high = std::fs::read(&output_high).unwrap();
        assert_ne!(
            bytes_low, bytes_high,
            "Outputs must differ for different quality values"
        );
    }

    #[test]
    fn execute_converts_png_to_png_successfully() {
        let input = create_test_png();
        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("copy.png");
        let req = make_request(
            input.path().to_str().unwrap(),
            output.to_str().unwrap(),
            "png",
            80,
            false,
        );
        let result = execute_image_conversion(&req);
        assert!(result.is_ok());
        assert!(output.exists());
    }

    #[test]
    fn execute_png_quality_ignored_produces_identical_output() {
        let img: RgbImage = ImageBuffer::from_fn(32, 32, |x, y| {
            image::Rgb([(x * 8) as u8, (y * 8) as u8, 128])
        });
        let f = tempfile::Builder::new().suffix(".png").tempfile().unwrap();
        img.save(f.path()).unwrap();

        let output_dir = tempfile::tempdir().unwrap();

        let output_10 = output_dir.path().join("result_10.png");
        let req_10 = make_request(
            f.path().to_str().unwrap(),
            output_10.to_str().unwrap(),
            "png",
            10,
            false,
        );

        let output_90 = output_dir.path().join("result_90.png");
        let req_90 = make_request(
            f.path().to_str().unwrap(),
            output_90.to_str().unwrap(),
            "png",
            90,
            false,
        );

        assert!(execute_image_conversion(&req_10).is_ok());
        assert!(execute_image_conversion(&req_90).is_ok());

        let bytes_10 = std::fs::read(&output_10).unwrap();
        let bytes_90 = std::fs::read(&output_90).unwrap();

        // Assert identical bytes since PNG quality is ignored in v1
        assert_eq!(
            bytes_10, bytes_90,
            "PNG quality should be intentionally ignored"
        );
    }

    #[test]
    fn execute_converts_png_to_jpeg_with_jpeg_spelling() {
        let input = create_test_png();
        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("result.jpeg");
        let req = make_request(
            input.path().to_str().unwrap(),
            output.to_str().unwrap(),
            "jpeg",
            75,
            false,
        );
        let result = execute_image_conversion(&req);
        assert!(result.is_ok());
        assert!(output.exists());
    }

    // ═══════════════════════════════════════════
    // Result Correctness
    // ═══════════════════════════════════════════

    #[test]
    fn execute_returns_output_path_in_result() {
        let input = create_test_png();
        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("result.webp");
        let req = make_request(
            input.path().to_str().unwrap(),
            output.to_str().unwrap(),
            "webp",
            80,
            false,
        );
        let result = execute_image_conversion(&req).unwrap();
        assert_eq!(result.output_path.unwrap(), output.to_str().unwrap());
    }

    #[test]
    fn execute_cleans_up_tmp_file_on_success() {
        let input = create_test_png();
        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("result.webp");
        let req = make_request(
            input.path().to_str().unwrap(),
            output.to_str().unwrap(),
            "webp",
            80,
            false,
        );
        execute_image_conversion(&req).unwrap();
        assert!(output.exists());
        assert_no_temp_debris(output_dir.path());
    }

    // ═══════════════════════════════════════════
    // Failure Paths
    // ═══════════════════════════════════════════

    #[test]
    fn execute_fails_for_nonexistent_input() {
        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("result.webp");
        let req = make_request(
            "/nonexistent/fake_image.png",
            output.to_str().unwrap(),
            "webp",
            80,
            false,
        );
        let result = execute_image_conversion(&req);
        assert!(result.is_err());
    }

    #[test]
    fn execute_fails_for_unsupported_output_format() {
        let input = create_test_png();
        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("result.gif");
        let req = make_request(
            input.path().to_str().unwrap(),
            output.to_str().unwrap(),
            "gif",
            80,
            false,
        );
        let result = execute_image_conversion(&req);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Unsupported output format"));
    }

    #[test]
    fn execute_fails_for_corrupt_input_file() {
        // Write garbage bytes as a "png" file
        let mut f = tempfile::Builder::new().suffix(".png").tempfile().unwrap();
        std::io::Write::write_all(&mut f, b"this is not an image").unwrap();
        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("result.webp");
        let req = make_request(
            f.path().to_str().unwrap(),
            output.to_str().unwrap(),
            "webp",
            80,
            false,
        );
        let result = execute_image_conversion(&req);
        assert!(result.is_err());
    }

    #[test]
    fn execute_fails_with_malformed_options() {
        let input = create_test_png();
        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("result.webp");
        let req = ConversionRequest {
            family: AssetFamily::Image,
            input_path: input.path().to_str().unwrap().to_string(),
            output_path: output.to_str().unwrap().to_string(),
            output_root: "/".to_string(),
            output_format: OutputFormat("webp".to_string()),
            options: json!("invalid_string_not_object"),
        };
        let result = execute_image_conversion(&req);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid options"));
    }

    // ═══════════════════════════════════════════
    // Case Insensitivity
    // ═══════════════════════════════════════════

    #[test]
    fn execute_handles_uppercase_format() {
        let input = create_test_png();
        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("result.webp");
        let req = make_request(
            input.path().to_str().unwrap(),
            output.to_str().unwrap(),
            "WEBP",
            80,
            false,
        );
        let result = execute_image_conversion(&req);
        assert!(result.is_ok());
    }

    #[test]
    fn test_image_png_to_webp_conversion_outputs_valid_webp() {
        let input = create_test_png();
        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("sanity_check.webp");

        let req = make_request(
            input.path().to_str().unwrap(),
            output.to_str().unwrap(),
            "webp",
            80,
            false,
        );

        let result = execute_image_conversion(&req);

        // Assertions
        assert!(result.is_ok(), "Conversion should succeed");
        let res = result.unwrap();
        assert!(res.success, "Conversion result should indicate success");
        assert!(output.exists(), "Output file must exist");

        let meta = std::fs::metadata(&output).unwrap();
        assert!(meta.len() > 0, "Output file must not be empty");

        // Assert WebP signature (RIFF...WEBP)
        let mut f = std::fs::File::open(&output).unwrap();
        let mut buf = [0u8; 12];
        std::io::Read::read_exact(&mut f, &mut buf).unwrap();
        assert_eq!(&buf[0..4], b"RIFF", "Should have RIFF signature");
        assert_eq!(&buf[8..12], b"WEBP", "Should have WEBP signature");

        // Assert no temp debris (zero_fs UUID-based temp files)
        assert_no_temp_debris(output_dir.path());
    }

    #[test]
    fn test_image_execute_overwrite_true_replaces_existing_file() {
        let input = create_test_png();
        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("overwrite_test.webp");

        std::fs::write(&output, b"dummy old bytes").unwrap();

        let req = make_request(
            input.path().to_str().unwrap(),
            output.to_str().unwrap(),
            "webp",
            80,
            true,
        );

        let result = execute_image_conversion(&req);

        assert!(result.is_ok(), "Conversion should succeed");
        assert!(output.exists(), "Output file must exist");

        let meta = std::fs::metadata(&output).unwrap();
        assert!(meta.len() > 0, "Output file must not be empty");

        let mut f = std::fs::File::open(&output).unwrap();
        let mut buf = [0u8; 12];
        std::io::Read::read_exact(&mut f, &mut buf).unwrap();
        assert_eq!(&buf[0..4], b"RIFF");
        assert_eq!(&buf[8..12], b"WEBP");

        assert_no_temp_debris(output_dir.path());
    }

    #[test]
    fn test_image_execute_fails_and_preserves_original_on_corrupt_input() {
        let mut f = tempfile::Builder::new().suffix(".png").tempfile().unwrap();
        std::io::Write::write_all(&mut f, b"this is not an image").unwrap();

        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("preserve_test.webp");

        let dummy_content = b"original dummy bytes for safe preserve test";
        std::fs::write(&output, dummy_content).unwrap();

        let req = make_request(
            f.path().to_str().unwrap(),
            output.to_str().unwrap(),
            "webp",
            80,
            true,
        );

        let result = execute_image_conversion(&req);

        assert!(result.is_err(), "Conversion should fail");
        assert!(output.exists(), "Existing output file must still exist");

        let content = std::fs::read(&output).unwrap();
        assert_eq!(
            content, dummy_content,
            "Existing output bytes must exactly match original dummy bytes"
        );

        assert_no_temp_debris(output_dir.path());
    }

    #[test]
    fn test_concurrent_batch_real_image_io_distinct_outputs() {
        let output_dir = tempfile::tempdir().unwrap();
        let mut handles = Vec::new();
        let mut inputs = Vec::new();

        for i in 0..3 {
            let input = create_test_png();
            let out_name = format!("concurrent_out_{}.webp", i);
            let output_path = output_dir.path().join(&out_name);

            let path_buf = input.into_temp_path().keep().unwrap();
            inputs.push(path_buf.clone());

            let req = make_request(
                path_buf.to_str().unwrap(),
                output_path.to_str().unwrap(),
                "webp",
                80,
                false,
            );

            handles.push(std::thread::spawn(move || execute_image_conversion(&req)));
        }

        for handle in handles {
            let result = handle.join().unwrap();
            assert!(result.is_ok(), "Concurrent conversion failed");
        }

        for i in 0..3 {
            let out_name = format!("concurrent_out_{}.webp", i);
            let output_path = output_dir.path().join(&out_name);
            assert!(output_path.exists(), "Output file must exist");

            let mut f = std::fs::File::open(&output_path).unwrap();
            let mut buf = [0u8; 12];
            std::io::Read::read_exact(&mut f, &mut buf).unwrap();
            assert_eq!(&buf[0..4], b"RIFF");
            assert_eq!(&buf[8..12], b"WEBP");
        }

        assert_no_temp_debris(output_dir.path());

        for path in inputs {
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn test_image_execute_fails_when_output_path_is_directory() {
        let input = create_test_png();
        // Create a controlled parent so we don't scan the global temp directory
        let parent_dir = tempfile::tempdir().unwrap();
        let output_path = parent_dir.path().join("target_dir");
        std::fs::create_dir(&output_path).unwrap();

        let req = make_request(
            input.path().to_str().unwrap(),
            output_path.to_str().unwrap(),
            "webp",
            80,
            true, // Overwrite true to bypass any plan-level checks if they existed (they don't in execute)
        );

        let result = execute_image_conversion(&req);

        // write_atomic rename fails because the target is a directory
        assert!(result.is_err(), "Should fail to overwrite a directory");

        // zero_fs TempFileGuard cleans up temp debris on rename failure
        assert_no_temp_debris(parent_dir.path());
    }

    // ═══════════════════════════════════════════
    // BMP / TIFF Expansion Tests
    // ═══════════════════════════════════════════

    #[test]
    fn execute_converts_png_to_bmp_successfully() {
        let input = create_test_png();
        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("result.bmp");
        let req = make_request(
            input.path().to_str().unwrap(),
            output.to_str().unwrap(),
            "bmp",
            80,
            false,
        );
        let result = execute_image_conversion(&req);
        assert!(result.is_ok());
        assert!(output.exists());

        // Verify it's actually a BMP
        let mut f = std::fs::File::open(&output).unwrap();
        let mut buf = [0u8; 2];
        std::io::Read::read_exact(&mut f, &mut buf).unwrap();
        assert_eq!(&buf, b"BM");
    }

    #[test]
    fn execute_converts_png_to_tiff_successfully() {
        let input = create_test_png();
        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("result.tiff");
        let req = make_request(
            input.path().to_str().unwrap(),
            output.to_str().unwrap(),
            "tiff",
            80,
            false,
        );
        let result = execute_image_conversion(&req);
        assert!(result.is_ok());
        assert!(output.exists());

        // Verify it's a TIFF (check little endian signature as fallback/default)
        let mut f = std::fs::File::open(&output).unwrap();
        let mut buf = [0u8; 2];
        std::io::Read::read_exact(&mut f, &mut buf).unwrap();
        assert!(
            buf == *b"II" || buf == *b"MM",
            "Expected TIFF signature II or MM"
        );
    }

    #[test]
    fn execute_handles_tiff_alias_spelling() {
        let input = create_test_png();
        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("result.tiff");
        let req = make_request(
            input.path().to_str().unwrap(),
            output.to_str().unwrap(),
            "tif", // Alias
            80,
            false,
        );
        let result = execute_image_conversion(&req);
        assert!(result.is_ok());
    }

    // ═══════════════════════════════════════════
    // ICO Output Tests
    // ═══════════════════════════════════════════

    /// Create a 64x64 RGBA PNG for ICO output tests (within 256x256 limit)
    fn create_test_png_rgba_64() -> tempfile::NamedTempFile {
        let img: image::RgbaImage = image::ImageBuffer::from_fn(64, 64, |x, y| {
            image::Rgba([(x * 4) as u8, (y * 4) as u8, 128, 255])
        });
        let f = tempfile::Builder::new().suffix(".png").tempfile().unwrap();
        img.save(f.path()).unwrap();
        f
    }

    /// Create a 64x64 RGB JPG for ICO output tests (no alpha channel)
    fn create_test_jpg_rgb_64() -> tempfile::NamedTempFile {
        let img: RgbImage = ImageBuffer::from_fn(64, 64, |x, y| {
            image::Rgb([(x * 4) as u8, (y * 4) as u8, 128])
        });
        let f = tempfile::Builder::new().suffix(".jpg").tempfile().unwrap();
        img.save(f.path()).unwrap();
        f
    }

    #[test]
    fn execute_converts_png_rgba_to_ico_successfully() {
        let input = create_test_png_rgba_64();
        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("result.ico");
        let req = make_request(
            input.path().to_str().unwrap(),
            output.to_str().unwrap(),
            "ico",
            80,
            false,
        );
        let result = execute_image_conversion(&req);
        assert!(result.is_ok(), "PNG RGBA → ICO conversion should succeed");
        let res = result.unwrap();
        assert!(res.success);
        assert!(output.exists(), "Output .ico file should exist");

        // Decode back: prove the generated ICO is valid
        let decoded = ImageReader::open(&output)
            .unwrap()
            .decode()
            .expect("Generated .ico must be decodable by the image crate");
        assert_eq!(decoded.width(), 64, "Decoded ICO width must match");
        assert_eq!(decoded.height(), 64, "Decoded ICO height must match");
        assert!(decoded.width() <= 256 && decoded.height() <= 256);
    }

    #[test]
    fn execute_converts_jpg_rgb_to_ico_successfully() {
        let input = create_test_jpg_rgb_64();
        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("result.ico");
        let req = make_request(
            input.path().to_str().unwrap(),
            output.to_str().unwrap(),
            "ico",
            80,
            false,
        );
        let result = execute_image_conversion(&req);
        assert!(
            result.is_ok(),
            "JPG RGB → ICO conversion should succeed (proves RGBA conversion)"
        );
        let res = result.unwrap();
        assert!(res.success);
        assert!(output.exists(), "Output .ico file should exist");

        // Decode back: prove RGB→RGBA conversion was safe
        let decoded = ImageReader::open(&output)
            .unwrap()
            .decode()
            .expect("Generated .ico from JPG must be decodable");
        assert_eq!(decoded.width(), 64, "Decoded ICO width must match");
        assert_eq!(decoded.height(), 64, "Decoded ICO height must match");
        assert!(decoded.width() <= 256 && decoded.height() <= 256);
    }

    // ═══════════════════════════════════════════
    // SVG Execution Tests
    // ═══════════════════════════════════════════

    fn create_test_svg() -> tempfile::NamedTempFile {
        // A 64x64 SVG with a transparent background and a 32x32 red center
        let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"64\" height=\"64\"><rect x=\"16\" y=\"16\" width=\"32\" height=\"32\" fill=\"#ff0000\"/></svg>";
        let mut f = tempfile::Builder::new().suffix(".svg").tempfile().unwrap();
        std::io::Write::write_all(&mut f, svg.as_bytes()).unwrap();
        f
    }

    #[test]
    fn execute_converts_svg_to_jpg_with_white_bg() {
        let input = create_test_svg();
        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("result.jpg");
        let req = make_request(
            input.path().to_str().unwrap(),
            output.to_str().unwrap(),
            "jpg",
            80,
            false,
        );
        let result = execute_image_conversion(&req);
        assert!(result.is_ok(), "SVG to JPG should succeed");
        assert!(output.exists());

        let decoded = ImageReader::open(&output)
            .unwrap()
            .decode()
            .unwrap()
            .into_rgb8();
        // Check transparent corner is white, not black
        let corner_px = decoded.get_pixel(0, 0);
        assert!(
            corner_px[0] > 250 && corner_px[1] > 250 && corner_px[2] > 250,
            "Expected white background, got {:?}",
            corner_px
        );

        // Check center is red
        let center_px = decoded.get_pixel(32, 32);
        assert!(
            center_px[0] > 200 && center_px[1] < 50 && center_px[2] < 50,
            "Expected red center, got {:?}",
            center_px
        );
    }

    #[test]
    fn execute_converts_svg_to_bmp_with_white_bg() {
        let input = create_test_svg();
        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("result.bmp");
        let req = make_request(
            input.path().to_str().unwrap(),
            output.to_str().unwrap(),
            "bmp",
            80,
            false,
        );
        let result = execute_image_conversion(&req);
        assert!(result.is_ok(), "SVG to BMP should succeed");
        assert!(output.exists());

        let decoded = ImageReader::open(&output)
            .unwrap()
            .decode()
            .unwrap()
            .into_rgb8();
        // Check transparent corner is white, not black
        let corner_px = decoded.get_pixel(0, 0);
        assert!(
            corner_px[0] > 250 && corner_px[1] > 250 && corner_px[2] > 250,
            "Expected white background, got {:?}",
            corner_px
        );
    }

    #[test]
    fn execute_converts_svg_to_ico_successfully() {
        let input = create_test_svg();
        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("result.ico");
        let req = make_request(
            input.path().to_str().unwrap(),
            output.to_str().unwrap(),
            "ico",
            80,
            false,
        );
        let result = execute_image_conversion(&req);
        assert!(result.is_ok(), "SVG to ICO should succeed");
        assert!(output.exists());

        let decoded = ImageReader::open(&output).unwrap().decode().unwrap();
        assert_eq!(decoded.width(), 64);
        assert_eq!(decoded.height(), 64);
    }

    // ═══════════════════════════════════════════
    // GIF Execution Tests
    // ═══════════════════════════════════════════

    fn create_test_static_gif() -> tempfile::NamedTempFile {
        let mut file = tempfile::Builder::new().suffix(".gif").tempfile().unwrap();
        // 1x1 static GIF
        let data = b"GIF89a\x01\x00\x01\x00\x80\x00\x00\xff\xff\xff\x00\x00\x00!\xf9\x04\x01\x00\x00\x00\x00,\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02D\x01\x00;";
        use std::io::Write;
        file.write_all(data).unwrap();
        file
    }

    #[test]
    fn execute_converts_static_gif_to_png() {
        let input = create_test_static_gif();
        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("result.png");
        let req = make_request(
            input.path().to_str().unwrap(),
            output.to_str().unwrap(),
            "png",
            80,
            false,
        );
        let result = execute_image_conversion(&req);
        assert!(result.is_ok(), "Static GIF to PNG should succeed");
        assert!(output.exists());

        let decoded = ImageReader::open(&output).unwrap().decode().unwrap();
        assert_eq!(decoded.width(), 1);
        assert_eq!(decoded.height(), 1);
    }

    // ═══════════════════════════════════════════
    // Alpha Flatten Helper Tests
    // ═══════════════════════════════════════════

    #[test]
    fn flatten_alpha_transparent_becomes_white() {
        let mut img = image::RgbaImage::new(1, 1);
        img.put_pixel(0, 0, image::Rgba([0, 0, 0, 0])); // fully transparent
        let rgb = super::flatten_rgba_to_rgb_over_white(&img);
        assert_eq!(
            rgb,
            vec![255, 255, 255],
            "Fully transparent pixel must become white"
        );
    }

    #[test]
    fn flatten_alpha_semitransparent_composites_correctly() {
        let mut img = image::RgbaImage::new(1, 1);
        // 50% red over white: (255*0.5 + 255*0.5, 0*0.5 + 255*0.5, 0*0.5 + 255*0.5) = (255, 128, 128)
        img.put_pixel(0, 0, image::Rgba([255, 0, 0, 128]));
        let rgb = super::flatten_rgba_to_rgb_over_white(&img);
        // alpha = 128/255 ≈ 0.502
        // r = 255*0.502 + 255*0.498 = 255
        // g = 0*0.502 + 255*0.498 ≈ 127
        // b = 0*0.502 + 255*0.498 ≈ 127
        assert_eq!(rgb[0], 255, "Red channel");
        assert!(
            (rgb[1] as i16 - 127).abs() <= 1,
            "Green channel should be ~127, got {}",
            rgb[1]
        );
        assert!(
            (rgb[2] as i16 - 127).abs() <= 1,
            "Blue channel should be ~127, got {}",
            rgb[2]
        );
    }

    #[test]
    fn flatten_alpha_opaque_black_stays_black() {
        let mut img = image::RgbaImage::new(1, 1);
        img.put_pixel(0, 0, image::Rgba([0, 0, 0, 255])); // fully opaque black
        let rgb = super::flatten_rgba_to_rgb_over_white(&img);
        assert_eq!(rgb, vec![0, 0, 0], "Fully opaque black must remain black");
    }

    // ═══════════════════════════════════════════
    // PDF Execution Tests
    // ═══════════════════════════════════════════

    #[test]
    fn execute_converts_png_to_pdf_successfully() {
        let input = create_test_png();
        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("result.pdf");
        let req = make_request(
            input.path().to_str().unwrap(),
            output.to_str().unwrap(),
            "pdf",
            80,
            false,
        );
        let result = execute_image_conversion(&req);
        assert!(result.is_ok(), "PNG to PDF should succeed");
        assert!(output.exists());
        let content = std::fs::read(&output).unwrap();
        assert!(!content.is_empty(), "PDF must not be empty");
        let text = String::from_utf8_lossy(&content);
        assert!(text.starts_with("%PDF-"), "Must start with %PDF-");
        assert!(text.contains("/Type /Catalog"), "Must contain Catalog");
        assert!(text.contains("/Type /Page"), "Must contain Page");
        assert!(
            text.contains("/Subtype /Image"),
            "Must contain Image XObject"
        );
        assert!(text.contains("/FlateDecode"), "PNG must use FlateDecode");
        // MediaBox should match 4x4 test image dimensions
        assert!(
            text.contains("/MediaBox [0 0 4 4]"),
            "MediaBox must match image dims, got: {}",
            text.lines()
                .find(|l| l.contains("MediaBox"))
                .unwrap_or("NOT FOUND")
        );
        assert_no_temp_debris(output_dir.path());
    }

    #[test]
    fn execute_converts_jpg_to_pdf_successfully() {
        // Create a real JPG
        let img: RgbImage = ImageBuffer::from_fn(8, 8, |x, y| {
            image::Rgb([(x * 32) as u8, (y * 32) as u8, 128])
        });
        let f = tempfile::Builder::new().suffix(".jpg").tempfile().unwrap();
        img.save(f.path()).unwrap();

        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("result.pdf");
        let req = make_request(
            f.path().to_str().unwrap(),
            output.to_str().unwrap(),
            "pdf",
            80,
            false,
        );
        let result = execute_image_conversion(&req);
        assert!(result.is_ok(), "JPG to PDF should succeed");
        assert!(output.exists());
        let content = std::fs::read(&output).unwrap();
        let text = String::from_utf8_lossy(&content);
        assert!(text.starts_with("%PDF-"));
        assert!(text.contains("/DCTDecode"), "JPG must use DCTDecode");
        assert!(text.contains("/Subtype /Image"));
        assert!(text.contains("/MediaBox [0 0 8 8]"));
        assert_no_temp_debris(output_dir.path());
    }

    #[test]
    fn oversized_jpeg_is_reencoded_not_read_whole() {
        // 8x8 picture followed by 5 MB of junk: far past its pixel size, so
        // it is decoded (bounded) instead of embedded byte for byte.
        let img: RgbImage = ImageBuffer::from_fn(8, 8, |_, _| image::Rgb([200, 40, 40]));
        let f = tempfile::Builder::new().suffix(".jpg").tempfile().unwrap();
        img.save(f.path()).unwrap();
        let mut bytes = std::fs::read(f.path()).unwrap();
        bytes.resize(bytes.len() + 5 * 1024 * 1024, 0);
        std::fs::write(f.path(), &bytes).unwrap();

        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("result.pdf");
        let req = make_request(
            f.path().to_str().unwrap(),
            output.to_str().unwrap(),
            "pdf",
            80,
            false,
        );
        execute_image_conversion(&req).unwrap();
        let pdf = std::fs::read(&output).unwrap();
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("/FlateDecode") && !text.contains("/DCTDecode"));
        assert!(
            pdf.len() < 1024 * 1024,
            "junk must not be copied into the PDF"
        );
    }

    #[test]
    fn execute_converts_webp_to_pdf_successfully() {
        // Create a real WebP via PNG->save as webp
        let img: image::RgbaImage = image::ImageBuffer::from_fn(6, 6, |x, y| {
            image::Rgba([(x * 40) as u8, (y * 40) as u8, 128, 255])
        });
        let f = tempfile::Builder::new().suffix(".webp").tempfile().unwrap();
        img.save(f.path()).unwrap();

        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("result.pdf");
        let req = make_request(
            f.path().to_str().unwrap(),
            output.to_str().unwrap(),
            "pdf",
            80,
            false,
        );
        let result = execute_image_conversion(&req);
        assert!(result.is_ok(), "WebP to PDF should succeed");
        assert!(output.exists());
        let content = std::fs::read(&output).unwrap();
        let text = String::from_utf8_lossy(&content);
        assert!(text.starts_with("%PDF-"));
        assert!(text.contains("/FlateDecode"), "WebP must use FlateDecode");
        assert!(text.contains("/MediaBox [0 0 6 6]"));
        assert_no_temp_debris(output_dir.path());
    }

    #[test]
    fn execute_png_rgba_to_pdf_flattens_alpha() {
        // Create a PNG with transparent pixels
        let img: image::RgbaImage = image::ImageBuffer::from_fn(2, 2, |_, _| {
            image::Rgba([255, 0, 0, 0]) // fully transparent red
        });
        let f = tempfile::Builder::new().suffix(".png").tempfile().unwrap();
        img.save(f.path()).unwrap();

        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("alpha_test.pdf");
        let req = make_request(
            f.path().to_str().unwrap(),
            output.to_str().unwrap(),
            "pdf",
            80,
            false,
        );
        let result = execute_image_conversion(&req);
        assert!(result.is_ok(), "Transparent PNG to PDF should succeed");
        assert!(output.exists());
        // The PDF should exist and be valid — transparency flattened to white
        let content = std::fs::read(&output).unwrap();
        assert!(String::from_utf8_lossy(&content).starts_with("%PDF-"));
    }

    #[test]
    fn execute_malformed_image_to_pdf_fails_cleanly() {
        let mut f = tempfile::Builder::new().suffix(".png").tempfile().unwrap();
        std::io::Write::write_all(&mut f, b"this is not an image").unwrap();
        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("should_fail.pdf");
        let req = make_request(
            f.path().to_str().unwrap(),
            output.to_str().unwrap(),
            "pdf",
            80,
            false,
        );
        let result = execute_image_conversion(&req);
        assert!(result.is_err(), "Malformed image to PDF must fail");
        assert!(
            !output.exists(),
            "No output file should be created on failure"
        );
        assert_no_temp_debris(output_dir.path());
    }

    #[test]
    fn execute_pdf_output_path_uses_pdf_extension() {
        let input = create_test_png();
        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("test_output.pdf");
        let req = make_request(
            input.path().to_str().unwrap(),
            output.to_str().unwrap(),
            "pdf",
            80,
            false,
        );
        let result = execute_image_conversion(&req).unwrap();
        assert_eq!(
            result.output_path.unwrap(),
            output.to_str().unwrap(),
            "Output path must use .pdf extension"
        );
    }

    #[test]
    fn execute_grayscale_jpg_to_pdf_succeeds() {
        // Create a grayscale JPEG
        let img: image::GrayImage =
            image::ImageBuffer::from_fn(4, 4, |x, y| image::Luma([(x * 60 + y * 60) as u8]));
        let f = tempfile::Builder::new().suffix(".jpg").tempfile().unwrap();
        img.save(f.path()).unwrap();

        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("gray.pdf");
        let req = make_request(
            f.path().to_str().unwrap(),
            output.to_str().unwrap(),
            "pdf",
            80,
            false,
        );
        let result = execute_image_conversion(&req);
        assert!(result.is_ok(), "Grayscale JPEG to PDF should succeed");
        let content = std::fs::read(&output).unwrap();
        let text = String::from_utf8_lossy(&content);
        assert!(
            text.contains("/DeviceGray"),
            "Grayscale JPEG must use DeviceGray"
        );
        assert!(text.contains("/DCTDecode"));
    }

    #[test]
    fn execute_pdf_accepts_all_existing_raster_formats() {
        // GIF is a single frame; QOI is served by the format module registry.
        let formats = [
            ("bmp", image::ImageFormat::Bmp),
            ("tiff", image::ImageFormat::Tiff),
            ("ico", image::ImageFormat::Ico),
            ("gif", image::ImageFormat::Gif),
            ("qoi", image::ImageFormat::Qoi),
        ];
        let handler = crate::handlers::image::ImageConverterHandler;
        use crate::conversion::traits::ConverterHandler;

        for (extension, image_format) in formats {
            let dir = tempfile::tempdir().unwrap();
            let input = dir.path().join(format!("input.{}", extension));
            let output = dir.path().join("out.pdf");
            let source = image::RgbaImage::from_fn(4, 4, |x, y| {
                image::Rgba([(x * 30) as u8, (y * 20) as u8, 160, 255])
            });
            source.save_with_format(&input, image_format).unwrap();
            let request = make_request(
                input.to_str().unwrap(),
                output.to_str().unwrap(),
                "pdf",
                80,
                false,
            );
            handler.validate(&request).unwrap();
            handler.execute(&request).unwrap();
            let bytes = std::fs::read(&output).unwrap();
            let pdf = String::from_utf8_lossy(&bytes);
            assert!(pdf.starts_with("%PDF-"), "{}", extension);
            assert!(pdf.contains("/MediaBox [0 0 4 4]"), "{}", extension);
            assert!(pdf.contains("/FlateDecode"), "{}", extension);
            assert_no_temp_debris(dir.path());
        }
    }

    #[test]
    fn execute_pdf_resize_updates_media_box() {
        let input = create_test_png();
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("half.pdf");
        let mut request = make_request(
            input.path().to_str().unwrap(),
            output.to_str().unwrap(),
            "pdf",
            80,
            false,
        );
        request.options = json!({
            "quality": 80,
            "overwrite": false,
            "resize_mode": "max_width",
            "resize_width": 2
        });
        execute_image_conversion(&request).unwrap();
        let pdf = String::from_utf8_lossy(&std::fs::read(&output).unwrap()).into_owned();
        assert!(pdf.contains("/MediaBox [0 0 2 2]"), "{}", pdf);
        assert!(pdf.contains("/FlateDecode"));
    }

    #[test]
    fn execute_pdf_resized_jpeg_uses_flate_unresized_uses_dct() {
        let img: RgbImage = ImageBuffer::from_fn(8, 4, |x, y| {
            image::Rgb([(x * 20) as u8, (y * 40) as u8, 100])
        });
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("source.jpg");
        img.save(&input).unwrap();

        let original = dir.path().join("original.pdf");
        let base = make_request(
            input.to_str().unwrap(),
            original.to_str().unwrap(),
            "pdf",
            80,
            false,
        );
        execute_image_conversion(&base).unwrap();
        let bytes = std::fs::read(&original).unwrap();
        let pdf = String::from_utf8_lossy(&bytes);
        assert!(pdf.contains("/DCTDecode"));
        assert!(pdf.contains("/MediaBox [0 0 8 4]"));

        let resized = dir.path().join("resized.pdf");
        let mut request = make_request(
            input.to_str().unwrap(),
            resized.to_str().unwrap(),
            "pdf",
            80,
            false,
        );
        request.options = json!({
            "quality": 80,
            "overwrite": false,
            "resize_mode": "fit_box",
            "resize_width": 4,
            "resize_height": 4
        });
        execute_image_conversion(&request).unwrap();
        let bytes = std::fs::read(&resized).unwrap();
        let pdf = String::from_utf8_lossy(&bytes);
        assert!(pdf.contains("/FlateDecode"));
        assert!(!pdf.contains("/DCTDecode"));
        assert!(pdf.contains("/MediaBox [0 0 4 2]"));
    }

    #[test]
    fn execute_malformed_jpg_to_pdf_fails_cleanly() {
        let mut f = tempfile::Builder::new().suffix(".jpg").tempfile().unwrap();
        std::io::Write::write_all(&mut f, b"not a jpeg file at all").unwrap();
        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("bad.pdf");
        let req = make_request(
            f.path().to_str().unwrap(),
            output.to_str().unwrap(),
            "pdf",
            80,
            false,
        );
        let result = execute_image_conversion(&req);
        assert!(result.is_err(), "Malformed JPEG to PDF must fail");
        let err = result.unwrap_err().to_string().to_lowercase();
        assert!(
            err.contains("jpeg"),
            "Error should mention JPEG, got: {}",
            err
        );
    }

    /// 32x16 JPEG, red top-left 16x8 block on blue, with an EXIF APP1
    /// segment (Orientation tag) spliced in right after SOI, as phones write.
    fn create_oriented_jpg(orientation: u8) -> tempfile::NamedTempFile {
        let img: RgbImage = ImageBuffer::from_fn(32, 16, |x, y| {
            if x < 16 && y < 8 {
                image::Rgb([255, 0, 0])
            } else {
                image::Rgb([0, 0, 255])
            }
        });
        let mut jpeg = Vec::new();
        img.write_to(
            &mut std::io::Cursor::new(&mut jpeg),
            image::ImageFormat::Jpeg,
        )
        .unwrap();
        #[rustfmt::skip]
        let exif: Vec<u8> = [
            &b"Exif\0\0MM\0\x2A\0\0\0\x08"[..], // big-endian TIFF header, IFD0 at 8
            &[0, 1],                             // one entry
            &[0x01, 0x12, 0, 3, 0, 0, 0, 1, 0, orientation, 0, 0], // Orientation, SHORT
            &[0, 0, 0, 0],                       // no next IFD
        ]
        .concat();
        let len = (exif.len() + 2) as u16;
        let mut bytes = jpeg[..2].to_vec();
        bytes.extend_from_slice(&[0xFF, 0xE1]);
        bytes.extend_from_slice(&len.to_be_bytes());
        bytes.extend_from_slice(&exif);
        bytes.extend_from_slice(&jpeg[2..]);
        let f = tempfile::Builder::new().suffix(".jpg").tempfile().unwrap();
        std::fs::write(f.path(), bytes).unwrap();
        f
    }

    fn is_red(p: image::Rgba<u8>) -> bool {
        p[0] > 200 && p[2] < 60
    }

    #[test]
    fn exif_orientation_is_applied_once_for_png_and_jpg_outputs() {
        // Orientation 6: stored landscape, shown rotated 90 degrees clockwise,
        // so the stored top-left (red) ends up top-right of a 16x32 image.
        let input = create_oriented_jpg(6);
        let dims = crate::image_limits::read_image_dimensions(input.path()).unwrap();
        assert_eq!(
            (dims.width, dims.height),
            (16, 32),
            "header dims must be oriented"
        );

        let output_dir = tempfile::tempdir().unwrap();
        for format in ["png", "jpg"] {
            let output = output_dir.path().join(format!("out.{format}"));
            let req = make_request(
                input.path().to_str().unwrap(),
                output.to_str().unwrap(),
                format,
                95,
                false,
            );
            execute_image_conversion(&req).unwrap();
            // The output carries no EXIF, so a viewer shows these pixels as is.
            let mut decoder = ImageReader::open(&output).unwrap().into_decoder().unwrap();
            assert_eq!(
                image::ImageDecoder::orientation(&mut decoder).unwrap(),
                image::metadata::Orientation::NoTransforms,
                "{format}: output must not carry the source orientation"
            );
            let out = image::DynamicImage::from_decoder(decoder)
                .unwrap()
                .to_rgba8();
            assert_eq!(out.dimensions(), (16, 32), "{format}: rotated dims");
            assert!(is_red(*out.get_pixel(13, 2)), "{format}: top-right is red");
            assert!(!is_red(*out.get_pixel(2, 2)), "{format}: top-left is blue");
            assert!(
                !is_red(*out.get_pixel(13, 29)),
                "{format}: bottom-right is blue"
            );
        }
    }

    #[test]
    fn exif_orientation_8_rotates_counter_clockwise_and_resize_uses_oriented_width() {
        // Orientation 8: stored top-left ends up bottom-left.
        let input = create_oriented_jpg(8);
        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("out.png");
        let mut req = make_request(
            input.path().to_str().unwrap(),
            output.to_str().unwrap(),
            "png",
            90,
            false,
        );
        req.options = json!({ "quality": 90, "resize_mode": "max_width", "resize_width": 8 });
        execute_image_conversion(&req).unwrap();
        let out = ImageReader::open(&output)
            .unwrap()
            .decode()
            .unwrap()
            .to_rgba8();
        assert_eq!(
            out.dimensions(),
            (8, 16),
            "max_width applies to the shown width"
        );
        assert!(is_red(*out.get_pixel(1, 14)), "bottom-left is red");
        assert!(!is_red(*out.get_pixel(1, 1)), "top-left is blue");
    }

    #[test]
    fn exif_oriented_jpg_to_pdf_is_decoded_not_embedded_sideways() {
        let input = create_oriented_jpg(6);
        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("out.pdf");
        let req = make_request(
            input.path().to_str().unwrap(),
            output.to_str().unwrap(),
            "pdf",
            90,
            false,
        );
        execute_image_conversion(&req).unwrap();
        let text = String::from_utf8_lossy(&std::fs::read(&output).unwrap()).into_owned();
        assert!(
            text.contains("/MediaBox [0 0 16 32]"),
            "page must be portrait"
        );
        assert!(
            text.contains("/FlateDecode"),
            "oriented JPEG must not be embedded raw"
        );
    }
}
