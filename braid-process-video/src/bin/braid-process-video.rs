use anyhow::{Context as ContextTrait, Result};
use clap::Parser;

use braid_process_video::{run_config, BraidRetrackVideoConfig};

#[derive(Debug, Parser)]
#[clap(author, version, about, long_about = None)]
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

    let args = BraidProcessVideoCliArgs::parse();

    let get_usage = || {
        let default_buf = toml::to_string_pretty(&BraidRetrackVideoConfig::default()).unwrap();
        format!(
            "Parsing TOML config file '{}' into BraidRetrackVideoConfig.\n\n\
            Example of a valid TOML configuration:\n\n```\n{}```",
            args.config.display(),
            default_buf
        )
    };

    // Get directory of configuration file. Works if args.config is relative or absolute.
    let abs_cfg_path = args.config.canonicalize()?;
    let cfg_dir = abs_cfg_path.parent();

    let cfg_str = std::fs::read_to_string(&args.config)
        .with_context(|| format!("Reading config file '{}'", args.config.display()))?;

    let cfg: BraidRetrackVideoConfig = toml::from_str(&cfg_str).with_context(get_usage)?;
    let cfg = cfg.validate(cfg_dir).with_context(get_usage)?;

    let cfg_as_string = toml::to_string_pretty(cfg.valid()).unwrap();
    log::info!(
        "Generating output using the following configuration:\n\n```\n{}```\n",
        cfg_as_string
    );

    run_config(&cfg)
}
