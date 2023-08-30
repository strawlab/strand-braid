#[macro_use]
extern crate log;

use anyhow::Result;
use braid::braid_start;
use clap::Parser;

/// show the configuration, including defaults and overrides in TOML format
#[derive(Debug, Parser)]
#[command(author, version)]
struct BraidShowConfigCliArgs {
    /// Input directory
    config_file: std::path::PathBuf,
}

fn main() -> Result<()> {
    braid_start("show-config")?;

    let args = BraidShowConfigCliArgs::parse();
    debug!("{:?}", args);

    let cfg = braid_config_data::parse_config_file(&args.config_file)?;
    // This 2 step serialization is needed to avoid ValueAfterTable
    // error. See https://github.com/alexcrichton/toml-rs/issues/142
    let value = toml::Value::try_from(&cfg)?;
    let buf = toml::to_string(&value)?;
    println!("{}", buf);
    Ok(())
}
