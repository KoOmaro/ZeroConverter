//! Conservative admission estimates for batches of CPU- and memory-heavy conversions.
//!
//! This is a *concurrency* budget, not a process RSS limit. The working-set
//! multiplier deliberately avoids promising byte-accurate codec accounting.

use crate::conversion::types::{AssetFamily, ConversionRequest};

const MIB: u64 = 1024 * 1024;
const DEFAULT_MEMORY_MIB: u32 = 1024;
const MIN_MEMORY_MIB: u32 = 256;
const MAX_MEMORY_MIB: u32 = 8192;
const JOB_OVERHEAD_MIB: u64 = 32;

/// The optional budget is clamped so an invalid / malicious environment value
/// cannot create an unbounded semaphore or silently disable admission.
pub(super) fn configured_memory_mib() -> u32 {
    std::env::var("ZERO_CONVERTER_MEMORY_MIB")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .map(|value| value.clamp(MIN_MEMORY_MIB, MAX_MEMORY_MIB))
        .unwrap_or(DEFAULT_MEMORY_MIB)
}

fn permits_for_bytes(estimated_bytes: u64, budget_mib: u32) -> u32 {
    let required_mib = estimated_bytes.saturating_add(MIB - 1) / MIB;
    required_mib
        .max(JOB_OVERHEAD_MIB)
        .min(u64::from(budget_mib)) as u32
}

/// Allow for the decoded surface, resizing / encoder buffers, plus input bytes.
/// This is intentionally a coarse admission heuristic, not a hard memory bound.
fn image_permits(pixels: u64, file_bytes: u64, budget_mib: u32) -> u32 {
    let decoded_rgba = pixels.saturating_mul(4);
    let working_set = decoded_rgba
        .saturating_mul(4)
        .saturating_add(file_bytes.saturating_mul(2))
        .saturating_add(JOB_OVERHEAD_MIB * MIB);
    permits_for_bytes(working_set, budget_mib)
}

/// Returns a bounded weighted semaphore cost. Unreadable headers and unfamiliar
/// families consume the full budget; validation remains the pipeline's job.
pub(super) fn estimate_permits(request: &ConversionRequest, budget_mib: u32) -> u32 {
    let input = std::path::Path::new(&request.input_path);
    let Ok(metadata) = std::fs::metadata(input) else {
        return budget_mib;
    };
    if !metadata.is_file() {
        return budget_mib;
    }

    match &request.family {
        AssetFamily::Image => {
            let Ok(dims) = crate::image_limits::read_image_dimensions(input) else {
                return budget_mib;
            };
            if dims.pixels == 0 || dims.pixels > crate::image_limits::MAX_IMAGE_PIXELS {
                return budget_mib;
            }
            image_permits(dims.pixels, metadata.len(), budget_mib)
        }
        AssetFamily::Audio => {
            // Compressed audio may expand substantially; the conversion handlers
            // still enforce their own decoding constraints.
            permits_for_bytes(
                metadata
                    .len()
                    .saturating_mul(8)
                    .saturating_add(JOB_OVERHEAD_MIB * MIB),
                budget_mib,
            )
        }
        _ => budget_mib,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiny_image_has_minimum_admission_cost() {
        assert_eq!(image_permits(64, 1024, 1024), 33);
    }

    #[test]
    fn large_image_takes_entire_default_budget() {
        assert_eq!(image_permits(100_000_000, 4096, 1024), 1024);
    }

    #[test]
    fn overflow_is_saturating_and_bounded() {
        assert_eq!(image_permits(u64::MAX, u64::MAX, 1024), 1024);
        assert_eq!(permits_for_bytes(u64::MAX, 256), 256);
    }

    #[test]
    fn unreadable_input_uses_entire_budget() {
        let request = ConversionRequest {
            family: AssetFamily::Image,
            input_path: "/zero-converter-missing-file-for-memory-admission.png".to_owned(),
            output_path: String::new(),
            output_root: String::new(),
            output_format: crate::conversion::types::OutputFormat("png".to_owned()),
            options: serde_json::json!({}),
        };
        assert_eq!(estimate_permits(&request, 256), 256);
    }

    #[test]
    fn admission_weight_is_bounded_by_budget() {
        for pixels in [1, 1_000_000, 100_000_000, u64::MAX] {
            let permits = image_permits(pixels, 10_000, 256);
            assert!((1..=256).contains(&permits));
        }
    }
}
