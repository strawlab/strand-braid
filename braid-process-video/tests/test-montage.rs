use braid_process_video::{
    BraidRetrackVideoConfig, OutputConfig, Valid, Validate, VideoOutputConfig, VideoSourceConfig,
};

const BASE_URL: &str = "https://strawlab-cdn.com/assets/flycube6-videos";
const SOURCE_JSON: &str = include_str!("source.json");

async fn do_config(cfg: &Valid<BraidRetrackVideoConfig>) -> anyhow::Result<()> {
    // generate the output
    let output_fnames = braid_process_video::run_config(cfg).await?;
    assert_eq!(output_fnames.len(), 1);
    let output_fname = output_fnames[0].clone();

    // start parsing output
    let do_decode_h264 = false;
    let _src = frame_source::from_path(&output_fname, do_decode_h264)?;

    // TODO: check output. How?

    Ok(())
}

fn parse_file_list(dirname: &str) -> anyhow::Result<Vec<(String, String)>> {
    let source_json: serde_json::Value = serde_json::from_str(SOURCE_JSON)?;
    let source_json = source_json.as_object().unwrap();
    let files = source_json.get(dirname).unwrap().as_array().unwrap();
    let mut results = vec![];
    for file_src in files {
        let elements = file_src.as_array().unwrap();
        assert_eq!(elements.len(), 2);
        let fname = elements[0].as_str().unwrap();
        let sha256sum = elements[1].as_str().unwrap();
        results.push((fname.to_string(), sha256sum.to_string()));
    }
    Ok(results)
}

fn get_files(
    dirname: &str,
    max_num_frames: Option<usize>,
) -> anyhow::Result<Valid<BraidRetrackVideoConfig>> {
    let file_list = parse_file_list(dirname)?;

    let outdir = format!("tests/downloaded-data/{}", dirname);
    let mut input_braidz = None;
    let mut input_video = vec![];
    for (fname, sha256sum) in file_list.iter() {
        let dest = format!("{}/{}", outdir, fname);
        download_verify::download_verify(
            format!("{}/{}/{}", BASE_URL, dirname, fname).as_str(),
            &dest,
            &download_verify::Hash::Sha256(sha256sum.into()),
        )?;

        if fname.ends_with(".braidz") {
            input_braidz = Some(dest);
        } else {
            // Get everything before first '.'.
            let stem = fname.split('.').next().unwrap();

            // Convert "movie20211109_080701_Basler-21714402" to "Basler-21714402".
            let camera_name = stem.split('_').skip(2).collect::<Vec<_>>().join("_");

            input_video.push(VideoSourceConfig {
                filename: dest,
                camera_name: Some(camera_name),
            });
        }
    }

    let input_braidz = input_braidz.map(Into::into);
    let output = vec![OutputConfig::Video(VideoOutputConfig {
        filename: format!("tests/rendered/{}.mp4", dirname),
        video_options: Default::default(),
    })];

    let cfg = BraidRetrackVideoConfig {
        input_braidz,
        input_video,
        output,
        max_num_frames,
        ..Default::default()
    };

    let basedir: Option<String> = None;
    cfg.validate(basedir)
}

fn init_logging() {
    let _ = env_logger::builder().is_test(true).try_init();
}

#[ignore]
#[tokio::test]
async fn test_fc6_led_100fps_2_cams_dark() -> anyhow::Result<()> {
    init_logging();
    let dirname = "fc6-led-100fps-2-cams-dark";

    do_config(&get_files(dirname, Some(100))?).await?;
    Ok(())
}

#[ignore]
#[tokio::test]
async fn test_fc6_led_4fps_5_cams_bright() -> anyhow::Result<()> {
    init_logging();
    let dirname = "fc6-led-4fps-5-cams-bright";

    do_config(&get_files(dirname, Some(100))?).await?;
    Ok(())
}

#[ignore]
#[tokio::test]
async fn test_fc6_led_4fps_5_cams_dark() -> anyhow::Result<()> {
    init_logging();
    let dirname = "fc6-led-4fps-5-cams-dark";

    do_config(&get_files(dirname, Some(100))?).await?;
    Ok(())
}

#[ignore]
#[tokio::test]
async fn test_fc6_flies_100fps_2_cams_mkv() -> anyhow::Result<()> {
    init_logging();
    let dirname = "fc6-flies-100fps-2-cams";

    do_config(&get_files(dirname, Some(100))?).await?;
    Ok(())
}

#[ignore]
#[tokio::test]
async fn test_fc6_flies_100fps_2_cams_mp4() -> anyhow::Result<()> {
    init_logging();
    let dirname = "fc6-flies-100fps-2-cams-mp4";

    do_config(&get_files(dirname, Some(100))?).await?;
    Ok(())
}
