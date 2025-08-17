// Copyright 2025 Andrew D. Straw.

use camino::Utf8PathBuf;
use clap::{Parser, ValueEnum};
use eyre::{self, Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};

use rusttype::Font;

use font_drawing::stamp_frame;

// trait DisplayTimestamp {
//     fn to_display(&self) -> String;
// }

// impl DisplayTimestamp for frame_source::Timestamp {
//     fn to_display(&self) -> String {
//         match self {
//             frame_source::Timestamp::Duration(dur) => {
//                 format!("{:9.1}ms", dur.as_secs_f64() * 1000.0)
//             }
//             frame_source::Timestamp::Fraction(frac) => {
//                 format!("{:2.1}%", frac * 100.0)
//             }
//         }
//     }
// }

// impl DisplayTimestamp for std::time::Duration {
//     fn to_display(&self) -> String {
//         frame_source::Timestamp::Duration(*self).to_display()
//     }
// }

// TODO: define SrtMsg only once in this codebase.
#[derive(Serialize, Deserialize)]
struct SrtMsg {
    timestamp: chrono::DateTime<chrono::Local>,
}

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Cli {
    /// Input MP4 video.
    #[arg(long)]
    input: camino::Utf8PathBuf,

    /// Output MP4 file.
    #[arg(long)]
    output: Utf8PathBuf,

    /// Source of timestamp.
    #[arg(long, value_enum, default_value_t)]
    timestamp_source: TimestampSource,

    /// Disable showing progress
    #[arg(short, long, default_value_t)]
    no_progress: bool,
}

#[derive(Default, Debug, Clone, Copy, ValueEnum, PartialEq)]
enum TimestampSource {
    #[default]
    BestGuess,
    FrameInfoRecvTime,
    Mp4Pts,
    MispMicrosectime,
    SrtFile,
}

impl From<TimestampSource> for frame_source::TimestampSource {
    fn from(orig: TimestampSource) -> Self {
        match orig {
            TimestampSource::BestGuess => frame_source::TimestampSource::BestGuess,
            TimestampSource::FrameInfoRecvTime => frame_source::TimestampSource::FrameInfoRecvTime,
            TimestampSource::SrtFile => frame_source::TimestampSource::SrtFile,
            TimestampSource::Mp4Pts => frame_source::TimestampSource::Mp4Pts,
            TimestampSource::MispMicrosectime => frame_source::TimestampSource::MispMicrosectime,
        }
    }
}

fn main() -> Result<()> {
    env_tracing_logger::init();
    let cli = Cli::parse();

    // Load the font
    // let font_data = include_bytes!("../Roboto-Regular.ttf");
    let font_data = ttf_firacode::REGULAR;
    // This only succeeds if collection consists of one font
    let font = Font::try_from_bytes(font_data as &[u8]).expect("Error constructing Font");

    let input_path = cli.input;

    let mut srt_file_path = None;
    let is_file = std::fs::metadata(&input_path)
        .with_context(|| format!("While opening input {input_path}"))?
        .is_file();
    if !is_file {
        eyre::bail!("Input path {input_path} is not a file.");
    }
    let file_ext = input_path.extension().map(|x| x.to_lowercase());

    if file_ext != Some("mp4".into()) {
        eyre::bail!("Input file must be an MP4 file.");
    }

    let mut srt_path = input_path.clone();
    srt_path.set_extension("srt");
    if srt_path.exists() && std::fs::metadata(&srt_path)?.is_file() {
        srt_file_path = Some(srt_path);
    } else if cli.timestamp_source == TimestampSource::SrtFile {
        eyre::bail!("Source specified as SRT file, but {srt_path} is not a file.");
    }

    if !cli.no_progress {
        eprintln!("Performing initial open of \"{input_path}\".");
    }

    let mut src = frame_source::FrameSourceBuilder::new(&input_path)
        .timestamp_source(cli.timestamp_source.into())
        .srt_file_path(srt_file_path.map(Into::into))
        .show_progress(!cli.no_progress)
        .build_source()?;

    let t0: chrono::DateTime<chrono::Utc> = src.frame0_time().unwrap().into();
    if !cli.no_progress {
        eprintln!("Done with initial open.");
    }

    let mut ffmpeg_wtr =
        ffmpeg_writer::FfmpegWriter::new(cli.output.as_str(), Default::default(), None)?;

    let mut pb: Option<ProgressBar> = if !cli.no_progress {
        let (lower_bound, _upper_bound) = src.iter().size_hint();

        // Custom progress bar with space at right end to prevent obscuring last
        // digit with cursor.
        let style =
            ProgressStyle::with_template("Burning timestamps {wide_bar} {pos}/{len} ETA: {eta} ")?;
        Some(ProgressBar::new(lower_bound.try_into().unwrap()).with_style(style))
    } else {
        None
    };

    for (idx, frame) in src.iter().enumerate() {
        let frame = frame?;

        if let Some(pb) = pb.as_mut() {
            pb.inc(1);
        }

        let im = if let Some(im) = frame.decoded() {
            im
        } else {
            eyre::bail!("Frame {idx} has no decoded image data.",);
        };

        let text = match frame.timestamp() {
            frame_source::Timestamp::Duration(dur) => {
                format!(
                    "{}",
                    (t0 + dur).to_rfc3339_opts(chrono::format::SecondsFormat::Millis, true)
                )
            }
            frame_source::Timestamp::Fraction(frac) => {
                format!("{:2.1}%", frac * 100.0)
            }
        };

        let mut frame_rgb8 = im.into_pixel_format()?.owned();
        stamp_frame(&mut frame_rgb8, &font, &text)?;

        let dy_im = strand_dynamic_frame::DynamicFrame::from_static_ref(&frame_rgb8);

        ffmpeg_wtr.write_dynamic_frame(&dy_im)?;
    }

    ffmpeg_wtr.close()?;
    if let Some(pb) = pb {
        pb.finish_and_clear();
    }

    Ok(())
}
