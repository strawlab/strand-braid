use anyhow::{Context, Result};
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
    braid_start("show-config").with_context(|| format!("launching show-config command"))?;

    let args = BraidShowConfigCliArgs::parse();
    tracing::debug!("{:?}", args);

    let cfg = braid_config_data::parse_config_file(&args.config_file).with_context(|| {
        format!(
            "While parsing configuration file {}",
            args.config_file.display()
        )
    })?;
    // This 2 step serialization is needed to avoid ValueAfterTable
    // error. See https://github.com/alexcrichton/toml-rs/issues/142
    let value = toml::Value::try_from(&cfg)?;
    let buf = toml::to_string(&value)?;
    println!("{}", buf);
    Ok(())
}
