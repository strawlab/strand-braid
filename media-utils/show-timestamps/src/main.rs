// Copyright 2022-2023 Andrew D. Straw.

use anyhow::Result;
use clap::Parser;

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
pub struct Cli {
    /// Input. Either TIFF image directory (`/path/to/tifs/`) or `file.mkv`.
    ///
    /// For a TIFF image directory, images will be ordered alphabetically.
    #[arg(short, long)]
    input: String,
}

fn main() -> Result<()> {
    env_tracing_logger::init();
    let cli = Cli::parse();

    println!("File: {}", cli.input);
    let do_decode_h264 = false; // no need to decode h264 to get timestamps.
    let mut src = frame_source::from_path(&cli.input, do_decode_h264)?;
    println!(
        "  Start time: {}, Dimensions: {}x{}, Timestamp source: {:?}",
        src.frame0_time()
            .map(|x| format!("{x}"))
            .unwrap_or_else(|| "(unknown)".to_string()),
        src.width(),
        src.height(),
        src.timestamp_source(),
    );

    let mut prev_timestamp: Option<frame_source::Timestamp> = None;

    let mut count = 0;
    for frame in src.iter() {
        let frame = frame?;
        let delta = if let (Some(prev_timestamp), frame_source::Timestamp::Duration(t)) =
            (prev_timestamp, frame.timestamp())
        {
            let delta = t - prev_timestamp.unwrap_duration();
            format!("    (delta: {})", delta.to_display())
        } else {
            String::new()
        };
        println!(
            "    {:5}: {:10}{}",
            frame.idx(),
            frame.timestamp().to_display(),
            delta,
        );
        prev_timestamp = Some(frame.timestamp());
        count += 1;
    }

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

    Ok(())
}
