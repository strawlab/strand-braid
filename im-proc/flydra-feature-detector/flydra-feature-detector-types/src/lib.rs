// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Configuration types for [Braid](https://strawlab.org/braid) and Flydra feature detection.
//!
//! This crate provides types for configuring 2D feature detection parameters as
//! originally used in the Flydra tracking system. Now these types are used in
//! Strand Camera and Braid.

use serde::{Deserialize, Serialize};

use strand_http_video_streaming_types::Shape;

/// Polarity of contrast for feature detection.
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub enum ContrastPolarity {
    /// Detect light features on dark background
    DetectLight,
    /// Detect dark features on light background
    DetectDark,
    /// Detect features based on absolute difference
    DetectAbsDiff,
}

/// Configuration parameters for feature detection.
///
/// These parameters are used in the 2D feature detection step. As such, they
/// are used to parameterize how incoming images are analyzed so that relevant
/// features are extracted and sent onwards for consideration as candidates for
/// 3D tracking.
///
/// # Algorithm overview
///
/// Detection operates on luminance (`Mono8`) images. Color input is converted
/// before any processing: RGB is converted to luma with the BT.601 full-swing
/// weights (Y ≈ 0.299 R + 0.587 G + 0.114 B, computed on the gamma-encoded
/// 8-bit values, i.e. without sRGB linearization), and raw Bayer images are
/// first demosaiced to RGB and then converted to luma the same way.
///
/// The background model consists of a per-pixel running mean and running mean
/// of squares, both stored as `f32`. At startup — and whenever a new background
/// image is requested (the "Take Current Image As Background" button in the
/// Strand Camera browser UI) — the model is seeded from the current frame and
/// accumulated over the next 20 frames; during this startup period no features
/// are detected. This initialization happens regardless of
/// `do_update_background_model`, which only controls whether the model
/// continues to be updated afterwards (every `bg_update_interval` frames, with
/// weight `alpha`).
///
/// On every frame, a per-pixel difference image is computed between the
/// current image and the background mean according to `polarity`. A pixel is
/// detected when this difference exceeds a per-pixel threshold: when `use_cmp`
/// is false, the threshold is simply `diff_threshold`; when `use_cmp` is true,
/// the threshold is `n_sigma` times the per-pixel running standard deviation,
/// but never less than `diff_threshold` (see `n_sigma` for details). Around
/// each detected maximum, a window of `feature_window_size` is analyzed with
/// image moments to extract the sub-pixel center of mass, area, and
/// orientation of the feature.
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImPtDetectCfg {
    /// Switch whether to continuously update the background model or not.
    ///
    /// Even when this is false, the background model is initialized at startup
    /// by accumulating the first 20 frames, and can be manually reset (see the
    /// "Algorithm overview" above). This switch only controls the ongoing
    /// updates performed every `bg_update_interval` frames.
    pub do_update_background_model: bool,
    /// What kind of difference from the background model to detect.
    #[serde(with = "serde_yaml::with::singleton_map")]
    pub polarity: ContrastPolarity,
    /// How much to weight the update of the background model.
    ///
    /// Valid range is 0.0 - 1.0. 0.0 means never update, 1.0 means complete
    /// replacement on every update. The update is
    /// `mean = (1 - alpha) * mean + alpha * current` and is applied once every
    /// `bg_update_interval` frames (not every frame). The same weighting is
    /// applied to the running mean of squared pixel values, from which the
    /// per-pixel standard deviation used with `n_sigma` is derived.
    ///
    /// The model is stored as `f32` (24-bit mantissa), so extremely small
    /// values of alpha can stall the update: a single update changes a pixel
    /// by `alpha * (current - mean)`, and when this increment is smaller than
    /// about `mean * 1e-7` it is lost to rounding. In practice values down to
    /// roughly 1e-4 behave well; for slower adaptation, prefer increasing
    /// `bg_update_interval` over shrinking alpha further.
    pub alpha: f32,
    /// Number of standard deviations a pixel must differ by to be detected.
    ///
    /// Used when `use_cmp` is true. No effect when `use_cmp` is false. Valid
    /// range is 0.0 - infinity. 0.0 means any difference is detected, large
    /// value means only large deviations from mean are detected.
    ///
    /// The standard deviation is computed per pixel from the running
    /// (exponentially weighted, see `alpha`) mean and mean of squares as
    /// `sqrt(|E[x²] - E[x]²|)`. The resulting per-pixel threshold
    /// `n_sigma * std` is rounded to 8 bits and clipped below at
    /// `diff_threshold`, so `diff_threshold` acts as a floor on the detection
    /// threshold even when `use_cmp` is true.
    pub n_sigma: f32,
    /// Bright pixels are not well modeled as Gaussian. Per-pixel thresholds of
    /// pixels whose background mean exceeds this value are replaced with
    /// `bright_non_gaussian_replacement`. (Used when `use_cmp` is true.)
    pub bright_non_gaussian_cutoff: u8,
    /// The per-pixel threshold used in place of `n_sigma * std` for pixels
    /// brighter than `bright_non_gaussian_cutoff`. (Used when `use_cmp` is
    /// true.)
    pub bright_non_gaussian_replacement: u8,
    /// How often to update the background model, in number of frames.
    ///
    /// Valid range is 0-4294967295.
    pub bg_update_interval: u32,
    /// This is the absolute difference required to detect a point. (For both
    /// `use_cmp` true and false.)
    ///
    /// When `use_cmp` is true, this acts as a lower bound on the per-pixel
    /// `n_sigma`-based threshold. Note that the per-pixel thresholds are
    /// stored with this lower bound already applied, so *lowering* this value
    /// at runtime only takes full effect when the per-pixel thresholds are
    /// next recomputed: at the next background model update, or when the
    /// background model is reset. (Raising it takes effect immediately.)
    pub diff_threshold: u8,
    /// If `use_cmp` is true, use n_sigma based difference.
    pub use_cmp: bool,
    /// How many points above threshold can be detected.
    pub max_num_points: u16,
    /// Half the width (and half the height) of the analysis region. In pixels.
    pub feature_window_size: u16, // previously `roi2_radius`
    /// Reduces moment arm when detecting pixels.
    ///
    /// The result of this computation or `despecked_threshold` is used,
    /// whichever is larger. Fraction of the maximum difference value in pixel
    /// intensity. Valid range is 0.0 - 1.0. 0.0 means the value is 0, 1.0 means
    /// the value used is the maximum difference in pixel intensity between the
    /// current image and the mean of the background model.
    pub clear_fraction: f32,
    /// Reduces moment arm when detecting pixels.
    ///
    /// This value or the result of the `clear_fraction` computation is used,
    /// whichever is larger. Intensity difference value. Value range is 0-255.
    pub despeckle_threshold: u8,
    /// The shape of the reason over which detected points are checked.
    #[serde(with = "serde_yaml::with::singleton_map")]
    pub valid_region: Shape,
}
