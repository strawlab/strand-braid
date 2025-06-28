// Copyright 2022-2023 Andrew D. Straw.

use clap::{Parser, ValueEnum};
use eyre::{self, Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};

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

// TODO: define SrtMsg only once in this codebase.
#[derive(Serialize, Deserialize)]
struct SrtMsg {
    timestamp: chrono::DateTime<chrono::Local>,
}

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Cli {
    /// Inputs. Either files (e.g. `file.mp4`) or TIFF image directories. The
    /// first TIFF file in a TIFF image directory is also accepted.
    ///
    /// For a TIFF image directory, images will be ordered alphabetically.
    #[arg(required=true, num_args=1..)]
    inputs: Vec<camino::Utf8PathBuf>,

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
    /// Print as SubRip subtitle file (.srt).
    Srt,
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
            Self::Srt => write!(f, "SRT (SubRip Subtitle)"),
        }
    }
}

fn main() -> Result<()> {
    // TODO: as we want stdout, configure tracing to log to stderr.
    env_tracing_logger::init();
    let cli = Cli::parse();

    for mut input_path in cli.inputs.into_iter() {
        let mut srt_file_path = None;
        let is_file = std::fs::metadata(&input_path)
            .with_context(|| format!("While opening input {input_path}"))?
            .is_file();
        if is_file {
            let file_ext = input_path.extension().map(|x| x.to_lowercase());

            if file_ext == Some("tif".into()) || file_ext == Some("tiff".into()) {
                // tif file - assume this is image sequence and use directory.
                input_path.pop();
            }

            if file_ext == Some("mp4".into()) {
                let mut srt_path = input_path.clone();
                srt_path.set_extension("srt");
                if srt_path.exists() && std::fs::metadata(&srt_path)?.is_file() {
                    match cli.output {
                        OutputFormat::Srt => {
                            tracing::debug!(
                                "Ignoring existing SRT file {srt_path} because output is \
                            SRT and we may be piping to it."
                            );
                            // Presumably if the user wants an SRT file with
                            // timestamps, they can use the original.
                        }
                        _ => {
                            srt_file_path = Some(srt_path);
                        }
                    };
                } else if cli.timestamp_source == TimestampSource::SrtFile {
                    eyre::bail!("Source specified as SRT file, but {srt_path} is not a file.");
                }
            }
        }

        if cli.progress {
            eprintln!("Performing initial open of \"{input_path}\".");
        }

        let mut src = frame_source::FrameSourceBuilder::new(&input_path)
            .do_decode_h264(false) // no need to decode h264 to get timestamps.
            .timestamp_source(cli.timestamp_source.into())
            .srt_file_path(srt_file_path.map(Into::into))
            .show_progress(cli.progress)
            .build_source()?;

        if cli.progress {
            eprintln!("Done with initial open.");
        }

        let mut pb: Option<ProgressBar> = if cli.progress {
            let (lower_bound, _upper_bound) = src.iter().size_hint();

            // Custom progress bar with space at right end to prevent obscuring last
            // digit with cursor.
            let style = ProgressStyle::with_template(
                "Loading timestamps {wide_bar} {pos}/{len} ETA: {eta} ",
            )?;
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
        let mut srt_wtr = None;
        match cli.output {
            OutputFormat::EveryFrame | OutputFormat::Summary => {
                println!("Path: {input_path}");
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
                    "# Path:{input_path}, Start time: {start_time_str}, Dimensions: {w}x{h}, Timestamp source: {tss:?}",
                    w=src.width(),
                    h=src.height(),
                    tss=src.timestamp_source(),
                );
                let col_name = if has_timestamps {
                    "timestamp_msec"
                } else {
                    "fraction"
                };
                println!("frame_idx,{col_name}");
            }
            OutputFormat::Srt => {
                let stdout = std::io::stdout();
                srt_wtr = Some(srt_writer::BufferingSrtFrameWriter::new(Box::new(stdout)));
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
                                    strand_datetime_conversion::datetime_to_f64(&stamp_chrono)
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
                OutputFormat::Srt => {
                    match frame.timestamp() {
                        frame_source::Timestamp::Duration(dur) => {
                            if let Some(start_time) = start_time.as_ref() {
                                let stamp_chrono = *start_time + dur;
                                let msg = SrtMsg {
                                    timestamp: stamp_chrono.into(),
                                };
                                let msg_str = serde_json::to_string(&msg)?;
                                srt_wtr.as_mut().unwrap().add_frame(dur, msg_str)?;
                            } else {
                                eyre::bail!("No start time available for SRT output.");
                            }
                        }
                        frame_source::Timestamp::Fraction(_frac) => {
                            eyre::bail!("SRT output does not support fractional timestamps.")
                        }
                    };
                }
                OutputFormat::Summary => {}
            }
            prev_timestamp = Some(frame.timestamp());
            count += 1;
        }

        if let Some(srt_wtr) = srt_wtr.take() {
            // Finish writing SRT file. (This would happen anyway when srt_wtr
            // goes out of scope, but this gives us a chance to catch errors
            // without panicking.)
            srt_wtr.close()?;
        }

        if let Some(pb) = pb {
            pb.finish_and_clear();
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
    }

    Ok(())
}
