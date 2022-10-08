use anyhow::Result;

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
    log::info!(
        "generating auto config from dir \"{}\"",
        source_dir.as_ref().display()
    );

    let mut input_braidz = None;
    let mut input_video = vec![];

    for entry in std::fs::read_dir(source_dir.as_ref())? {
        let fname = entry?.path();
        let filename = path_to_string(fname)?;

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

    // from input in `/path/of/input`, output is `/path/of/input-rendered.mkv`
    let output_path = source_dir.as_ref().to_path_buf();

    let mut output_video_path = output_path.clone();
    output_video_path.set_file_name(format!(
        "{}-rendered.mkv",
        output_path
            .file_name()
            .unwrap()
            .to_os_string()
            .to_str()
            .unwrap()
    ));

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

    cfg.validate(Some(source_dir))
}
