// Copyright 2022-2023 Andrew D. Straw.

use std::time::Duration;

use anyhow::Result;
use clap::Parser;

trait DisplayMsecDuration {
    fn msec(&self) -> String;
}

impl DisplayMsecDuration for Duration {
    fn msec(&self) -> String {
        format!("{:9.1}ms", self.as_secs_f64() * 1000.0)
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
        println!("    {:5}: {}", frame.idx(), frame.timestamp().msec());
    }

    Ok(())
}
