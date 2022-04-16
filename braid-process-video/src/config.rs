use anyhow::Result;
use serde::{Deserialize, Serialize};

/// A wrapper newtype indicating the inner type has been validated.
pub struct Valid<T>(T);

impl<T> Valid<T> {
    /// Return a reference to the validated inner type.
    pub fn valid(&self) -> &T {
        &self.0
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputConfig {
    /// The type of output desired. Currently only type "video" is supported.
    #[serde(rename = "type")]
    pub type_: String,
    /// The filename of the output desired.
    pub filename: String,
    /// If the output type is "mkv", the options for the emitted MKV file.
    pub video_options: Option<OutputVideoConfig>,
}

impl OutputConfig {
    /// Validate the configuration.
    ///
    /// If `basedir` is not `None`, it specifies the directory in which relative
    /// filenames are searched.
    fn validate(self, basedir: Option<&std::path::Path>) -> Result<Valid<Self>> {
        // Validate `type_`.
        if !VALID_OUTPUT_TYPES.contains(&self.type_.as_str()) {
            anyhow::bail!(
                "Output type \"{}\" not one of: {:?}",
                self.type_,
                VALID_OUTPUT_TYPES
            )
        }

        // Validate `filename`
        let filename = base_join_inner(self.filename, basedir)?;

        // Validate `video_options`.
        let video_options = self
            .video_options
            .map(|opts| opts.validate())
            .transpose()?
            .map(|valid| valid.0);
        Ok(Valid(Self {
            filename,
            video_options,
            ..self
        }))
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
    pub composite_margin_pixels: Option<usize>,
    pub time_dilation_factor: Option<f32>,
    /// The radius of the circle to overlay when drawing braidz 2D features.
    pub feature_radius: Option<String>,
    /// The SVG style string of the point to overlay when drawing braidz 2D features.
    ///
    /// For example: "fill:none;stroke:deepskyblue;stroke-width:3".
    pub feature_style: Option<String>,
    /// The title of the saved video, set in the segment metadata.
    pub title: Option<String>,
}

impl Default for OutputVideoConfig {
    fn default() -> Self {
        Self {
            composite_margin_pixels: None,
            time_dilation_factor: None,
            feature_radius: None,
            feature_style: None,
            title: None,
        }
    }
}

impl OutputVideoConfig {
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

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BraidRetrackVideoConfig {
    /// Specifies the maximum duration between frames to count as "synchronous",
    /// defaults to half of `frame_duration_microsecs`.
    pub sync_threshold_microseconds: Option<u64>,
    /// The interval between adjacent frames. Defaults to the value detected in
    /// the first frames of the given video inputs.
    pub frame_duration_microsecs: Option<u64>,
    /// The first frame to render, skipping prior frames
    pub start_frame: Option<usize>,
    /// maximum number of frames to render
    pub max_num_frames: Option<usize>,
    /// Every `log_interval_frames` a status message will be displayed.
    pub log_interval_frames: Option<usize>,
    pub input_braidz: Option<String>,
    pub output: Vec<OutputConfig>,
    pub input_video: Vec<VideoSourceConfig>,
}

impl Default for BraidRetrackVideoConfig {
    fn default() -> Self {
        Self {
            sync_threshold_microseconds: None,
            frame_duration_microsecs: None,
            start_frame: None,
            max_num_frames: None,
            log_interval_frames: None,
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

impl BraidRetrackVideoConfig {
    /// Validate the configuration.
    ///
    /// If `basedir` is not `None`, it specifies the directory in which relative
    /// filenames are searched.
    pub fn validate(self, basedir: Option<&std::path::Path>) -> Result<Valid<Self>> {
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

        // Validate `input_braidz`.
        let input_braidz = base_join(self.input_braidz, basedir)?;

        // Validate `output`.
        let output = self
            .output
            .into_iter()
            .map(|output| output.validate(basedir))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .map(|o| o.0)
            .collect();

        // Validate `input_video`.
        if self.input_video.is_empty() {
            anyhow::bail!("No input videos found. At least one source is required.")
        }
        let input_video = self
            .input_video
            .into_iter()
            .map(|source| source.validate(basedir))
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

impl VideoSourceConfig {
    /// Validate the configuration.
    ///
    /// If `basedir` is not `None`, it specifies the directory in which relative
    /// filenames are searched.
    fn validate(self, basedir: Option<&std::path::Path>) -> Result<Valid<Self>> {
        // Validate `filename`.
        if !(self.filename.to_lowercase().ends_with(".mkv")
            || self.filename.to_lowercase().ends_with(".fmf")
            || self.filename.to_lowercase().ends_with(".fmf.gz"))
        {
            anyhow::bail!(
                "Video source filename \"{}\" does not end with \".mkv\", \".fmf\", or \".fmf.gz\".",
                self.filename
            )
        }
        let filename = base_join_inner(self.filename, basedir)?;

        Ok(Valid(Self { filename, ..self }))
    }
}

/// If `filename` is relative, join it to `basedir` if possible.
fn base_join_inner(filename: String, basedir: Option<&std::path::Path>) -> Result<String> {
    fn path_to_string(p: std::path::PathBuf) -> Result<String> {
        p.into_os_string()
            .into_string()
            .map_err(|os_str| anyhow::anyhow!("path \"{}\" is not UTF8", os_str.to_string_lossy()))
    }

    fn maybe_join(filename: String, basedir: Option<&std::path::Path>) -> std::path::PathBuf {
        let p = std::path::PathBuf::from(filename);
        match (p.is_relative(), basedir) {
            (true, Some(dirpath)) => dirpath.join(p),
            _ => p,
        }
    }

    path_to_string(maybe_join(filename, basedir))
}

/// If `filename` is not None and is relative, join it to `basedir` if possible.
fn base_join(
    filename: Option<String>,
    basedir: Option<&std::path::Path>,
) -> Result<Option<String>> {
    let fname = filename.map(|s| base_join_inner(s, basedir)).transpose()?;
    Ok(fname)
}

#[test]
fn test_default_config_is_valid_and_serializable() -> Result<()> {
    let cfg = BraidRetrackVideoConfig::default().validate(None)?;
    toml::to_string_pretty(&cfg.valid())?;
    Ok(())
}
