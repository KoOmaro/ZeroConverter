use crate::app_error::AppError;
use image::ImageReader;
use std::path::Path;

/// Maximum allowed image pixel count before full decode.
/// 100 MP ≈ 400 MB decoded RGBA8 worst-case baseline.
pub const MAX_IMAGE_PIXELS: u64 = 100_000_000;

/// Result of a successful dimension check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageDimensions {
    pub width: u32,
    pub height: u32,
    pub pixels: u64,
    pub estimated_rgba8_bytes: u64,
}

/// Evaluates whether a (width, height) pair exceeds the memory safety limit.
/// Returns `Ok(())` if within bounds, or `Err(AppError::Validate)` if over.
///
/// This is the pure policy check, separated from I/O for testability.
pub fn check_pixel_budget(dims: &ImageDimensions) -> Result<(), AppError> {
    if dims.pixels > MAX_IMAGE_PIXELS {
        let mp = dims.pixels / 1_000_000;
        let max_mp = MAX_IMAGE_PIXELS / 1_000_000;
        return Err(AppError::Validate(format!(
            "Image dimensions {}x{} ({} MP) exceed safe limit of {} MP.",
            dims.width, dims.height, mp, max_mp
        )));
    }
    Ok(())
}

/// Build an `ImageDimensions` from a raw width/height pair.
/// Does NOT enforce the pixel budget.
pub fn dimensions_from_pair(width: u32, height: u32) -> ImageDimensions {
    let pixels = (width as u64).saturating_mul(height as u64);
    let estimated_rgba8_bytes = pixels.saturating_mul(4);
    ImageDimensions {
        width,
        height,
        pixels,
        estimated_rgba8_bytes,
    }
}

/// Reads image dimensions from the file header without decoding pixel data.
/// Does NOT enforce the pixel budget — huge images will succeed here.
/// Returns an error only if the file cannot be opened or dimensions cannot be read.
pub fn read_image_dimensions(path: &Path) -> Result<ImageDimensions, AppError> {
    // Format modules read their own headers; the image crate handles the rest.
    if let Some(module) = crate::handlers::image::formats::sniff_file(path) {
        let (width, height) = (module.dimensions)(path)?;
        return Ok(dimensions_from_pair(width, height));
    }

    let reader = ImageReader::open(path)
        .map_err(|e| AppError::Io(format!("Failed to open image for bounds check: {}", e)))?;

    let (width, height) = reader
        .into_dimensions()
        .map_err(|e| AppError::Probe(format!("Failed to read image dimensions: {}", e)))?;

    Ok(dimensions_from_pair(width, height))
}

/// Reads image dimensions from the file header without decoding pixel data.
/// Returns an error if the file cannot be opened, dimensions cannot be read,
/// or the pixel count exceeds `MAX_IMAGE_PIXELS`.
pub fn validate_image_dimensions(path: &Path) -> Result<ImageDimensions, AppError> {
    let dims = read_image_dimensions(path)?;
    check_pixel_budget(&dims)?;
    Ok(dims)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, RgbImage};
    use std::io::Write;

    /// Create a real 4x4 PNG image in a temp file.
    fn create_small_png() -> tempfile::NamedTempFile {
        let img: RgbImage = ImageBuffer::from_fn(4, 4, |x, y| {
            image::Rgb([(x * 64) as u8, (y * 64) as u8, 128])
        });
        let f = tempfile::Builder::new().suffix(".png").tempfile().unwrap();
        img.save(f.path()).unwrap();
        f
    }

    // ═══════════════════════════════════════════
    // validate_image_dimensions: I/O integration
    // ═══════════════════════════════════════════

    #[test]
    fn small_image_accepted() {
        let input = create_small_png();
        let result = validate_image_dimensions(input.path());
        assert!(result.is_ok(), "Small image should pass: {:?}", result);
        let dims = result.unwrap();
        assert_eq!(dims.width, 4);
        assert_eq!(dims.height, 4);
        assert_eq!(dims.pixels, 16);
        assert_eq!(dims.estimated_rgba8_bytes, 64);
    }

    #[test]
    fn nonexistent_file_returns_io_error() {
        let result = validate_image_dimensions(Path::new("/nonexistent/phantom.png"));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("bounds check"),
            "Expected bounds check error, got: {}",
            err
        );
    }

    #[test]
    fn corrupt_file_returns_probe_error() {
        let mut f = tempfile::Builder::new().suffix(".png").tempfile().unwrap();
        f.write_all(b"this is not an image").unwrap();
        let result = validate_image_dimensions(f.path());
        assert!(result.is_err());
    }

    // ═══════════════════════════════════════════
    // check_pixel_budget: Pure policy tests
    //
    // These test the rejection logic without I/O,
    // avoiding the need to allocate huge pixel buffers
    // or craft fake image headers.
    // ═══════════════════════════════════════════

    // ═══════════════════════════════════════════
    // read_image_dimensions: header-only probe
    // ═══════════════════════════════════════════

    #[test]
    fn read_dimensions_works_on_small_image() {
        let input = create_small_png();
        let dims = read_image_dimensions(input.path()).unwrap();
        assert_eq!(dims.width, 4);
        assert_eq!(dims.height, 4);
        assert_eq!(dims.pixels, 16);
    }

    #[test]
    fn read_dimensions_does_not_enforce_budget() {
        // Synthetic check: dimensions_from_pair for huge values succeeds,
        // proving read_image_dimensions would not reject huge images.
        let dims = dimensions_from_pair(20_000, 20_000);
        assert_eq!(dims.pixels, 400_000_000);
        // check_pixel_budget would reject, but dimensions_from_pair does not.
        assert!(check_pixel_budget(&dims).is_err());
    }

    // ═══════════════════════════════════════════
    // check_pixel_budget: Pure policy tests
    // ═══════════════════════════════════════════

    #[test]
    fn budget_accepts_small_image() {
        let dims = dimensions_from_pair(100, 100);
        assert!(check_pixel_budget(&dims).is_ok());
        assert_eq!(dims.pixels, 10_000);
        assert_eq!(dims.estimated_rgba8_bytes, 40_000);
    }

    #[test]
    fn budget_accepts_exactly_at_limit() {
        // 10000 x 10000 = 100_000_000 == MAX_IMAGE_PIXELS
        let dims = dimensions_from_pair(10_000, 10_000);
        assert!(
            check_pixel_budget(&dims).is_ok(),
            "Exactly at limit should pass"
        );
    }

    #[test]
    fn budget_rejects_one_pixel_over_limit() {
        // 10001 x 10000 = 100_010_000 > MAX_IMAGE_PIXELS
        let dims = dimensions_from_pair(10_001, 10_000);
        assert!(
            check_pixel_budget(&dims).is_err(),
            "One pixel over limit should fail"
        );
        let err = check_pixel_budget(&dims).unwrap_err().to_string();
        assert!(err.contains("exceed safe limit"));
    }

    #[test]
    fn budget_rejects_huge_dimensions() {
        // 20000 x 20000 = 400_000_000 pixels
        let dims = dimensions_from_pair(20_000, 20_000);
        assert!(
            check_pixel_budget(&dims).is_err(),
            "Huge dimensions should fail"
        );
        let err = check_pixel_budget(&dims).unwrap_err().to_string();
        assert!(
            err.contains("20000x20000"),
            "Expected dimensions in error: {}",
            err
        );
        assert!(err.contains("400 MP"), "Expected MP in error: {}", err);
        assert!(err.contains("100 MP"), "Expected max MP in error: {}", err);
    }

    #[test]
    fn budget_rejects_extreme_single_axis() {
        // 1 x 200_000_000 = 200 MP
        let dims = dimensions_from_pair(1, 200_000_000);
        assert!(check_pixel_budget(&dims).is_err());
    }

    #[test]
    fn budget_handles_u32_max_overflow_safely() {
        // u32::MAX x u32::MAX would overflow u64::MAX via checked_mul
        let dims = dimensions_from_pair(u32::MAX, u32::MAX);
        assert!(
            check_pixel_budget(&dims).is_err(),
            "u32::MAX overflow should be rejected"
        );
    }

    #[test]
    fn budget_zero_dimensions_accepted() {
        let dims = dimensions_from_pair(0, 0);
        assert!(check_pixel_budget(&dims).is_ok());
        assert_eq!(dims.pixels, 0);
    }

    #[test]
    fn budget_returns_validate_error_variant() {
        let dims = dimensions_from_pair(20_000, 20_000);
        assert!(check_pixel_budget(&dims).is_err());
        match check_pixel_budget(&dims).unwrap_err() {
            AppError::Validate(_) => {} // correct
            other => panic!("Expected AppError::Validate, got: {:?}", other),
        }
    }

    #[test]
    fn estimated_bytes_correct_for_known_dimensions() {
        let dims = dimensions_from_pair(1920, 1080);
        assert_eq!(dims.pixels, 1920 * 1080);
        assert_eq!(dims.estimated_rgba8_bytes, 1920 * 1080 * 4);
    }
}
