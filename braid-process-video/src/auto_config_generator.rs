use color_eyre::{
    eyre::{self as anyhow},
    Result,
};

use crate::config::{
    path_to_string, BraidRetrackVideoConfig, DebugOutputConfig, OutputConfig, Valid, Validate,
    VideoOutputConfig, VideoOutputOptions, VideoSourceConfig,
};

pub fn auto_config<P: AsRef<std::path::Path>>(
    source_dir: P,
    max_num_frames: Option<usize>,
    with_debug_file: bool,
    time_dilation_factor: Option<f32>,
) -> Result<Valid<BraidRetrackVideoConfig>> {
    tracing::info!(
        "generating auto config from dir \"{}\"",
        source_dir.as_ref().display()
    );

    let mut input_braidz = None;
    let mut input_video = vec![];

    for entry in std::fs::read_dir(source_dir.as_ref())? {
        let filename = path_to_string(entry?.path())?;

        if filename.to_lowercase().ends_with(".braidz") {
            if input_braidz.is_some() {
                anyhow::bail!("More than on input .braidz file is not supported");
            }
            input_braidz = Some(filename);
        } else {
            for extension in crate::config::VALID_VIDEO_SOURCES.iter() {
                if filename.to_lowercase().ends_with(extension) {
                    input_video.push(VideoSourceConfig {
                        filename,
                        camera_name: None,
                    });
                    break;
                }
            }
        }
    }

    // from input in `/path/of/input`, output is `/path/of/input-rendered.mp4`
    let output_path = source_dir.as_ref().to_path_buf();

    let output_file_name = format!(
        "{}-rendered.mp4",
        output_path
            .as_path()
            .file_name()
            .unwrap()
            .to_os_string()
            .to_str()
            .unwrap()
    );
    let output_video_path = source_dir.as_ref().with_file_name(output_file_name);

    let video_options = VideoOutputOptions {
        time_dilation_factor,
        ..Default::default()
    };
    let mut output = vec![OutputConfig::Video(VideoOutputConfig {
        filename: path_to_string(output_video_path)?,
        video_options,
    })];

    if with_debug_file {
        let mut output_debug_path = output_path.clone();
        output_debug_path.set_file_name(format!(
            "{}-debug.txt",
            output_path
                .file_name()
                .unwrap()
                .to_os_string()
                .to_str()
                .unwrap()
        ));
        output.push(OutputConfig::DebugTxt(DebugOutputConfig {
            filename: path_to_string(output_debug_path)?,
        }))
    }

    let cfg = BraidRetrackVideoConfig {
        input_braidz,
        input_video,
        output,
        max_num_frames,
        ..Default::default()
    };

    cfg.validate::<std::path::PathBuf>(None)
}
