use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputConfig {
    /// The type of output desired. Currently only type "mkv" is supported.
    #[serde(rename = "type")]
    pub type_: String,
    /// The filename of the output desired.
    pub filename: String,
    /// If the output type is "mkv", the options for the emitted MKV file.
    pub video_options: Option<OutputVideoConfig>,
}

impl Validate for OutputConfig {
    fn validate(&mut self) -> Result<()> {
        if !VALID_OUTPUT_TYPES.contains(&self.type_.as_str()) {
            anyhow::bail!(
                "Output type \"{}\" not one of: {:?}",
                self.type_,
                VALID_OUTPUT_TYPES
            )
        }
        if let Some(opts) = self.video_options.as_mut() {
            opts.validate()?;
        }
        Ok(())
    }
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            type_: "video".to_string(),
            filename: "output.mkv".to_string(),
            video_options: Some(OutputVideoConfig::default()),
        }
    }
}

const VALID_OUTPUT_TYPES: &[&str] = &["video", "debug_txt"];

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputVideoConfig {
    /// The space surrounding each image in the composite view.
    pub composite_margin_pixels: usize,
    pub time_dilation_factor: Option<f32>,
    /// The size of the point to overlay when drawing braidz 2D features.
    pub feature_size_pixels: Option<u16>,
    /// The title of the saved video, set in the segment metadata.
    pub title: Option<String>,
}

impl Default for OutputVideoConfig {
    fn default() -> Self {
        Self {
            composite_margin_pixels: 5,
            time_dilation_factor: None,
            feature_size_pixels: None,
            title: None,
        }
    }
}

impl Validate for OutputVideoConfig {
    fn validate(&mut self) -> Result<()> {
        if self.time_dilation_factor == Some(1.0) {
            self.time_dilation_factor = None;
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BraidRetrackVideoConfig {
    /// Specifies the maximum duration between frames to count as "synchronous",
    /// defaults to half of `frame_duration_microsecs`.
    pub sync_threshold_microseconds: Option<u64>,
    /// The interval between adjacent frames. Defaults to the value detected in
    /// the first frames of the given video inputs.
    pub frame_duration_microsecs: Option<u64>,
    /// maximum number of frames to render
    pub max_num_frames: Option<usize>,
    pub input_braidz: Option<String>,
    pub output: Vec<OutputConfig>,
    pub input_video: Vec<VideoSourceConfig>,
}

impl Default for BraidRetrackVideoConfig {
    fn default() -> Self {
        Self {
            sync_threshold_microseconds: None,
            frame_duration_microsecs: None,
            max_num_frames: None,
            input_braidz: None,
            output: vec![OutputConfig::default()],
            input_video: vec![
                VideoSourceConfig::new("a.mkv"),
                VideoSourceConfig::new("b.mkv"),
                VideoSourceConfig::new("c.mkv"),
            ],
        }
    }
}

impl Validate for BraidRetrackVideoConfig {
    fn validate(&mut self) -> Result<()> {
        let n_output_videos = self.output.iter().filter(|x| x.type_ == "video").count();
        if n_output_videos != 1 {
            anyhow::bail!(
                "{} output videos specified, but only exactly one is supported.",
                n_output_videos
            );
        }

        let n_output_debug_txt = self
            .output
            .iter()
            .filter(|x| x.type_ == "debug_txt")
            .count();
        if n_output_debug_txt > 1 {
            anyhow::bail!(
                "{} output debug text files specified, but at most one is supported.",
                n_output_debug_txt
            );
        }

        for output in self.output.iter_mut() {
            output.validate()?;
        }
        if self.input_video.is_empty() {
            anyhow::bail!("No input videos found. At least one source is required.")
        }
        for source in self.input_video.iter_mut() {
            source.validate()?;
        }
        Ok(())
    }
}

pub trait Validate {
    fn validate(&mut self) -> Result<()>;
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct VideoSourceConfig {
    pub filename: String,
    pub camera_name: Option<String>,
}

impl VideoSourceConfig {
    fn new(filename: &str) -> Self {
        Self {
            filename: filename.to_string(),
            camera_name: None,
        }
    }
}

impl Validate for VideoSourceConfig {
    fn validate(&mut self) -> Result<()> {
        if !self.filename.to_lowercase().ends_with(".mkv") {
            anyhow::bail!("Video source filename does not end with \".mkv\".")
        }
        Ok(())
    }
}

#[test]
fn test_default_config_is_valid_and_serializable() {
    let mut default_config = BraidRetrackVideoConfig::default();
    default_config.validate().unwrap();
    toml::to_string_pretty(&default_config).unwrap();
}