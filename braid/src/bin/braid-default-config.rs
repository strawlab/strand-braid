use braid::braid_start;
use braid_config_data::BraidConfig;
use clap::Parser;
use eyre::Result;

/// show the default configuration in TOML format
#[derive(Debug, Parser)]
#[command(author, version)]
struct BraidDefaultConfigCliArgs {}

fn main() -> Result<()> {
    braid_start("default-config")?;

    env_tracing_logger::init();

    let version = format!("{} (git {})", env!("CARGO_PKG_VERSION"), env!("GIT_HASH"));
    tracing::info!("{} {}", env!("CARGO_PKG_NAME"), version);

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
