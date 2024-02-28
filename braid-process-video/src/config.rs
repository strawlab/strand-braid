use color_eyre::{
    eyre::{self as anyhow},
    Result,
};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// A wrapper newtype indicating the inner type has been validated.
pub struct Valid<T>(T);

impl<T> Valid<T> {
    /// Return a reference to the validated inner type.
    pub fn valid(&self) -> &T {
        &self.0
    }
}

pub trait Validate {
    /// Validate the configuration.
    ///
    /// If `basedir` is not `None`, it specifies the directory in which relative
    /// filenames are searched.
    fn validate<P: AsRef<Path>>(self, basedir: Option<P>) -> Result<Valid<Self>>
    where
        Self: Sized;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, tag = "type")]
pub enum OutputConfig {
    #[serde(rename = "video")]
    Video(VideoOutputConfig),
    #[serde(rename = "debug_txt")]
    DebugTxt(DebugOutputConfig),
    #[serde(rename = "braidz")]
    Braidz(BraidzOutputConfig),
}

impl Default for OutputConfig {
    fn default() -> Self {
        OutputConfig::Video(Default::default())
    }
}

impl Validate for OutputConfig {
    fn validate<P: AsRef<Path>>(self, basedir: Option<P>) -> Result<Valid<Self>> {
        match self {
            OutputConfig::Video(v) => Ok(Valid(OutputConfig::Video(v.validate(basedir)?.0))),
            OutputConfig::DebugTxt(d) => Ok(Valid(OutputConfig::DebugTxt(d.validate(basedir)?.0))),
            OutputConfig::Braidz(b) => Ok(Valid(OutputConfig::Braidz(b.validate(basedir)?.0))),
        }
    }
}

impl OutputConfig {
    pub fn filename(&self) -> &str {
        match self {
            OutputConfig::Video(v) => &v.filename,
            OutputConfig::DebugTxt(d) => &d.filename,
            OutputConfig::Braidz(b) => &b.filename,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BraidzOutputConfig {
    /// The filename of the output desired.
    pub filename: String,
}

impl Default for BraidzOutputConfig {
    fn default() -> Self {
        Self {
            filename: "output.braidz".to_string(),
        }
    }
}

impl Validate for BraidzOutputConfig {
    fn validate<P: AsRef<Path>>(self, basedir: Option<P>) -> Result<Valid<Self>> {
        // Validate `filename`
        let filename = base_join_inner(self.filename, basedir)?;
        Ok(Valid(Self { filename }))
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProcessingConfig {
    pub feature_detection_method: FeatureDetectionMethod,
    pub camera_calibration_source: CameraCalibrationSource,
    pub tracking_parameters_source: TrackingParametersSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, tag = "type")]
pub enum FeatureDetectionMethod {
    #[serde(rename = "copy")]
    CopyExisting,
    // #[serde(rename = "bright-point")]
    // BrightPoint(BrightPointOptions),
    // #[serde(rename = "flydra")]
    // Flydra,
}

impl Default for FeatureDetectionMethod {
    fn default() -> FeatureDetectionMethod {
        FeatureDetectionMethod::CopyExisting
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BrightPointOptions {
    max_num_points: usize,
}

impl Default for BrightPointOptions {
    fn default() -> Self {
        Self { max_num_points: 10 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, tag = "type")]
pub enum CameraCalibrationSource {
    #[serde(rename = "none")]
    None,
    #[serde(rename = "copy")]
    CopyExisting,
}

impl Default for CameraCalibrationSource {
    fn default() -> CameraCalibrationSource {
        CameraCalibrationSource::None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, tag = "type")]
pub enum TrackingParametersSource {
    #[serde(rename = "copy")]
    CopyExisting,
    #[serde(rename = "default")]
    Default,
}

impl Default for TrackingParametersSource {
    fn default() -> TrackingParametersSource {
        TrackingParametersSource::Default
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DebugOutputConfig {
    /// The filename of the output desired.
    pub filename: String,
}

impl Validate for DebugOutputConfig {
    fn validate<P: AsRef<Path>>(self, basedir: Option<P>) -> Result<Valid<Self>> {
        let filename = base_join_inner(self.filename, basedir)?;
        Ok(Valid(Self { filename }))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct VideoOutputConfig {
    /// The filename of the output desired.
    pub filename: String,
    /// If the output type is "mp4", the options for the emitted MP4 file.
    #[serde(default)]
    pub video_options: VideoOutputOptions,
}

impl Validate for VideoOutputConfig {
    /// Validate the configuration.
    ///
    /// If `basedir` is not `None`, it specifies the directory for relative
    /// filenames.
    fn validate<P: AsRef<Path>>(self, basedir: Option<P>) -> Result<Valid<Self>> {
        // Validate `filename`
        let filename = base_join_inner(self.filename, basedir)?;

        // Validate `video_options`.
        let video_options = self.video_options.validate()?.0;
        Ok(Valid(Self {
            filename,
            video_options,
        }))
    }
}

impl Default for VideoOutputConfig {
    fn default() -> Self {
        Self {
            filename: "output.mp4".to_string(),
            video_options: VideoOutputOptions::default(),
        }
    }
}

pub const VALID_VIDEO_SOURCES: &[&str] = &[".fmf", ".fmf.gz", ".mkv", ".mp4"];

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Clone)]
#[serde(deny_unknown_fields)]
pub struct VideoOutputOptions {
    /// The space surrounding each image in the composite view.
    ///
    /// The default value of `None` will resolve to
    /// [`crate::DEFAULT_COMPOSITE_MARGIN_PIXELS`].
    pub composite_margin_pixels: Option<usize>,
    /// The multiplier by which time is slowed down in the output video.
    ///
    /// A value of 10.0 means the output will be slowed by tenfold. The default
    /// value of `None` render 1:1 at realtime.
    pub time_dilation_factor: Option<f32>,
    /// The radius of the circle to overlay when drawing braidz 2D features.
    ///
    /// The default value of `None` will resolve to
    /// [`crate::DEFAULT_FEATURE_RADIUS`].
    pub feature_radius: Option<String>,
    /// The SVG style string of the point to overlay when drawing braidz 2D features.
    ///
    /// The default value of `None` will resolve to [`crate::DEFAULT_FEATURE_STYLE`].
    pub feature_style: Option<String>,
    /// The SVG style string of the camera text.
    ///
    /// The default value of `None` will resolve to [`crate::DEFAULT_CAMERA_TEXT_STYLE`].
    pub cam_text_style: Option<String>,
    /// The title of the saved video, set in the segment metadata.
    ///
    /// The default value of `None` means this value will not be set in the
    /// saved video.
    pub title: Option<String>,
}

impl VideoOutputOptions {
    fn validate(self) -> Result<Valid<Self>> {
        // Validate `time_dilation_factor`.
        let time_dilation_factor = if self.time_dilation_factor == Some(1.0) {
            None
        } else {
            self.time_dilation_factor
        };
        Ok(Valid(Self {
            time_dilation_factor,
            ..self
        }))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BraidRetrackVideoConfig {
    /// Specifies the maximum duration between frames to count as "synchronous",
    /// defaults to half of `frame_duration_microsecs`.
    pub sync_threshold_microseconds: Option<u64>,
    /// The interval between adjacent frames. Defaults to the value detected in
    /// the first frames of the given video inputs.
    pub frame_duration_microsecs: Option<u64>,
    /// The first output frame to render, skipping prior frames
    pub skip_n_first_output_frames: Option<usize>,
    /// maximum number of frames to render
    pub max_num_frames: Option<usize>,
    /// Every `log_interval_frames` a status message will be displayed.
    pub log_interval_frames: Option<usize>,
    pub input_braidz: Option<String>,
    #[serde(default)]
    pub input_video: Vec<VideoSourceConfig>,
    pub output: Vec<OutputConfig>,
    #[serde(default)]
    pub processing_config: ProcessingConfig,
}

impl Default for BraidRetrackVideoConfig {
    fn default() -> Self {
        Self {
            sync_threshold_microseconds: None,
            frame_duration_microsecs: None,
            skip_n_first_output_frames: None,
            max_num_frames: None,
            log_interval_frames: None,
            input_braidz: None,
            output: vec![OutputConfig::default()],
            input_video: vec![
                VideoSourceConfig::new("a.mkv"),
                VideoSourceConfig::new("b.mkv"),
                VideoSourceConfig::new("c.mkv"),
            ],
            processing_config: ProcessingConfig::default(),
        }
    }
}

impl Validate for BraidRetrackVideoConfig {
    /// Validate the configuration.
    ///
    /// If `basedir` is not `None`, it specifies the directory in which relative
    /// filenames are searched.
    fn validate<P: AsRef<Path>>(self, basedir: Option<P>) -> Result<Valid<Self>> {
        if self.input_video.is_empty() && self.input_braidz.is_none() {
            anyhow::bail!("No input videos or braidz file. At least one source is required.")
        }

        // Validate `input_braidz`.
        let input_braidz = base_join(self.input_braidz, basedir.as_ref())?;

        // Validate `output`.
        let output = self
            .output
            .into_iter()
            .map(|output| output.validate(basedir.as_ref()))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .map(|o| o.0)
            .collect();

        // Validate `input_video`.
        let input_video = self
            .input_video
            .into_iter()
            .map(|source| source.validate(basedir.as_ref()))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .map(|iv| iv.0)
            .collect();

        Ok(Valid(Self {
            input_braidz,
            output,
            input_video,
            ..self
        }))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
    /// Validate the configuration.
    ///
    /// If `basedir` is not `None`, it specifies the directory in which relative
    /// filenames are searched.
    fn validate<P: AsRef<Path>>(self, basedir: Option<P>) -> Result<Valid<Self>> {
        // Validate `filename`.
        let mut found = false;
        for extension in VALID_VIDEO_SOURCES.iter() {
            if self.filename.to_lowercase().ends_with(extension) {
                found = true;
                break;
            }
        }

        if !found {
            anyhow::bail!(
                "Video source filename \"{}\" is not one of {:?}.",
                self.filename,
                VALID_VIDEO_SOURCES,
            )
        }
        let filename = base_join_inner(self.filename, basedir)?;

        Ok(Valid(Self { filename, ..self }))
    }
}

pub(crate) fn path_to_string<P: AsRef<Path>>(p: P) -> Result<String> {
    p.as_ref()
        .as_os_str()
        .to_os_string()
        .into_string()
        .map_err(|os_str| anyhow::anyhow!("path \"{}\" is not UTF8", os_str.to_string_lossy()))
}

/// If `filename` is relative, join it to `basedir` if possible.
fn base_join_inner<P: AsRef<Path>>(filename: String, basedir: Option<P>) -> Result<String> {
    fn maybe_join<P: AsRef<Path>>(filename: String, basedir: Option<P>) -> std::path::PathBuf {
        let p = std::path::PathBuf::from(filename);
        match (p.is_relative(), basedir) {
            (true, Some(dirpath)) => dirpath.as_ref().to_path_buf().join(p),
            _ => p,
        }
    }

    path_to_string(maybe_join(filename, basedir))
}

/// If `filename` is not None and is relative, join it to `basedir` if possible.
fn base_join<P: AsRef<Path>>(
    filename: Option<String>,
    basedir: Option<P>,
) -> Result<Option<String>> {
    let fname = filename.map(|s| base_join_inner(s, basedir)).transpose()?;
    Ok(fname)
}

#[test]
fn test_default_config_is_valid_and_serializable() -> Result<()> {
    let basedir: Option<String> = None;
    let cfg = BraidRetrackVideoConfig::default().validate(basedir)?;
    toml::to_string_pretty(&cfg.valid())?;
    Ok(())
}
