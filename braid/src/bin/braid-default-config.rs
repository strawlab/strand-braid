#[macro_use]
extern crate log;

use anyhow::Result;
use braid::{braid_start, BraidConfig2};
use clap::Parser;

/// show the default configuration in TOML format
#[derive(Debug, Parser)]
#[command(author, version)]
struct BraidDefaultConfigCliArgs {}

fn main() -> Result<()> {
    braid_start("default-config")?;

    let args = BraidDefaultConfigCliArgs::parse();
    debug!("{:?}", args);

    let cfg = BraidConfig2::default();
    // This 2 step serialization is needed to avoid ValueAfterTable
    // error. See https://github.com/alexcrichton/toml-rs/issues/142
    let value = toml::Value::try_from(&cfg)?;
    let buf = toml::to_string(&value)?;
    println!("{}", buf);

    Ok(())
}
