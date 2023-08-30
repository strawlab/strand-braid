use anyhow::Context;
use clap::Parser;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[structopt(name = "braidz-cli")]
#[command(author, version)]
struct Opt {
    /// Input braidz filename
    input: PathBuf,

    /// print all data in the `data2d_distorted` table
    #[arg(short, long)]
    data2d_distorted: bool,
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
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

    Ok(())
}
