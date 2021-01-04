#[macro_use]
extern crate log;

use anyhow::Result;
use braid::{braid_start, parse_config_file};
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(about = "show the configuration, including defaults and overrides in TOML format")]
struct BraidShowConfigCliArgs {
    /// Input directory
    #[structopt(parse(from_os_str))]
    config_file: std::path::PathBuf,
}

fn main() -> Result<()> {
    braid_start("show-config")?;

    let args = BraidShowConfigCliArgs::from_args();
    debug!("{:?}", args);

    let cfg = parse_config_file(&args.config_file)?;
    // This 2 step serialization is needed to avoid ValueAfterTable
    // error. See https://github.com/alexcrichton/toml-rs/issues/142
    let value = toml::Value::try_from(&cfg)?;
    let buf = toml::to_string(&value)?;
    println!("{}", buf);
    Ok(())
}
