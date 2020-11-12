#[macro_use]
extern crate log;

use braid::{braid_start, BraidConfig};
use failure::Error;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(about = "show the default configuration in TOML format")]
struct BraidDefaultConfigCliArgs {}

fn main() -> Result<(), Error> {
    braid_start("default-config")?;

    let args = BraidDefaultConfigCliArgs::from_args();
    debug!("{:?}", args);

    let cfg = BraidConfig::default();
    // This 2 step serialization is needed to avoid ValueAfterTable
    // error. See https://github.com/alexcrichton/toml-rs/issues/142
    let value = toml::Value::try_from(&cfg)?;
    let buf = toml::to_string(&value)?;
    println!("{}", buf);

    Ok(())
}
