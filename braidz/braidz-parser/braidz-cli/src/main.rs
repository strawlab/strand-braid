use anyhow::Context;
use clap::Parser;
use std::path::PathBuf;

mod frame_by_frame;

#[derive(Debug, Parser)]
#[command(author, version)]
struct Opt {
    /// Input braidz filename
    input: PathBuf,

    /// print all data in the `data2d_distorted` table
    #[arg(short, long)]
    data2d_distorted: bool,

    /// print frame-by-frame view of all data
    #[arg(short, long)]
    frame_by_frame: bool,
}

fn main() -> anyhow::Result<()> {
    env_tracing_logger::init();
    let opt = Opt::parse();
    let attr = std::fs::metadata(&opt.input)
        .with_context(|| format!("Getting file metadata for {}", opt.input.display()))?;

    let mut archive = braidz_parser::braidz_parse_path(&opt.input)
        .with_context(|| format!("Parsing file {}", opt.input.display()))?;

    let summary =
        braidz_parser::summarize_braidz(&archive, opt.input.display().to_string(), attr.len());

    let yaml_buf = serde_yaml::to_string(&summary)?;
    println!("{}", yaml_buf);

    if opt.data2d_distorted {
        println!("data2d_distorted table: --------------");
        for row in archive.iter_data2d_distorted()? {
            println!("{:?}", row);
        }
    }

    if opt.frame_by_frame {
        frame_by_frame::print_frame_by_frame(archive)?;
    }

    Ok(())
}
