use crate::api::dto::{ThumbnailRequestDto, ThumbnailResultDto};
use crate::core::app_error::AppError;
use crate::core::thumbnails::get_or_create_thumbnail as core_get_or_create_thumbnail;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[tauri::command]
pub async fn get_or_create_thumbnail(
    app: AppHandle,
    request: ThumbnailRequestDto,
) -> Result<ThumbnailResultDto, AppError> {
    let cache_dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| AppError::Io(format!("Failed to resolve app_cache_dir: {}", e)))?;

    let source_path = PathBuf::from(&request.path);

    let result = core_get_or_create_thumbnail(
        &cache_dir,
        &source_path,
        request.size,
        request.cache_key_hint.as_deref(),
    )
    .await?;

    Ok(ThumbnailResultDto {
        cached_path: result.cached_path,
        width: result.width,
        height: result.height,
        cache_hit: result.cache_hit,
    })
}
