use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ResizeMode {
    #[default]
    None,
    MaxWidth,
    MaxHeight,
    FitBox,
}

/// Quality when a request does not set one; the app and the CLI share it.
pub const DEFAULT_QUALITY: u8 = 90;

fn default_quality() -> u8 {
    DEFAULT_QUALITY
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageOptions {
    #[serde(default = "default_quality")]
    pub quality: u8,
    #[serde(default)]
    pub overwrite: bool,
    #[serde(default)]
    pub resize_mode: ResizeMode,
    pub resize_width: Option<u32>,
    pub resize_height: Option<u32>,
    /// DDS block compression: `bc1`..`bc7`. Defaults to BC7.
    #[serde(default)]
    pub dds_format: Option<String>,
    /// DDS only: write the full mip chain.
    #[serde(default)]
    pub dds_mipmaps: bool,
}

impl Default for ImageOptions {
    fn default() -> Self {
        Self {
            quality: DEFAULT_QUALITY,
            overwrite: false,
            resize_mode: ResizeMode::None,
            resize_width: None,
            resize_height: None,
            dds_format: None,
            dds_mipmaps: false,
        }
    }
}

pub fn compute_resize_target(
    orig_w: u32,
    orig_h: u32,
    options: &ImageOptions,
) -> Option<(u32, u32)> {
    if orig_w == 0 || orig_h == 0 {
        return None;
    }

    match options.resize_mode {
        ResizeMode::None => None,
        ResizeMode::MaxWidth => {
            let bound_w = options.resize_width?;
            if orig_w <= bound_w {
                return None;
            }
            let target_w = bound_w;
            let target_h =
                std::cmp::max(1, ((orig_h as u64 * bound_w as u64) / orig_w as u64) as u32);
            Some((target_w, target_h))
        }
        ResizeMode::MaxHeight => {
            let bound_h = options.resize_height?;
            if orig_h <= bound_h {
                return None;
            }
            let target_h = bound_h;
            let target_w =
                std::cmp::max(1, ((orig_w as u64 * bound_h as u64) / orig_h as u64) as u32);
            Some((target_w, target_h))
        }
        ResizeMode::FitBox => {
            let bound_w = options.resize_width?;
            let bound_h = options.resize_height?;

            if orig_w <= bound_w && orig_h <= bound_h {
                return None;
            }

            let scale_w = (orig_w as u64 * bound_h as u64) / orig_h as u64; // width if height = bound_h
            let scale_h = (orig_h as u64 * bound_w as u64) / orig_w as u64; // height if width = bound_w

            let (target_w, target_h) = if scale_w <= bound_w as u64 {
                (std::cmp::max(1, scale_w as u32), bound_h)
            } else {
                (bound_w, std::cmp::max(1, scale_h as u32))
            };

            Some((target_w, target_h))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_opts(mode: ResizeMode, w: Option<u32>, h: Option<u32>) -> ImageOptions {
        ImageOptions {
            resize_mode: mode,
            resize_width: w,
            resize_height: h,
            ..Default::default()
        }
    }

    #[test]
    fn test_none_returns_none() {
        let opts = make_opts(ResizeMode::None, Some(100), Some(100));
        assert_eq!(compute_resize_target(500, 500, &opts), None);
    }

    #[test]
    fn test_zero_original_dimensions_do_not_panic() {
        let opts = make_opts(ResizeMode::FitBox, Some(100), Some(100));
        assert_eq!(compute_resize_target(0, 500, &opts), None);
        assert_eq!(compute_resize_target(500, 0, &opts), None);
        assert_eq!(compute_resize_target(0, 0, &opts), None);
    }

    #[test]
    fn test_max_width_downscales_expected_dimensions() {
        let opts = make_opts(ResizeMode::MaxWidth, Some(800), None);
        // 1600x1200 -> 800x600
        assert_eq!(compute_resize_target(1600, 1200, &opts), Some((800, 600)));
    }

    #[test]
    fn test_max_width_does_not_upscale() {
        let opts = make_opts(ResizeMode::MaxWidth, Some(800), None);
        // 400x300 -> stays 400x300
        assert_eq!(compute_resize_target(400, 300, &opts), None);
        // 800x600 -> stays 800x600
        assert_eq!(compute_resize_target(800, 600, &opts), None);
    }

    #[test]
    fn test_max_height_downscales_expected_dimensions() {
        let opts = make_opts(ResizeMode::MaxHeight, None, Some(600));
        // 1600x1200 -> 800x600
        assert_eq!(compute_resize_target(1600, 1200, &opts), Some((800, 600)));
    }

    #[test]
    fn test_max_height_does_not_upscale() {
        let opts = make_opts(ResizeMode::MaxHeight, None, Some(600));
        // 400x300 -> stays 400x300
        assert_eq!(compute_resize_target(400, 300, &opts), None);
    }

    #[test]
    fn test_fit_box_fits_within_both_bounds() {
        let opts = make_opts(ResizeMode::FitBox, Some(100), Some(100));
        // Landscape 200x50 -> 100x25
        assert_eq!(compute_resize_target(200, 50, &opts), Some((100, 25)));
        // Portrait 50x200 -> 25x100
        assert_eq!(compute_resize_target(50, 200, &opts), Some((25, 100)));
        // Square 200x200 -> 100x100
        assert_eq!(compute_resize_target(200, 200, &opts), Some((100, 100)));
    }

    #[test]
    fn test_tiny_outputs_clamp_to_at_least_1() {
        let opts = make_opts(ResizeMode::MaxWidth, Some(10), None);
        // 1000x1 -> 10x0.01 -> clamps to 10x1
        assert_eq!(compute_resize_target(1000, 1, &opts), Some((10, 1)));

        let opts2 = make_opts(ResizeMode::MaxHeight, None, Some(10));
        // 1x1000 -> 0.01x10 -> clamps to 1x10
        assert_eq!(compute_resize_target(1, 1000, &opts2), Some((1, 10)));
    }

    #[test]
    fn test_large_dimensions_do_not_overflow() {
        let opts = make_opts(ResizeMode::FitBox, Some(100), Some(100));
        // 100000x100000 (huge) -> 100x100
        assert_eq!(
            compute_resize_target(100000, 100000, &opts),
            Some((100, 100))
        );
    }
}
