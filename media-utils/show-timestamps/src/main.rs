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
    let cli = Cli::parse();

    println!("File: {}", cli.input);
    let do_decode_h264 = false; // no need to decode h264 to get timestamps.
    let mut src = frame_source::from_path(&cli.input, do_decode_h264)?;
    println!(
        "  Start time: {}, Dimensions: {}x{}",
        src.frame0_time()
            .map(|x| format!("{x}"))
            .unwrap_or_else(|| "(unknown)".to_string()),
        src.width(),
        src.height(),
    );

    for frame in src.iter() {
        let frame = frame?;
        println!("    {:5}: {}", frame.idx(), frame.timestamp().to_display());
    }

    Ok(())
}
