// Copyright 2022-2023 Andrew D. Straw.

use clap::{Parser, ValueEnum};
use eyre::{self, Result};
use indicatif::{ProgressBar, ProgressStyle};

trait DisplayTimestamp {
    fn to_display(&self) -> String;
}

impl DisplayTimestamp for frame_source::Timestamp {
    fn to_display(&self) -> String {
        match self {
            frame_source::Timestamp::Duration(dur) => {
                format!("{:9.1}ms", dur.as_secs_f64() * 1000.0)
            }
            frame_source::Timestamp::Fraction(frac) => {
                format!("{:2.1}%", frac * 100.0)
            }
        }
    }
}

impl DisplayTimestamp for std::time::Duration {
    fn to_display(&self) -> String {
        frame_source::Timestamp::Duration(*self).to_display()
    }
}

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Cli {
    /// Inputs. Either files (e.g. `file.mp4`) or TIFF image directories. The
    /// first TIFF file in a TIFF image directory is also accepted.
    ///
    /// For a TIFF image directory, images will be ordered alphabetically.
    #[arg(required=true, num_args=1..)]
    inputs: Vec<String>,

    /// Output format.
    #[arg(long, value_enum, default_value_t)]
    output: OutputFormat,

    /// Source of timestamp.
    #[arg(long, value_enum, default_value_t)]
    timestamp_source: TimestampSource,

    /// Show progress
    #[arg(short, long, default_value_t)]
    progress: bool,
}

#[derive(Default, Debug, Clone, ValueEnum)]
enum OutputFormat {
    #[default]
    /// Print a summary in human-readable format.
    Summary,
    /// Print a line for every frame in human-readable format.
    EveryFrame,
    /// Print as comma-separated values with a row for every frame.
    Csv,
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

impl std::fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        match self {
            Self::EveryFrame => write!(f, "every frame (human-readable)"),
            Self::Summary => write!(f, "summary (human-readable)"),
            Self::Csv => write!(f, "CSV (Comma Separated Values)"),
        }
    }
}

fn main() -> Result<()> {
    env_tracing_logger::init();
    let cli = Cli::parse();

    for input in cli.inputs.iter() {
        let mut input_path = std::path::PathBuf::from(input);
        let mut srt_file_path = None;
        let is_file = std::fs::metadata(&input_path)?.is_file();
        if is_file {
            let file_ext = input_path
                .extension()
                .and_then(|x| x.to_str())
                .map(|x| x.to_lowercase());
            if file_ext == Some("tif".into()) || file_ext == Some("tiff".into()) {
                // tif file - assume this is image sequence and use directory.
                input_path.pop();
            }

            if file_ext == Some("mp4".into()) {
                let mut srt_path = input_path.clone();
                srt_path.set_extension("srt");
                if srt_path.exists() && std::fs::metadata(&srt_path)?.is_file() {
                    srt_file_path = Some(srt_path);
                } else if cli.timestamp_source == TimestampSource::SrtFile {
                    eyre::bail!(
                        "Source specified as SRT file, but {} is not a file.",
                        srt_path.display()
                    );
                }
            }
        }

        if cli.progress {
            eprintln!("Performing initial open of \"{}\".", input_path.display());
        }

        let do_decode_h264 = false; // no need to decode h264 to get timestamps.
        let mut src = frame_source::from_path_with_srt_timestamp_source(
            &input_path,
            do_decode_h264,
            cli.timestamp_source.into(),
            srt_file_path,
        )?;

        if cli.progress {
            eprintln!("Done with initial open.");
        }

        let mut pb: Option<ProgressBar> = if cli.progress {
            let (lower_bound, _upper_bound) = src.iter().size_hint();

            // Custom progress bar with space at right end to prevent obscuring last
            // digit with cursor.
            let style = ProgressStyle::with_template("{wide_bar} {pos}/{len} ETA: {eta} ")?;
            Some(ProgressBar::new(lower_bound.try_into().unwrap()).with_style(style))
        } else {
            None
        };

        let has_timestamps = src.has_timestamps();

        let start_time = src.frame0_time();

        let start_time_str = start_time
            .as_ref()
            .map(|x| format!("{x}"))
            .unwrap_or_else(|| "(unknown)".to_string());
        match cli.output {
            OutputFormat::EveryFrame | OutputFormat::Summary => {
                println!("Path: {}", input_path.display());
                println!(
                    "  Start time: {}, Dimensions: {}x{}, Timestamp source: {:?}",
                    start_time_str,
                    src.width(),
                    src.height(),
                    src.timestamp_source(),
                );
            }
            OutputFormat::Csv => {
                println!(
                    "# Path:{}, Start time: {}, Dimensions: {}x{}, Timestamp source: {:?}",
                    input_path.display(),
                    start_time_str,
                    src.width(),
                    src.height(),
                    src.timestamp_source(),
                );
                let col_name = if has_timestamps {
                    "timestamp_msec"
                } else {
                    "fraction"
                };
                println!("frame_idx,{col_name}");
            }
        }

        let mut prev_timestamp: Option<frame_source::Timestamp> = None;

        let mut count = 0;
        for frame in src.iter() {
            let frame = frame?;

            if let Some(pb) = pb.as_mut() {
                pb.inc(1);
            }

            match cli.output {
                OutputFormat::EveryFrame => {
                    let delta =
                        if let (Some(prev_timestamp), frame_source::Timestamp::Duration(t)) =
                            (prev_timestamp, frame.timestamp())
                        {
                            let total_str = if let Some(start_time) = start_time.as_ref() {
                                let stamp_chrono = *start_time + t;
                                format!(
                                    " datetime {}, since 1970-01-01 {}",
                                    stamp_chrono,
                                    datetime_conversion::datetime_to_f64(&stamp_chrono)
                                )
                            } else {
                                String::new()
                            };
                            let delta = t - prev_timestamp.unwrap_duration();
                            format!("    (delta: {}){total_str}", delta.to_display())
                        } else {
                            String::new()
                        };
                    println!(
                        "    {:5}: {:10}{}",
                        frame.idx(),
                        frame.timestamp().to_display(),
                        delta,
                    );
                }
                OutputFormat::Csv => {
                    let time_val = match frame.timestamp() {
                        frame_source::Timestamp::Duration(dur) => {
                            format!("{}", dur.as_secs_f64() * 1000.0)
                        }
                        frame_source::Timestamp::Fraction(frac) => format!("{}", frac),
                    };
                    println!("{},{time_val}", frame.idx());
                }
                OutputFormat::Summary => {}
            }
            prev_timestamp = Some(frame.timestamp());
            count += 1;
        }

        match cli.output {
            OutputFormat::EveryFrame | OutputFormat::Summary => {
                if let Some(frame_source::Timestamp::Duration(prev_timestamp)) = prev_timestamp {
                    if count > 0 {
                        let fps = count as f64 / prev_timestamp.as_secs_f64();
                        println!(
                            "  {} frames in {}: {:.2} frames per second",
                            count,
                            prev_timestamp.to_display(),
                            fps
                        );
                    }
                }
            }
            _ => {}
        }
        if let Some(pb) = pb {
            pb.finish_and_clear();
        }
    }

    Ok(())
}
