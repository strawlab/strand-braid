use clap::Parser;
use eyre::{self, WrapErr};
use indicatif::{ProgressBar, ProgressStyle};

use frame_source::{ImageData, Timestamp};

#[derive(Debug, Parser)]
#[command(version, about)]
struct Opt {
    /// Input video filename.
    #[arg(short, long)]
    input: camino::Utf8PathBuf,

    /// Recording ID
    #[arg(short, long)]
    recording_id: Option<String>,

    /// Entity Path
    #[arg(short, long)]
    entity_path: Option<String>,

    /// If true, connect directly to rerun viewer using GRPC rather than saving an output file.
    #[arg(short, long)]
    connect: bool,

    /// Output rrd filename. Defaults to "<INPUT>.rrd".
    ///
    /// This must not be used with --connect.
    #[arg(short, long)]
    output: Option<camino::Utf8PathBuf>,

    /// Start time of the video. By default, this will be read from the video itself.
    #[arg(short, long)]
    start_time: Option<chrono::DateTime<chrono::FixedOffset>>,

    /// Exclude frames before this time.
    #[arg(long)]
    exclude_before: Option<chrono::DateTime<chrono::FixedOffset>>,

    /// Exclude frames after this time.
    #[arg(long)]
    exclude_after: Option<chrono::DateTime<chrono::FixedOffset>>,

    /// Force the video to be interpreted as having this frame rate (in frames per second).
    ///
    /// By default, timestamps in the video itself will be used.
    #[arg(short, long)]
    framerate: Option<f64>,

    /// Disable display of progress indicator
    #[arg(long)]
    no_progress: bool,

    /// Filename with camera parameters. When given, used to remove distortion in output movie.
    ///
    /// This allows working around https://github.com/rerun-io/rerun/issues/2499.
    #[arg(long)]
    undistort_with_calibration: Option<String>,
}

fn to_rr_image(
    im: ImageData,
    undist_cache: &undistort_image::UndistortionCache,
) -> eyre::Result<re_types::archetypes::EncodedImage> {
    let decoded = match im {
        ImageData::Decoded(decoded) => decoded,
        _ => eyre::bail!("image not decoded"),
    };

    let to_save = { undistort_image::undistort_image(decoded.borrow(), &undist_cache)? };

    // jpeg compression TODO: give option to save uncompressed?
    let contents = to_save
        .borrow()
        .to_encoded_buffer(convert_image::EncoderOptions::Jpeg(80))?;
    Ok(re_types::archetypes::EncodedImage::from_file_contents(
        contents,
    ))
}

/// A guard to remove incomplete file.
///
/// Create this and call [Self::keep_file] when the file is correctly written.
///
/// [Self::keep_file] is not called, the file will be automatically deleted when
/// this goes out of scope.
struct DeleteIfDropped {
    fname: Option<camino::Utf8PathBuf>,
}

impl Drop for DeleteIfDropped {
    fn drop(&mut self) {
        if let Some(fname) = self.fname.take() {
            std::fs::remove_file(fname).unwrap()
        }
    }
}

impl DeleteIfDropped {
    fn new(fname: camino::Utf8PathBuf) -> Self {
        Self { fname: Some(fname) }
    }

    fn keep_file(mut self) {
        self.fname = None;
    }
}

fn get_timestamp(ts: &chrono::DateTime<chrono::FixedOffset>) -> i64 {
    ts.timestamp_nanos_opt().unwrap()
}

fn main() -> eyre::Result<()> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "info");
    }
    env_tracing_logger::init();
    let opt = Opt::parse();

    let entity_path = if let Some(p) = opt.entity_path.as_ref() {
        p.clone()
    } else {
        // get just the filename part
        let movie_filename = opt.input.file_name().unwrap().to_string();
        movie_filename
    };

    let undist_cache = if let Some(yaml_intrinsics_fname) = &opt.undistort_with_calibration {
        let yaml_buf = std::fs::read_to_string(&yaml_intrinsics_fname)
            .with_context(|| format!("while reading {yaml_intrinsics_fname}"))?;

        let intrinsics: opencv_ros_camera::RosCameraInfo<f64> = serde_yaml::from_str(&yaml_buf)
            .with_context(|| format!("while parsing {yaml_intrinsics_fname}"))?;

        let intrinsics: opencv_ros_camera::NamedIntrinsicParameters<f64> =
            intrinsics.try_into().unwrap();
        let undist_cache = undistort_image::UndistortionCache::new(
            &intrinsics.intrinsics,
            intrinsics.width,
            intrinsics.height,
        )?;
        Some(undist_cache)
    } else {
        None
    };

    // We need the images iff we will undistort them.
    let do_decode_h264 = undist_cache.is_some();

    let mut src = frame_source::FrameSourceBuilder::new(&opt.input)
        .do_decode_h264(do_decode_h264)
        .build_source()?;

    let re_version = re_sdk::build_info().version;
    tracing::info!("Rerun version: {re_version}");
    tracing::info!("Frame size: {}x{}", src.width(), src.height());

    let start_time = if let Some(t) = opt.start_time.as_ref() {
        *t
    } else {
        src.frame0_time().ok_or_else(|| {
            eyre::eyre!(
                "video start time could not be determined from the \
        video, nor was it specified on the command line."
            )
        })?
    };
    // let frametimes = self.frametimes.get(&cam_data.camn).unwrap();
    // let (data2d_fnos, data2d_stamps): (Vec<i64>, Vec<f64>) = frametimes.iter().cloned().unzip();

    // Initiate recording
    let mut rec_builder = re_sdk::RecordingStreamBuilder::new(env!("CARGO_PKG_NAME"));

    if let Some(recording_id) = opt.recording_id {
        rec_builder = rec_builder.recording_id(recording_id);
    }

    let output = match (opt.output, opt.connect) {
        (Some(output), false) => Some(output),
        (None, false) => {
            let mut output = opt.input.as_os_str().to_owned();
            output.push(".rrd");
            let output = camino::Utf8PathBuf::from_os_string(output).unwrap();
            Some(output)
        }
        (Some(_output), true) => {
            eyre::bail!("Cannot specify output file when connecting directly to rerun.");
        }
        (None, true) => None,
    };

    let (rec, drop_guard) = if opt.connect {
        (rec_builder.connect_grpc()?, None)
    } else {
        let output = output.unwrap();
        let rec = rec_builder
            .save(&output)
            .wrap_err_with(|| format!("Creating output file {output}"))?;
        tracing::info!("Saving output to {output}");
        let delete_if_dropped = DeleteIfDropped::new(output);
        (rec, Some(delete_if_dropped))
    };

    let src_iter = src.iter();

    let pb = if !opt.no_progress {
        let (_, max_size) = src_iter.size_hint();
        if let Some(max_size) = max_size {
            if opt.exclude_before.is_none() && opt.exclude_after.is_none() {
                // Custom progress bar with space at right end to prevent obscuring last
                // digit with cursor.
                let style = ProgressStyle::with_template("{wide_bar} {pos}/{len} ETA: {eta} ")?;
                Some(ProgressBar::new(max_size.try_into().unwrap()).with_style(style))
            } else {
                // Have not computed number of frames, so cannot have progress bar.
                tracing::info!(
                    "Not showing progress bar due to use of exclude_before or \
                exclude_after options."
                );
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    if undist_cache.is_some() {
        if opt.connect {
            tracing::info!("Reading timestamps, converting images, and sending to rerun");
        } else {
            tracing::info!("Reading timestamps, converting images, and saving to RRD");
        }
    } else {
        tracing::info!("Reading timestamps");
    }

    let mut absolute_timestamps = Vec::new();
    for (fno, frame) in src_iter.enumerate() {
        let frame = frame?;
        let pts = if let Some(forced_framerate) = opt.framerate.as_ref() {
            let elapsed_secs = fno as f64 / forced_framerate;
            std::time::Duration::from_secs_f64(elapsed_secs)
        } else {
            match frame.timestamp() {
                Timestamp::Duration(pts) => pts,
                _ => {
                    eyre::bail!(
                        "video has no PTS timestamps and framerate was not \
                    specified on the command line."
                    );
                }
            }
        };

        let stamp_chrono = start_time + pts;
        absolute_timestamps.push(stamp_chrono);

        if let Some(first_time) = opt.exclude_before {
            if stamp_chrono < first_time {
                continue;
            }
        }

        if let Some(last_time) = opt.exclude_after {
            if stamp_chrono > last_time {
                continue;
            }
        }

        if let Some(undist_cache) = undist_cache.as_ref() {
            {
                let time: std::time::SystemTime = stamp_chrono.into();
                let time: re_sdk::TimeCell = time.try_into().map_err(|_| eyre::eyre!("ts fail"))?;
                rec.set_time("wall_clock", time);
            }

            let image = to_rr_image(frame.into_image(), undist_cache)?;

            rec.log(entity_path.as_str(), &image)?;
        }

        if let Some(pb) = &pb {
            // Increment the counter.
            pb.inc(1);
        }
    }
    if let Some(pb) = pb {
        pb.finish_and_clear();
    }

    if undist_cache.is_none() {
        if opt.connect {
            tracing::info!("Sending video to rerun");
        } else {
            tracing::info!("Saving video to RRD");
        }
        let video_asset = re_types::archetypes::AssetVideo::from_file_path(&opt.input).unwrap();
        rec.log_static(entity_path.as_str(), &video_asset)?;
        let frame_timestamps_nanos =
            re_types::archetypes::AssetVideo::read_frame_timestamps_nanos(&video_asset)?;
        let video_timestamps_nanos = frame_timestamps_nanos
            .iter()
            .copied()
            .map(re_types::components::VideoTimestamp::from_nanos)
            .collect::<Vec<_>>();
        let absolute_nanos: Vec<i64> = absolute_timestamps.iter().map(get_timestamp).collect();
        let time_column =
            re_chunk::TimeColumn::new_timestamp_nanos_since_epoch("wall_clock", absolute_nanos);
        rec.send_columns(
            entity_path,
            [time_column],
            re_types::archetypes::VideoFrameReference::update_fields()
                .with_many_timestamp(video_timestamps_nanos)
                .columns_of_unit_batches()?,
        )?;
    }

    if let Some(delete_if_dropped) = drop_guard {
        delete_if_dropped.keep_file();
    }
    Ok(())
}
