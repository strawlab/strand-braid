#[macro_use]
extern crate log;

use anyhow::Result;
use braid::{braid_start, BraidConfig2};
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(about = "show the default configuration in TOML format")]
struct BraidDefaultConfigCliArgs {}

fn main() -> Result<()> {
    braid_start("default-config")?;

    let args = BraidDefaultConfigCliArgs::from_args();
    debug!("{:?}", args);

    let cfg = BraidConfig2::default();
    // This 2 step serialization is needed to avoid ValueAfterTable
    // error. See https://github.com/alexcrichton/toml-rs/issues/142
    let value = toml::Value::try_from(&cfg)?;
    let buf = toml::to_string(&value)?;
    println!("{}", buf);

    Ok(())
}
