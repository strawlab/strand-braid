use clap::Parser;
use color_eyre::eyre::{self, WrapErr};
use std::{io::Write, path::PathBuf};

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

trait Srt {
    fn srt(&self) -> String;
}

impl Srt for std::time::Duration {
    fn srt(&self) -> String {
        // from https://en.wikipedia.org/wiki/SubRip :
        // "hours:minutes:seconds,milliseconds with time units fixed to two
        // zero-padded digits and fractions fixed to three zero-padded digits
        // (00:00:00,000). The fractional separator used is the comma, since the
        // program was written in France."
        let total_secs = self.as_secs();
        let hours = total_secs / (60 * 60);
        let minutes = (total_secs % (60 * 60)) / 60;
        let seconds = total_secs % 60;
        dbg!(total_secs);
        dbg!(hours);
        dbg!(minutes);
        dbg!(seconds);
        debug_assert_eq!(total_secs, hours * 60 * 60 + minutes * 60 + seconds);
        let millis = self.subsec_millis();
        format!("{hours:02}:{minutes:02}:{seconds:02},{millis:03}")
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
        output.push(".srt");
        output.into()
    });

    let do_decode_h264 = false;
    let mut src = frame_source::from_path(&opt.input, do_decode_h264)
        .with_context(|| format!("while opening path {}", opt.input.display()))?;

    let start_time = src
        .frame0_time()
        .ok_or_else(|| eyre::eyre!("no start time found"))?;

    let mut out_fd = std::fs::File::create(&output)?;

    let mut prev_data: Option<(std::time::Duration, _)> = None;

    let mut count = 1;
    for (_fno, frame) in src.iter().enumerate() {
        let frame = frame?;
        let pts = match frame.timestamp() {
            Timestamp::Duration(pts) => pts,
            _ => {
                eyre::bail!(
                    "video has no PTS timestamps and framerate was not \
                    specified on the command line."
                );
            }
        };
        let frame_stamp = start_time + pts;
        if let Some((prev_pts, prev_stamp)) = prev_data.take() {
            out_fd.write_all(
                format!(
                    "{}\n{} --> {}\n{}\n\n",
                    count,
                    prev_pts.srt(),
                    pts.srt(),
                    prev_stamp
                )
                .as_bytes(),
            )?;
            count += 1;
        }
        prev_data = Some((pts, frame_stamp));
    }

    if let Some((prev_pts, prev_stamp)) = prev_data.take() {
        let pts = prev_pts + std::time::Duration::from_secs(1);
        out_fd.write_all(
            format!(
                "{}\n{} --> {}\n{}\n\n",
                count,
                prev_pts.srt(),
                pts.srt(),
                prev_stamp
            )
            .as_bytes(),
        )?;
    }

    Ok(())
}
