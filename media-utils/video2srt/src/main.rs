use clap::Parser;
use eyre::{self, WrapErr};
use srt_writer::BufferingSrtFrameWriter;
use std::path::PathBuf;

use frame_source::Timestamp;

#[derive(Debug, Parser)]
#[command(version, about)]
struct Opt {
    /// Input video filename.
    #[arg(short, long)]
    input: PathBuf,

    /// Output srt filename. Defaults to "<INPUT>.srt"
    #[arg(short, long)]
    output: Option<PathBuf>,
}

fn main() -> eyre::Result<()> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "info");
    }
    env_tracing_logger::init();
    let opt = Opt::parse();

    let output = opt.output;

    let output = output.unwrap_or_else(|| {
        let mut output = opt.input.clone();
        output.set_extension(".srt");
        output
    });

    let do_decode_h264 = false;
    let mut src = frame_source::from_path(&opt.input, do_decode_h264)
        .with_context(|| format!("while opening path {}", opt.input.display()))?;

    let start_time = src
        .frame0_time()
        .ok_or_else(|| eyre::eyre!("no start time found"))?;

    let out_fd = std::fs::File::create(&output)?;
    let mut wtr = BufferingSrtFrameWriter::new(Box::new(out_fd));

    for frame in src.iter() {
        let frame = frame?;
        let pts = match frame.timestamp() {
            Timestamp::Duration(pts) => pts,
            _ => {
                eyre::bail!("video has no PTS timestamps.");
            }
        };
        let frame_stamp = start_time + pts;
        wtr.add_frame(pts, format!("{frame_stamp}"))?;
    }

    Ok(())
}
