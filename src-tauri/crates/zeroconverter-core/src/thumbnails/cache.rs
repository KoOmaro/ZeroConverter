use crate::app_error::AppError;
use crate::image_limits::read_image_dimensions;
use crate::thumbnails::key::{cache_dir_version, generate_cache_key, ThumbnailEngine};
use crate::thumbnails::router;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::Semaphore;
use zero_fs::SyncPolicy;

fn global_semaphore() -> Arc<Semaphore> {
    static SEMAPHORE: OnceLock<Arc<Semaphore>> = OnceLock::new();
    SEMAPHORE
        .get_or_init(|| Arc::new(Semaphore::new(4)))
        .clone()
}

fn in_flight_map() -> &'static Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>> {
    static MAP: OnceLock<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Debug)]
pub struct ThumbnailResult {
    pub cached_path: String,
    pub width: u32,
    pub height: u32,
    pub cache_hit: bool,
}

pub async fn get_or_create_thumbnail(
    cache_root: &Path,
    source_path: &Path,
    size: u32,
    cache_key_hint: Option<&str>,
) -> Result<ThumbnailResult, AppError> {
    if !source_path.is_file() {
        return Err(AppError::Io("Source path is not a valid file".into()));
    }

    // ── Probe dimensions to determine engine ──────────────────────────
    let dims = read_image_dimensions(source_path)?;
    let engine = router::select_engine(&dims);

    // ── Generate engine-aware cache key ───────────────────────────────
    let cache_key = generate_cache_key(source_path, size, engine, cache_key_hint)?;

    let extension = match engine {
        ThumbnailEngine::RustImage => "png",
        ThumbnailEngine::Oversized => "jpg",
    };
    let file_name = format!("{}_{}.{}", cache_key, size, extension);

    // Use v2 cache namespace
    let thumbnail_dir = cache_root.join("thumbnails").join(cache_dir_version());

    // Ensure the cache directory exists before validation, otherwise zero_fs::ensure_within_root
    // will fail to canonicalize the non-existent root path.
    std::fs::create_dir_all(&thumbnail_dir)
        .map_err(|e| AppError::Io(format!("Failed to create thumbnail dir: {}", e)))?;

    let target_path = thumbnail_dir.join(&file_name);

    zero_fs::ensure_within_root(cache_root, &target_path)
        .map_err(|e| AppError::Io(format!("Thumbnail path escaped cache root: {}", e)))?;

    // First check (fast path)
    if target_path.exists() {
        return read_cached_dimensions(&target_path, true);
    }

    // Acquire or create the lock for this specific cache key
    let key_lock = {
        let in_flight = in_flight_map();
        let mut map = in_flight.lock().unwrap();
        map.entry(cache_key.clone())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    };

    let _guard = key_lock.lock().await;

    // Double check (in case another thread generated it while we were waiting on key_lock)
    if target_path.exists() {
        cleanup_in_flight(&cache_key);
        return read_cached_dimensions(&target_path, true);
    }

    let semaphore = global_semaphore();
    let _permit = semaphore.acquire().await.ok();

    // ── Delegate to router ────────────────────────────────────────────
    let result = router::generate(source_path, size, engine, &dims).await?;

    zero_fs::write_atomic(&target_path, &result.bytes, SyncPolicy::Disposable)
        .map_err(|e| AppError::Io(format!("Failed to write thumbnail: {}", e)))?;

    // Cleanup in_flight map
    cleanup_in_flight(&cache_key);

    Ok(ThumbnailResult {
        cached_path: target_path.to_string_lossy().to_string(),
        width: result.width,
        height: result.height,
        cache_hit: false,
    })
}

fn cleanup_in_flight(key: &str) {
    let in_flight = in_flight_map();
    let mut map = in_flight.lock().unwrap();
    map.remove(key);
}

fn read_cached_dimensions(path: &Path, cache_hit: bool) -> Result<ThumbnailResult, AppError> {
    let (width, height) = image::image_dimensions(path)
        .map_err(|e| AppError::Io(format!("Failed to read cached image dimensions: {}", e)))?;

    Ok(ThumbnailResult {
        cached_path: path.to_string_lossy().to_string(),
        width,
        height,
        cache_hit,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[tokio::test]
    async fn directory_is_rejected_as_source() {
        let dir = tempdir().unwrap();
        let cache_root = dir.path().join("cache");
        std::fs::create_dir_all(&cache_root).unwrap();

        let result = get_or_create_thumbnail(&cache_root, dir.path(), 256, None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn second_request_is_a_cache_hit() {
        let dir = tempdir().unwrap();
        let cache_root = dir.path().join("cache");
        let img_path = dir.path().join("pic.png");
        image::RgbImage::new(64, 40).save(&img_path).unwrap();

        let first = get_or_create_thumbnail(&cache_root, &img_path, 32, None)
            .await
            .unwrap();
        let second = get_or_create_thumbnail(&cache_root, &img_path, 32, None)
            .await
            .unwrap();
        assert!(!first.cache_hit);
        assert!(second.cache_hit);
        assert_eq!(first.cached_path, second.cached_path);
    }

    #[test]
    fn test_read_cached_dimensions() {
        let dir = tempdir().unwrap();
        let img_path = dir.path().join("test.png");
        let img = image::RgbImage::new(50, 30);
        img.save(&img_path).unwrap();

        let result = read_cached_dimensions(&img_path, true).unwrap();
        assert_eq!(result.width, 50);
        assert_eq!(result.height, 30);
        assert!(result.cache_hit);
    }

    #[test]
    fn test_corrupt_cached_file_returns_error() {
        let dir = tempdir().unwrap();
        let corrupt_path = dir.path().join("bad.png");
        let mut f = File::create(&corrupt_path).unwrap();
        f.write_all(b"not an image").unwrap();

        let result = read_cached_dimensions(&corrupt_path, false);
        assert!(result.is_err());
    }
}
