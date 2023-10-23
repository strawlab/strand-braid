use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Cli {
    src: PathBuf,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut archive = zip_or_dir::ZipDirArchive::auto_from_path(&cli.src)?;
    let mut start = None;
    let chunk_dur = std::time::Duration::from_millis(1000);
    for chunk in braidz_chunked_iter::chunk_by_duration(&mut archive, chunk_dur)? {
        println!("---- duration chunk ----");
        for row in chunk.rows {
            if start.is_none() {
                start = Some(row.timestamp.as_ref().unwrap().as_f64());
            }
            let t0 = start.unwrap();
            println!(
                "  frame: {}, timestamp: {}",
                row.frame.0,
                row.timestamp.unwrap().as_f64() - t0
            );
        }
    }
    Ok(())
}
