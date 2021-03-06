#[macro_use]
extern crate serde_derive;
extern crate http_video_streaming_types;

use http_video_streaming_types::Shape;

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub enum ContrastPolarity {
    DetectLight,
    DetectDark,
    DetectAbsDiff,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImPtDetectCfg {
    /// Switch whether to continuously update the background model or not.
    pub do_update_background_model: bool,
    /// What kind of difference from the background model to detect.
    pub polarity: ContrastPolarity,
    /// How much to weight the update of the background model.
    ///
    /// Valid range is 0.0 - 1.0. 0.0 means never update, 1.0 means complete
    /// replacement on every update.
    pub alpha: f32,
    /// Number of standard deviations a pixel must differ by to be detected.
    ///
    /// Used when `use_cmp` is true. No effect when `use_cmp` is false. Valid
    /// range is 0.0 - infinity. 0.0 means any difference is detected, large
    /// value means only large deviations from mean are detected.
    pub n_sigma: f32,
    /// (used when `use_cmp` is true)
    pub bright_non_gaussian_cutoff: u8,
    /// (used when `use_cmp` is true)
    pub bright_non_gaussian_replacement: u8,
    /// How often to update the background model, in number of frames.
    ///
    /// Valid range is 0-4294967295.
    pub bg_update_interval: u32,
    /// If `use_cmp` is true, this is the absolute difference required to detect
    /// a point.
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
    pub valid_region: Shape,
}
