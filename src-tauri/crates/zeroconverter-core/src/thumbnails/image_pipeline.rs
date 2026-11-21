use crate::app_error::AppError;
use std::io::Cursor;
use std::path::Path;

pub fn process_image(
    source_path: &Path,
    requested_size: u32,
) -> Result<(Vec<u8>, u32, u32), AppError> {
    // Reject oversized images before full pixel decode
    crate::image_limits::validate_image_dimensions(source_path)?;

    let img = image::open(source_path)
        .map_err(|e| AppError::Execute(format!("Failed to open image for thumbnail: {}", e)))?;

    let resized = img.thumbnail(requested_size, requested_size);
    let width = resized.width();
    let height = resized.height();

    let mut buffer = Cursor::new(Vec::new());
    resized
        .write_to(&mut buffer, image::ImageFormat::Png)
        .map_err(|e| AppError::Execute(format!("Failed to encode PNG thumbnail: {}", e)))?;

    Ok((buffer.into_inner(), width, height))
}
