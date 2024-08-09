use clap::Parser;
use color_eyre::eyre::{self, WrapErr};
use indicatif::{ProgressBar, ProgressStyle};
use machine_vision_formats::{pixel_format, PixFmt};
use ndarray::Array;
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
}

fn to_rr_image(im: ImageData) -> eyre::Result<rerun::Image> {
    let decoded = match im {
        ImageData::Decoded(decoded) => decoded,
        _ => eyre::bail!("image not decoded"),
    };

    if true {
        // jpeg compression
        let contents = basic_frame::match_all_dynamic_fmts!(
            &decoded,
            x,
            convert_image::frame_to_image(x, convert_image::ImageOptions::Jpeg(80),)
        )?;
        let format = Some(rerun::external::image::ImageFormat::Jpeg);
        Ok(rerun::Image::from_file_contents(contents, format).unwrap())
    } else {
        // Much larger file size but higher quality.
        let w = decoded.width() as usize;
        let h = decoded.height() as usize;

        let image = match decoded.pixel_format() {
            PixFmt::Mono8 => {
                let mono8 = decoded.clone().into_pixel_format::<pixel_format::Mono8>()?;
                Array::from_vec(mono8.into()).into_shape((h, w, 1)).unwrap()
            }
            _ => {
                let rgb8 = decoded
                    .clone()
                    .into_pixel_format::<machine_vision_formats::pixel_format::RGB8>()?;
                Array::from_vec(rgb8.into()).into_shape((h, w, 3)).unwrap()
            }
        };
        Ok(rerun::Image::try_from(image)?)
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

    let do_decode_h264 = true;
    let mut src = frame_source::from_path(&opt.input, do_decode_h264)?;

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
    let mut rec_builder = rerun::RecordingStreamBuilder::new(env!("CARGO_PKG_NAME"));

    if let Some(recording_id) = opt.recording_id {
        rec_builder = rec_builder.recording_id(recording_id);
    }

    let rec = rec_builder
        .save(&output)
        .wrap_err_with(|| format!("Creating output file {}", output.display()))?;

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
        let image = to_rr_image(frame.into_image())?;

        rec.log(entity_path.as_str(), &image)?;
        if let Some(pb) = &pb {
            // Increment the counter.
            pb.inc(1);
        }
    }
    if let Some(pb) = pb {
        pb.finish_and_clear();
    }
    Ok(())
}
