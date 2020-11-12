use anyhow::Context;
use std::path::PathBuf;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(name = "braidz-cli")]
struct Opt {
    /// Input braidz filename
    #[structopt(parse(from_os_str))]
    input: PathBuf,
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let opt = Opt::from_args();
    let attr = std::fs::metadata(&opt.input)
        .with_context(|| format!("Getting file metadata for {}", opt.input.display()))?;

    if attr.is_dir() {
        anyhow::bail!("{} is a directory, not a file.", &opt.input.display())
    }
    let archive = braidz_parser::braidz_parse_path(&opt.input)
        .with_context(|| format!("Parsing file {}", opt.input.display()))?;

    let summary =
        braidz_parser::summarize_braidz(&archive, opt.input.display().to_string(), attr.len());

    let yaml_buf = serde_yaml::to_string(&summary)?;
    println!("{}", yaml_buf);
    Ok(())
}
