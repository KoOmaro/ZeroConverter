use crate::app_error::AppError;
use crate::image_limits::{ImageDimensions, MAX_IMAGE_PIXELS};
use crate::thumbnails::image_pipeline;
use crate::thumbnails::key::ThumbnailEngine;
use std::path::Path;

/// Result from thumbnail generation: raw PNG/JPEG bytes plus dimensions.
pub struct GeneratedThumbnail {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub engine: ThumbnailEngine,
}

/// Select the appropriate thumbnail engine based on image dimensions.
pub fn select_engine(dims: &ImageDimensions) -> ThumbnailEngine {
    if dims.pixels > MAX_IMAGE_PIXELS {
        ThumbnailEngine::Oversized
    } else {
        ThumbnailEngine::RustImage
    }
}

/// Route thumbnail generation to the appropriate engine.
///
/// The caller (`cache.rs`) has already probed image dimensions and selected
/// the engine. This function executes the decision — it does NOT re-probe
/// dimensions or re-select the engine.
///
/// For `ThumbnailEngine::RustImage`, delegates to the standard Rust `image` pipeline.
/// For `ThumbnailEngine::Oversized` (over the pixel budget), returns a
/// controlled error: no preview is made for such images.
pub async fn generate(
    source_path: &Path,
    size: u32,
    engine: ThumbnailEngine,
    dims: &ImageDimensions,
) -> Result<GeneratedThumbnail, AppError> {
    match engine {
        ThumbnailEngine::RustImage => {
            let source = source_path.to_path_buf();
            let (bytes, width, height) =
                tokio::task::spawn_blocking(move || image_pipeline::process_image(&source, size))
                    .await
                    .map_err(|_| AppError::Internal("Thumbnail task panicked".into()))??;

            Ok(GeneratedThumbnail {
                bytes,
                width,
                height,
                engine: ThumbnailEngine::RustImage,
            })
        }
        ThumbnailEngine::Oversized => Err(AppError::Validate(format!(
            "Image dimensions {}x{} ({} MP) exceed the {} MP preview limit.",
            dims.width,
            dims.height,
            dims.pixels / 1_000_000,
            MAX_IMAGE_PIXELS / 1_000_000,
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image_limits::dimensions_from_pair;

    #[test]
    fn selects_rust_engine_for_small_image() {
        let dims = dimensions_from_pair(1920, 1080);
        assert_eq!(select_engine(&dims), ThumbnailEngine::RustImage);
    }

    #[test]
    fn selects_rust_engine_at_exact_limit() {
        let dims = dimensions_from_pair(10_000, 10_000);
        assert_eq!(select_engine(&dims), ThumbnailEngine::RustImage);
    }

    #[test]
    fn selects_oversized_over_limit() {
        let dims = dimensions_from_pair(10_001, 10_000);
        assert_eq!(select_engine(&dims), ThumbnailEngine::Oversized);
    }

    #[test]
    fn selects_oversized_for_huge_image() {
        let dims = dimensions_from_pair(20_000, 20_000);
        assert_eq!(select_engine(&dims), ThumbnailEngine::Oversized);
    }
}
