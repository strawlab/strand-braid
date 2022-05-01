use anyhow::Result;

use crate::config::{
    path_to_string, BraidRetrackVideoConfig, OutputConfig, Valid, VideoSourceConfig,
};

pub fn auto_config<P: AsRef<std::path::Path>>(
    source_dir: P,
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
    let mut output_path = source_dir.as_ref().to_path_buf();
    let file_name = format!(
        "{}-rendered.mkv",
        output_path
            .file_name()
            .unwrap()
            .to_os_string()
            .to_str()
            .unwrap()
    );
    output_path.set_file_name(file_name);

    let output = vec![OutputConfig {
        type_: "video".to_string(),
        filename: path_to_string(output_path)?,
        video_options: None,
    }];

    let cfg = BraidRetrackVideoConfig {
        input_braidz,
        input_video,
        output,
        ..Default::default()
    };

    Ok(cfg.validate(Some(source_dir))?)
}
