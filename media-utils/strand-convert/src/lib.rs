// Copyright 2022-2023 Andrew D. Straw.
//! Convert MKV videos saved by Strand Cam and Tiff Images saved by Micromanager
//! from Photometrics cameras into MP4 videos of the format saved by Strand Cam.
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use indicatif::{HumanBytes, HumanDuration, ProgressBar, ProgressStyle};
use ordered_float::NotNan;

use ci2_remote_control::H264Metadata;

use basic_frame::{match_all_dynamic_fmts, DynamicFrame};
use frame_source::{
    fmf_source, mp4_source, pv_tiff_stack, strand_cam_mkv_source, FrameData, FrameDataSource,
    ImageData,
};
use tiff_decoder::HdrConfig;

const N_FRAMES_TO_COMPUTE_FPS: usize = 100;

/// This program converts an input frame source into an output MP4 file (or a
/// PNG sequence if --export-pngs option is used).
///
/// It assumes that the input has a fixed framerate and encodes this into the
/// output file. Skipped frames are filled to maintain original timing. The
/// target framerate is computed from the first frames.
///
/// The --skip and --take options can adjust which frames go into the output
/// movie.
///
/// Metadata from Strand Camera is preserved when saving to MP4, but lost when
/// saving to a PNG sequence.
///
/// Large deviations of the data from the nominal framerate result in an error.
#[derive(Parser, Debug)]
#[command(author, version, about)]
pub struct Cli {
    /// Input. Either TIFF image directory (`/path/to/tifs/`) or `file.mkv`.
    ///
    /// For a TIFF image directory, images will be ordered alphabetically.
    #[arg(short, long)]
    input: String,

    /// Output filename when the output is an mp4 file or output directory when the output
    /// is a image sequence of PNG files.
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Export frames as a PNG image sequence (instead of mp4).
    ///
    /// This will loose timestamp information.
    #[arg(long)]
    export_pngs: bool,

    /// Set the H264 encoder
    ///
    /// (This is ignored when --export-pngs is set.)
    #[arg(long, value_enum)]
    encoder: Option<Encoder>,

    /// Limit the number of frames to process
    #[arg(long)]
    take: Option<usize>,

    /// Do not delete output movie in case of error
    #[arg(long)]
    truncate_on_error: bool,

    /// Show timestamps
    #[arg(long)]
    show_timestamps: bool,

    /// Ignore timing, just copy the data without trying to normalize timing
    ///
    /// (This is ignored when --export-pngs is set.)
    #[arg(long)]
    ignore_timing: bool,

    /// Skip this many frames at the start of the source data
    #[arg(long)]
    skip: Option<usize>,

    /// Hide the progress bar
    #[arg(long)]
    no_progress: bool,

    /// Overwrite existing output.
    #[arg(long)]
    overwrite: bool,

    /// Milliseconds between frames.
    ///
    /// If not set, an average computed from the first frames is used.
    #[arg(long)]
    frame_interval_msec: Option<f64>,

    /// Milliseconds of imprecision to accept in timestamp.
    ///
    /// If not set, a default value of half the frame_interval_msec is used.
    #[arg(long)]
    frame_interval_precision_msec: Option<f64>,

    /// Creation time
    ///
    /// If not set, the value is read from the source
    #[arg(long)]
    creation_time: Option<chrono::DateTime<chrono::FixedOffset>>,

    /// Configuration for dealing with high dynamic range input
    #[arg(long, default_value_t=HdrConfig::Preserve)]
    hdr_config: HdrConfig,
    /// Set to detect range of luminance in input
    #[arg(long)]
    hdr_autodetect_range: bool,
    /// Set range of luminance in input
    #[arg(long)]
    hdr_range: Option<MinMax>,
    // The fill option is currently disabled because there is only one choice at
    // the moment. Zebra would be the best choice in general.
    // /// Set the method to fill missing space when frame skipped.
    // #[arg(long, value_enum, default_value_t = FillMethod::Repeat)]
    // fill: FillMethod,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
enum Encoder {
    /// The less-avc uncompressed H264 encoder
    LessAvc,
    /// The openh264 encoder
    OpenH264,
    /// The Nvidia NVENC encoder
    NvEnc,
    /// Copy existing H264 stream
    NoneCopyExistingH264,
    // FfmpegH264,
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct MinMax {
    min: u16,
    max: u16,
}

struct ImageSequenceWriter {
    dirname: PathBuf,
    index: usize,
}

impl ImageSequenceWriter {
    fn write_dynamic(&mut self, frame: &DynamicFrame) -> Result<()> {
        use std::io::Write;

        let file = format!("frame{:05}.png", self.index);
        let fname = self.dirname.join(&file);
        let buf = match_all_dynamic_fmts!(
            frame,
            x,
            convert_image::frame_to_image(x, convert_image::ImageOptions::Png)
        )?;
        let mut fd = std::fs::File::create(fname)?;
        fd.write_all(&buf)?;
        self.index += 1;
        Ok(())
    }
}

enum FrameWriter<'a, T: std::io::Write + std::io::Seek> {
    Mp4(mp4_writer::Mp4Writer<'a, T>),
    Image(ImageSequenceWriter),
}

impl<'a, T: std::io::Write + std::io::Seek> FrameWriter<'a, T> {
    fn write_dynamic(
        &mut self,
        frame: &DynamicFrame,
        timestamp: chrono::DateTime<chrono::Utc>,
    ) -> Result<()> {
        match self {
            Self::Mp4(x) => x.write_dynamic(frame, timestamp)?,
            Self::Image(x) => x.write_dynamic(frame)?,
        }
        Ok(())
    }
    fn write_h264_buf(
        &mut self,
        data: &frame_source::H264EncodingVariant,
        width: u32,
        height: u32,
        timestamp: chrono::DateTime<chrono::Utc>,
        frame0_time: chrono::DateTime<chrono::Utc>,
        insert_precision_timestamp: bool,
    ) -> Result<()> {
        match self {
            Self::Mp4(x) => x.write_h264_buf(
                data,
                width,
                height,
                timestamp,
                frame0_time,
                insert_precision_timestamp,
            )?,
            Self::Image(_) => {
                anyhow::bail!("cannot decode individual h264 frame to image");
            }
        }
        Ok(())
    }
    fn finish(&mut self) -> Result<()> {
        match self {
            Self::Mp4(x) => x.finish()?,
            Self::Image(_) => {}
        }
        Ok(())
    }
}

impl std::str::FromStr for MinMax {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        let x: Vec<&str> = s.split('-').collect();
        if x.len() != 2 {
            anyhow::bail!("Could not parse MinMax: expected exactly one '-' character.");
        }
        let mins = x[0].trim();
        let maxs = x[1].trim();
        let min: u16 = mins.parse()?;
        let max: u16 = maxs.parse()?;
        Ok(Self { min, max })
    }
}

#[test]
fn test_min_max_from_str() {
    let x: MinMax = "1-2".parse().unwrap();
    assert_eq!(x, MinMax { min: 1, max: 2 });
}

impl std::fmt::Display for Encoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        let tmp = self.to_possible_value().unwrap();
        let s = tmp.get_name();
        write!(f, "{}", s)
    }
}

#[test]
fn test_encoder_get_name() {
    for e in Encoder::value_variants() {
        let name = format!("{e}");
        let e2 = Encoder::from_str(&name, false).unwrap();
        assert_eq!(e, &e2);
    }
}

enum TimingInfo {
    Ignore,
    Desired {
        desired_interval: Duration,
        desired_precision: Duration,
    },
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
enum FillMethod {
    // Zebra,
    // White,
    // Black,
    Repeat,
}

fn append_extension<P: AsRef<Path>>(input: P, extension: &str) -> PathBuf {
    let mut str = input.as_ref().as_os_str().to_os_string();
    str.push(extension);
    str.into()
}

fn init_logger() {
    use std::io::Write;
    // Set default level to info and change template not to show date/time or
    // level.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format(|buf, record| {
            let style = buf.style();
            writeln!(buf, "{}", style.value(record.args()))
        })
        .write_style(env_logger::WriteStyle::Auto)
        .init();
}

/// Deletes a file being written in case of error.
///
/// Call [Self::no_error] to keep the file. Otherwise, dropping this will cause
/// the file to be deleted.
struct DeleteOnError {
    path: PathBuf,
    do_delete: bool,
}

impl Drop for DeleteOnError {
    fn drop(&mut self) {
        if self.do_delete {
            if std::fs::metadata(&self.path).unwrap().is_file() {
                // unlink file
                std::fs::remove_file(&self.path).unwrap();
            } else {
                // unlink dir
                std::fs::remove_dir_all(&self.path).unwrap();
            }
            self.do_delete = false
        }
    }
}

impl DeleteOnError {
    fn new<P: AsRef<Path>>(p: P) -> Self {
        Self {
            path: p.as_ref().to_path_buf(),
            do_delete: true,
        }
    }

    fn no_error(mut self) {
        self.do_delete = false;
    }
}

fn abs_diff(a: Duration, b: Duration) -> Duration {
    if a > b {
        a - b
    } else {
        b - a
    }
}

trait DisplayMsecDuration {
    fn msec(&self) -> String;
}

impl DisplayMsecDuration for Duration {
    fn msec(&self) -> String {
        format!("{:.1}ms", self.as_secs_f64() * 1000.0)
    }
}

#[inline]
fn is_needed_now(
    next_dest_pts: Duration,
    next_src_pts: Duration,
    prev_dest_pts: Duration,
    timing_info: &TimingInfo,
) -> Result<bool> {
    if let TimingInfo::Desired {
        desired_interval,
        desired_precision,
    } = timing_info
    {
        let diff = abs_diff(next_dest_pts, next_src_pts);
        let result = diff < *desired_precision;
        log::debug!(
            "is_needed_now(next_dest_pts: {}, next_src_pts: {}, prev_dest_pts: {}, {}) -> next_src_pts: {} -> diff: {} -> result: {result}",
            next_dest_pts.msec(),next_src_pts.msec(),prev_dest_pts.msec(),desired_precision.msec(),next_src_pts.msec(),diff.msec(),
        );

        if (next_dest_pts - prev_dest_pts) > *desired_interval * 100 {
            anyhow::bail!("gap in source data of more than 100 frames (next_src_pts: {}, prev_dest_pts: {}, desired_interval: {})", next_src_pts.msec(), prev_dest_pts.msec(), desired_interval.msec());
        }

        Ok(result)
    } else {
        Ok(true)
    }
}

pub fn main() -> Result<()> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "info");
    }

    // env_logger::init();
    init_logger();
    let cli = Cli::parse();
    run_cli(cli)
}

pub fn run_cli(cli: Cli) -> Result<()> {
    if cli.encoder.is_some() && cli.export_pngs {
        anyhow::bail!("Cannot specify both mp4 encoder and export image sequence.");
    }

    #[allow(unused_assignments)]
    let mut nvenc_libs = None;

    let h264_bitrate = None;

    let input_path = PathBuf::from(&cli.input);

    let is_file = std::fs::metadata(&cli.input)?.is_file();

    // These variables prevent the original data source from being dropped while
    // the iterator over frames maintains only a reference to it.
    let mut src: Box<dyn FrameDataSource>;
    let default_encoder;
    let output_basename;
    let mut camera_name = None;
    let mut gamma = None;

    let writing_app = format!("{}-{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));

    log::info!("input: {}", cli.input);

    if is_file {
        output_basename = input_path.as_path().with_extension(""); // removes extension but keeps leading directory.

        let mut ext: Option<&str> = input_path.extension().map(|x| x.to_str()).flatten();
        if ext == Some("gz") {
            if let Some(input_path) = input_path.as_os_str().to_str() {
                if input_path.to_lowercase().ends_with(".fmf.gz") {
                    ext = Some("fmf.gz");
                }
            }
        }
        match ext {
            Some("mkv") => {
                let do_decode_h264 = cli.export_pngs || cli.skip.is_some();
                let mkv_video = strand_cam_mkv_source::from_path(&cli.input, do_decode_h264)?;
                let metadata = &mkv_video.parsed.metadata;
                camera_name = metadata.camera_name.clone();
                gamma = metadata.gamma;
                let encoder = if mkv_video.is_uncompressed() {
                    Encoder::LessAvc
                } else {
                    Encoder::NoneCopyExistingH264
                };
                log::debug!("  MKV video");
                src = Box::new(mkv_video);
                default_encoder = encoder;
            }
            Some("mp4") => {
                let mp4_video = mp4_source::from_path(&cli.input)?;
                if let Some(metadata) = &mp4_video.h264_metadata {
                    camera_name = metadata.camera_name.clone();
                    gamma = metadata.gamma;
                }
                log::debug!("  MP4 video");
                src = Box::new(mp4_video);
                default_encoder = Encoder::NoneCopyExistingH264;
            }
            Some("fmf") | Some("fmf.gz") => {
                let fmf_video = fmf_source::from_path(&cli.input)?;
                log::debug!("  FMF video");
                src = Box::new(fmf_video);
                default_encoder = Encoder::LessAvc;
            }
            _ => {
                anyhow::bail!(
                    "input {} is a file, but not a supported extension.",
                    cli.input
                );
            }
        }
    } else {
        use path_slash::PathBufExt;

        let dirname = PathBuf::from(&cli.input);

        // Special case to convert "/_1/Default/*.tif" -> "_1.mp4". This is the
        // default saved by micromanager.
        let path_str = dirname.to_slash().unwrap();
        const ONE_DEFAULT: &str = "/_1/Default";
        output_basename = if path_str.ends_with(ONE_DEFAULT) {
            PathBuf::from_slash(path_str.replace(ONE_DEFAULT, "/_1"))
        } else {
            dirname.clone()
        };

        if !std::fs::metadata(&cli.input)?.is_dir() {
            anyhow::bail!(
                "Attempting to open \"{}\" as directory with TIFF stack failed \
                because it is not a directory.",
                dirname.display()
            );
        }
        let pattern = dirname.join("*.tif");
        let stack = pv_tiff_stack::from_path_pattern(pattern.to_str().unwrap())?;
        log::debug!("  TIFF stack with {} files", stack.len());
        src = Box::new(stack);
        default_encoder = Encoder::LessAvc;
    }

    let width = src.width();
    let height = src.height();

    // If we need to skip some initial frames, do it before we compute FPS
    if let Some(skip) = cli.skip {
        log::info!("skipping {} initial input images", skip);
        src.skip_n_frames(skip)?;
    };

    let frame0_time = match cli.creation_time {
        Some(creation_time) => creation_time,
        None => {
            // Get frame0_time from source after skipping first frames.
            src.frame0_time().ok_or_else(|| {
                anyhow::anyhow!(
                    "No timestamp could be found for first frame, but this is \
                    required (hint: use --creation-time CLI arg)"
                )
            })?
        }
    };

    let frame0_time_utc = frame0_time.with_timezone(&chrono::Utc);
    let mut h264_metadata = H264Metadata::new(&writing_app, frame0_time);
    if let Some(ref camera_name) = camera_name {
        h264_metadata.camera_name = Some(camera_name.clone());
    }
    if let Some(ref gamma) = &gamma {
        h264_metadata.gamma = Some(*gamma);
    }

    let ignore_timing = if cli.export_pngs || cli.ignore_timing {
        true
    } else {
        false
    };

    let timing_info = if !ignore_timing {
        let desired_interval = if let Some(frame_interval_msec) = cli.frame_interval_msec {
            Duration::from_nanos((frame_interval_msec * 1_000_000.0) as u64)
        } else {
            let timestamps: Vec<Duration> = src
                .iter()
                .take(N_FRAMES_TO_COMPUTE_FPS)
                .map(|frame_data| frame_data.map(|x| x.timestamp()))
                .collect::<Result<Vec<Duration>>>()?;
            if timestamps.len() <= 1 {
                // at most only a single frame, so interval does not matter.
                Duration::from_nanos(1_000_000)
            } else {
                let deltas: Vec<f64> = ((&timestamps[1..])
                    .iter()
                    .zip(timestamps[..timestamps.len() - 1].iter()))
                .map(|(t1, t0)| t1.as_secs_f64() - t0.as_secs_f64())
                .collect();
                let deltas: Vec<NotNan<f64>> = deltas
                    .into_iter()
                    .map(NotNan::new)
                    .map(|r| r.map_err(|_e| anyhow::anyhow!("is nan")))
                    .collect::<Result<Vec<NotNan<f64>>>>()?;
                if deltas.len() == 1 {
                    Duration::from_secs_f64(deltas[0].into_inner())
                } else {
                    if cli.show_timestamps {
                        log::info!("While calculating inverval:");
                        for (fno, (delta, ts)) in deltas.iter().zip(timestamps).enumerate() {
                            log::info!(
                                "Frame {fno} for time: {} (interval to next: {}).",
                                ts.msec(),
                                Duration::from_secs_f64(delta.into_inner()).msec(),
                            );
                        }
                    }
                    let min_delta = deltas.iter().min().unwrap().into_inner();
                    let max_delta = deltas.iter().max().unwrap().into_inner();
                    if max_delta / min_delta > 1.05 {
                        anyhow::bail!("Cannot estimate frame interval reliably. Frame interval varies by more than 5%. Specify with `frame_interval_msec`.")
                    }
                    let sum_delta = deltas.iter().sum::<NotNan<f64>>().into_inner();
                    let avg_delta = Duration::from_secs_f64(sum_delta / deltas.len() as f64);
                    log::debug!(
                        "Average interval over first frames: {} ({} fps).",
                        avg_delta.msec(),
                        1.0 / avg_delta.as_secs_f64()
                    );
                    avg_delta
                }
            }
        };

        let desired_precision = if let Some(msec) = &cli.frame_interval_precision_msec {
            Duration::from_nanos((msec * 1_000_000.0) as u64)
        } else {
            desired_interval / 2
        };
        TimingInfo::Desired {
            desired_interval,
            desired_precision,
        }
    } else {
        TimingInfo::Ignore
    };

    let output_fname = if let Some(cli_output) = cli.output {
        cli_output
    } else {
        if !cli.export_pngs {
            append_extension(output_basename.as_path(), ".mp4")
        } else {
            output_basename.as_path().into()
        }
    };

    if !cli.export_pngs {
        if output_fname.as_path().extension() != Some(std::ffi::OsStr::new("mp4")) {
            anyhow::bail!("Will not continue. Output extension not .mp4");
        }
    }

    if !cli.overwrite {
        if output_fname.exists() {
            anyhow::bail!(
                "Will not continue, output exists: {}",
                output_fname.display()
            );
        }
    }

    if cli.export_pngs {
        std::fs::create_dir_all(&output_fname)?;
    }

    let mut hdr_lum_range = if cli.hdr_autodetect_range {
        let (min, max) = src.estimate_luminance_range()?;
        log::info!("  estimated luminance range in input: {min}-{max}",);
        Some((min, max))
    } else {
        None
    };

    if let Some(minmax) = cli.hdr_range {
        let MinMax { min, max } = minmax;
        hdr_lum_range = Some((min, max));
    }

    let mut stack_iter = src.iter();

    h264_metadata.creation_time = frame0_time.clone();

    let encoder = if cli.export_pngs {
        Encoder::LessAvc // this is a dummy value.
    } else {
        cli.encoder.clone().unwrap_or(default_encoder)
    };

    let (codec, libs_and_nv_enc) = match encoder {
        Encoder::NoneCopyExistingH264 => (ci2_remote_control::Mp4Codec::H264RawStream, None),
        Encoder::LessAvc => (ci2_remote_control::Mp4Codec::H264LessAvc, None),
        Encoder::OpenH264 => {
            let codec = ci2_remote_control::Mp4Codec::H264OpenH264({
                let preset = if let Some(bitrate) = h264_bitrate {
                    ci2_remote_control::OpenH264Preset::SkipFramesBitrate(bitrate)
                } else {
                    ci2_remote_control::OpenH264Preset::AllFrames
                };
                ci2_remote_control::OpenH264Options {
                    preset,
                    debug: false,
                }
            });
            (codec, None)
        }
        Encoder::NvEnc => {
            nvenc_libs = Some(nvenc::Dynlibs::new()?);
            let codec = ci2_remote_control::Mp4Codec::H264NvEnc(Default::default());
            (
                codec,
                Some(nvenc::NvEnc::new(nvenc_libs.as_ref().unwrap())?),
            )
        }
    };

    if let Some(take) = cli.take {
        log::info!("  limiting to {} input images", take);
        stack_iter = Box::new(stack_iter.take(take)) as Box<dyn Iterator<Item = Result<FrameData>>>;
    };

    let mut stack_iter = stack_iter.peekable();

    let n_src_frames_expected = stack_iter.size_hint().0;

    match &timing_info {
        TimingInfo::Desired {
            desired_interval,
            desired_precision: _,
        } =>
            log::info!(
        "size: {width}x{height}, start time: {frame0_time}, desired_interval: {} ({:.1} fps), num frames: {}",
        desired_interval.msec(),
        1.0 / desired_interval.as_secs_f64(), n_src_frames_expected,
    ),
        TimingInfo::Ignore =>
            log::info!("size: {width}x{height}, start time: {frame0_time}, num frames: {}",n_src_frames_expected),

    }

    // Custom progress bar with space at right end to prevent obscuring last
    // digit with cursor.
    let style = ProgressStyle::with_template("{wide_bar} {pos}/{len} ETA: {eta} ")?;
    let pb = ProgressBar::new(n_src_frames_expected.try_into()?).with_style(style);

    // load first file
    let read_start = std::time::Instant::now();
    let image0 = stack_iter
        .peek()
        .unwrap()
        .as_ref()
        .map_err(|e| anyhow::anyhow!("Error peeking at first frame: {e}"))?;

    if image0.timestamp().as_secs_f64() != 0.0 {
        anyhow::bail!("Failed expectation that timestamp of first frame is 0");
    }
    // let black_frame =
    //     SimpleFrame::new(width, height, width, vec![0; (width * height) as usize]).unwrap();
    // let white_frame =
    //     SimpleFrame::new(width, height, width, vec![255; (width * height) as usize]).unwrap();
    // let zebra_frame = {
    //     let mut image_data = vec![0; (width * height) as usize];
    //     let stripe_width = width.min(height) as usize / 20;
    //     for (i, row) in image_data.chunks_exact_mut(width as usize).enumerate() {
    //         for j in 0..row.len() {
    //             let val = if ((j + i) % (stripe_width * 2)) > stripe_width {
    //                 255u8
    //             } else {
    //                 0u8
    //             };
    //             row[j] = val;
    //         }
    //     }
    //     SimpleFrame::new(width, height, width, image_data).unwrap()
    // };

    // ---------

    let mut output_writer = if cli.export_pngs {
        FrameWriter::Image(ImageSequenceWriter {
            dirname: output_fname.clone(),
            index: 0,
        })
    } else {
        log::debug!(
            "Saving metadata: {}",
            serde_json::to_string(&serde_json::to_value(&h264_metadata)?)?
        );

        let sample_duration = if let TimingInfo::Desired {
            desired_interval,
            desired_precision: _,
        } = &timing_info
        {
            desired_interval.clone()
        } else {
            let (arbitrarily, frame_interval_msec) = if let Some(msec) = cli.frame_interval_msec {
                ("as requested ", msec)
            } else {
                ("arbitrarily ", 20.0)
            };
            let duration = std::time::Duration::from_secs_f64(frame_interval_msec / 1000.0);
            log::warn!(
                "As requested, ignoring timing data. However, MP4 requires a \
            sample duration for each frame. This is set {arbitrarily}to {}.",
                duration.msec()
            );
            duration
        };

        let mp4_cfg = ci2_remote_control::Mp4RecordingConfig {
            codec,
            sample_duration,
            max_framerate: Default::default(),
            h264_metadata: Some(h264_metadata),
        };

        let out_fd = std::fs::File::create(&output_fname)
            .with_context(|| format!("writing to {}", output_fname.display()))?;
        FrameWriter::Mp4(mp4_writer::Mp4Writer::new(
            out_fd,
            mp4_cfg,
            libs_and_nv_enc,
        )?)
    };

    let mut delete_on_error = DeleteOnError::new(&output_fname);
    if cli.truncate_on_error {
        delete_on_error.do_delete = false;
    }

    // ---------

    let mut out_fno: u32 = 0;
    let mut next_dest_pts = Duration::from_nanos(0);

    let mut no_progress = cli.no_progress;
    if cli.show_timestamps {
        no_progress = true;
    }

    // Increment the counter from the first file (above). We do this here to be
    // after the various print statements that come in the start.
    if !no_progress {
        pb.inc(1);
    }

    let mut n_missing_frames = 0;
    let mut src_count = 0; // read one image
    let mut bytes_read = 0;
    let mut prev_frame = image0.image().clone();
    let mut peeked_into_error = false;
    let mut prev_dest_pts = image0.timestamp();

    while let Some(result_peek_source_data) = stack_iter.peek() {
        // If we will get an error, break and handle it.
        let peek_source_data = match result_peek_source_data.as_ref() {
            Ok(peek_source_data) => peek_source_data,
            Err(_) => {
                peeked_into_error = true;
                break;
            }
        };

        let next_src_pts = peek_source_data.timestamp();
        // Check if next available source image is what we need or if it is too far in the future.
        let (save_frame, save_elapsed) =
            if is_needed_now(next_dest_pts, next_src_pts, prev_dest_pts, &timing_info)
                .with_context(|| {
                    format!(
                        "while reading source frame {} of {}",
                        peek_source_data.idx(),
                        n_src_frames_expected,
                    )
                })?
            {
                if cli.show_timestamps {
                    log::info!(
                        "Output frame {out_fno} for time: {} (source frame: {}, source time: {}).",
                        next_dest_pts.msec(),
                        peek_source_data.idx(),
                        next_src_pts.msec()
                    );
                }

                // Use this source frame. (We know we can unwrap because we peeked.)
                let this_data = stack_iter.next().unwrap()?;
                src_count += 1;
                bytes_read += this_data.num_bytes();
                if !no_progress {
                    pb.inc(1);
                }

                log::debug!(
                    "Output frame {out_fno} from source frame {} at {}",
                    src_count,
                    this_data.timestamp().msec(),
                );

                prev_dest_pts = this_data.timestamp();
                let frame_elapsed = this_data.timestamp();
                prev_frame = this_data.into_image();
                (&prev_frame, frame_elapsed)
            } else {
                if cli.show_timestamps {
                    log::warn!(
                    "Output frame {out_fno} missing for time: {}. (Next source idx: {}, time: {})",
                    next_dest_pts.msec(),
                    peek_source_data.idx(),
                    peek_source_data.timestamp().msec(),
                );
                }

                log::debug!("Output frame {out_fno} missing from source");
                n_missing_frames += 1;

                // frame is missing in input data
                (&prev_frame, next_dest_pts)
                // Currently disabled:
                // match cli.fill {
                //     // FillMethod::Zebra => (zebra_frame.clone(), out_elapsed),
                //     // FillMethod::White => (white_frame.clone(), out_elapsed),
                //     // FillMethod::Black => (black_frame.clone(), out_elapsed),
                //     FillMethod::Repeat => (prev_frame.clone(), out_elapsed),
                // }
            };

        let frame_timestamp_tz = frame0_time + chrono::Duration::from_std(save_elapsed)?;
        let frame_timestamp_utc = frame_timestamp_tz.with_timezone(&chrono::Utc);
        match &save_frame {
            ImageData::Tiff(tiff_image) => {
                let frame = tiff_decoder::read_tiff_image(
                    tiff_image,
                    frame0_time,
                    out_fno.try_into().unwrap(),
                    &cli.hdr_config,
                    hdr_lum_range,
                )?;
                output_writer.write_dynamic(&frame, frame_timestamp_utc)?;
            }
            ImageData::Decoded(frame) => {
                output_writer.write_dynamic(&frame, frame_timestamp_utc)?;
            }
            ImageData::EncodedH264(encoded_h264) => {
                output_writer
                    .write_h264_buf(
                        &encoded_h264.data,
                        width,
                        height,
                        frame_timestamp_utc,
                        frame0_time_utc,
                        !encoded_h264.has_precision_timestamp,
                    )
                    .with_context(|| "while writing raw h264 buffer")?;
            }
        }

        // update desired for next frame
        out_fno += 1;
        if let TimingInfo::Desired {
            desired_interval,
            desired_precision: _,
        } = &timing_info
        {
            next_dest_pts = *desired_interval * out_fno;
        }
    }

    if !no_progress {
        pb.finish_and_clear();
    }

    if peeked_into_error {
        // raise the error we saw coming
        stack_iter.next().unwrap()?;
    }

    // Done reading. Summarize reading.

    {
        let elapsed = read_start.elapsed();
        let bytes_per_sec = bytes_read as f64 / elapsed.as_secs_f64();

        log::info!(
            "Processing statistics: read {} images, {} in {} ({} per second). {n_missing_frames} missing frames.",
            src_count,
            HumanBytes(bytes_read as u64),
            HumanDuration(elapsed),
            HumanBytes(bytes_per_sec as u64),
        );
    }

    // stop saving
    output_writer.finish()?;
    std::mem::drop(output_writer);

    // Done writing. Summarize writing.

    {
        let out_bytes = std::fs::metadata(&output_fname)?.len();
        let out_bytes_per_second = out_bytes as f64 / next_dest_pts.as_secs_f64();
        let fps = out_fno as f64 / prev_dest_pts.as_secs_f64();

        log::info!(
            "Saved movie statistics: {out_fno} frames, codec: H264, encoder: {encoder}, size: {}, duration: {}, fps: {:.1}, byterate: {}, filename: {}",
            HumanBytes(out_bytes as u64),
            HumanDuration(prev_dest_pts),
            fps,
            HumanBytes(out_bytes_per_second as u64),
            output_fname.display(),
        );
    }

    delete_on_error.no_error();
    Ok(())
}
