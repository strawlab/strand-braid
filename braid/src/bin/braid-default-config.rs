use anyhow::Result;
use braid::{braid_start, BraidConfig};
use clap::Parser;

/// show the default configuration in TOML format
#[derive(Debug, Parser)]
#[command(author, version)]
struct BraidDefaultConfigCliArgs {}

fn main() -> Result<()> {
    braid_start("default-config")?;

    let args = BraidDefaultConfigCliArgs::parse();
    tracing::debug!("{:?}", args);

    let cfg = BraidConfig::default();
    // This 2 step serialization is needed to avoid ValueAfterTable
    // error. See https://github.com/alexcrichton/toml-rs/issues/142
    let value = toml::Value::try_from(&cfg)?;
    let buf = toml::to_string(&value)?;
    println!("{}", buf);

    Ok(())
}
