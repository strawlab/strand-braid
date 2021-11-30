use anyhow::{Context as ContextTrait, Result};
use structopt::StructOpt;

use braid_process_video::{run_config, BraidRetrackVideoConfig, Validate};

#[derive(Debug, StructOpt)]
#[structopt(about = "process videos within the Braid multi-camera framework")]
struct BraidProcessVideoCliArgs {
    /// Input configuration TOML file
    #[structopt(long, parse(from_os_str))]
    config: std::path::PathBuf,
}

fn main() -> Result<()> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "info");
    }

    env_logger::init();

    let args = BraidProcessVideoCliArgs::from_args();

    let cfg_fname = match args.config.to_str() {
        None => {
            panic!("Configuration file name not utf-8.");
        }
        Some(cfg_fname) => cfg_fname.to_string(),
    };

    let get_usage = || {
        let default_buf = toml::to_string_pretty(&BraidRetrackVideoConfig::default()).unwrap();
        format!(
            "Parsing TOML config file '{}' into BraidRetrackVideoConfig.\n\n\
            Example of a valid TOML configuration:\n\n```\n{}```",
            &cfg_fname, default_buf
        )
    };

    let cfg_str = std::fs::read_to_string(&cfg_fname)
        .with_context(|| format!("Reading config file '{}'", &cfg_fname))?;

    let mut cfg: BraidRetrackVideoConfig = toml::from_str(&cfg_str).with_context(get_usage)?;
    cfg.validate().with_context(get_usage)?;

    let cfg_as_string = toml::to_string_pretty(&cfg).unwrap();
    log::info!(
        "Generating output using the following configuration:\n\n```\n{}```\n",
        cfg_as_string
    );

    run_config(&cfg)
}
