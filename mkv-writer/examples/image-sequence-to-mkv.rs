fn usage_exit() -> Result<(), anyhow::Error> {
    println!(
        "Usage:

        image-sequence-to-mkv <FOLDER_NAME> <OUTPUT_NAME.MKV>"
    );
    Err(anyhow::format_err!("invalid usage"))
}

fn main() -> Result<(), anyhow::Error> {
    env_logger::init();

    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        usage_exit()?;
    }

    let input_name = &args[1];
    let output_fname = &args[2];

    let out_fd = std::fs::File::create(&output_fname)?;
    let fps = 20.0;
    let dt_nano = (1.0 / fps * 1e9) as i64;

    let cfg = ci2_remote_control::MkvRecordingConfig {
        codec: ci2_remote_control::MkvCodec::VP9(ci2_remote_control::VP9Options { bitrate: 10000 }),
        max_framerate: ci2_remote_control::RecordingFrameRate::Unlimited,
        ..Default::default()
    };

    let mut my_mkv_writer = mkv_writer::MkvWriter::new(out_fd, cfg, None)?;

    let mut entries = std::fs::read_dir(input_name)?
        .map(|res| res.map(|e| e.path()))
        .collect::<Result<Vec<_>, std::io::Error>>()?;
    entries.sort();

    let start = chrono::Utc::now();

    for (count, file_path) in entries.iter().enumerate() {
        log::info!("{}", file_path.display());

        let dt = chrono::Duration::nanoseconds(dt_nano * count as i64);

        let ts = start.checked_add_signed(dt).unwrap();

        // The text to render
        let image = image::open(file_path)?;
        let rgb = convert_image::piston_to_frame(image)?;

        my_mkv_writer.write(&rgb, ts)?;
    }

    my_mkv_writer.finish()?;
    Ok(())
}
