use clap::Parser;
use eyre::{self, WrapErr};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::PathBuf;

use basic_frame::DynamicFrame;
use frame_source::{ImageData, Timestamp};

#[derive(Debug, Parser)]
#[command(version, about)]
struct Opt {
    /// Input video filename.
    #[arg(short, long)]
    input: PathBuf,

    /// Recording ID
    #[arg(short, long)]
    recording_id: Option<String>,

    /// Entity Path
    #[arg(short, long)]
    entity_path: Option<String>,

    /// Output rrd filename. Defaults to "<INPUT>.rrd"
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Start time of the video. By default, this will be read from the video itself.
    #[arg(short, long)]
    start_time: Option<chrono::DateTime<chrono::FixedOffset>>,

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
    undist_cache: Option<&undistort_image::UndistortionCache>,
) -> eyre::Result<re_types::archetypes::EncodedImage> {
    let decoded = match im {
        ImageData::Decoded(decoded) => decoded,
        _ => eyre::bail!("image not decoded"),
    };

    let to_save = if let Some(undist_cache) = undist_cache {
        undistort_image::undistort_image(decoded, &undist_cache)?
    } else {
        decoded
    };

    // jpeg compression TODO: give option to save uncompressed?
    let contents = basic_frame::match_all_dynamic_fmts!(
        &to_save,
        x,
        convert_image::frame_to_encoded_buffer(x, convert_image::EncoderOptions::Jpeg(80),)
    )?;
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
    fname: Option<PathBuf>,
}

impl Drop for DeleteIfDropped {
    fn drop(&mut self) {
        if let Some(fname) = self.fname.take() {
            std::fs::remove_file(fname).unwrap()
        }
    }
}

impl DeleteIfDropped {
    fn new(fname: PathBuf) -> Self {
        Self { fname: Some(fname) }
    }

    fn keep_file(mut self) {
        self.fname = None;
    }
}

fn main() -> eyre::Result<()> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "info");
    }
    env_tracing_logger::init();
    let opt = Opt::parse();

    let output = opt.output;

    let output = output.unwrap_or_else(|| {
        let mut output = opt.input.as_os_str().to_owned();
        output.push(".rrd");
        output.into()
    });

    let mut src = frame_source::FrameSourceBuilder::new(&opt.input).build_source()?;

    let entity_path = if let Some(p) = opt.entity_path.as_ref() {
        p.clone()
    } else {
        // get just the filename part
        let movie_filename = opt
            .input
            .file_name()
            .unwrap()
            .to_os_string()
            .to_str()
            .unwrap()
            .to_string();
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

    let rec = rec_builder
        .save(&output)
        .wrap_err_with(|| format!("Creating output file {}", output.display()))?;
    let delete_if_dropped = DeleteIfDropped::new(output);

    let src_iter = src.iter();

    let pb = if !opt.no_progress {
        let (_, max_size) = src_iter.size_hint();
        if let Some(max_size) = max_size {
            // Custom progress bar with space at right end to prevent obscuring last
            // digit with cursor.
            let style = ProgressStyle::with_template("{wide_bar} {pos}/{len} ETA: {eta} ")?;
            Some(ProgressBar::new(max_size.try_into().unwrap()).with_style(style))
        } else {
            None
        }
    } else {
        None
    };

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
        let stamp_flydra =
            flydra_types::FlydraFloatTimestampLocal::<flydra_types::Triggerbox>::from(stamp_chrono);
        let stamp_f64 = stamp_flydra.as_f64();
        rec.set_time_seconds("wall_clock", stamp_f64);
        let image = to_rr_image(frame.into_image(), undist_cache.as_ref())?;

        rec.log(entity_path.as_str(), &image)?;
        if let Some(pb) = &pb {
            // Increment the counter.
            pb.inc(1);
        }
    }
    if let Some(pb) = pb {
        pb.finish_and_clear();
    }

    delete_if_dropped.keep_file();
    Ok(())
}
