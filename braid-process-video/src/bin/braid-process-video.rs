use anyhow::{Context as ContextTrait, Result};
use clap::{Parser, Subcommand};

use braid_process_video::{auto_config, run_config, BraidRetrackVideoConfig};

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
struct Cli {
    #[clap(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Process video using a TOML file as configuration.
    ConfigToml {
        /// Input configuration TOML file
        #[clap(short, long, parse(from_os_str), value_name = "CONFIG_TOML")]
        config_toml: std::path::PathBuf,
    },

    /// Process video using an auto-generated configuration.
    AutoConfig {
        /// Directory with input files
        #[clap(short, long, parse(from_os_str), value_name = "INPUT_DIR")]
        input_dir: std::path::PathBuf,
    },

    /// Print an example configuration TOML.
    PrintExampleConfigToml,
}

fn main() -> Result<()> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "info");
    }

    env_logger::init();

    let cli = Cli::parse();

    let cfg = match &cli.command {
        Some(Commands::ConfigToml { config_toml }) => {
            // Get directory of configuration file. Works if config_toml is
            // relative or absolute.
            let abs_cfg_path = config_toml.canonicalize()?;
            let cfg_dir = abs_cfg_path.parent();

            let cfg_str = std::fs::read_to_string(&config_toml)
                .with_context(|| format!("Reading config file '{}'", config_toml.display()))?;

            let cfg: BraidRetrackVideoConfig = toml::from_str(&cfg_str).with_context(|| {
                anyhow::anyhow!(
                    "Parse error reading config toml file at \"{}\"",
                    config_toml.display()
                )
            })?;

            let cfg = cfg.validate(cfg_dir).with_context(|| {
                anyhow::anyhow!(
                    "Validation error with config toml file at \"{}\"",
                    config_toml.display()
                )
            })?;

            cfg
        }
        Some(Commands::AutoConfig { input_dir }) => {
            let cfg = auto_config(input_dir)?;
            cfg
        }
        Some(Commands::PrintExampleConfigToml) => {
            let default_buf = toml::to_string_pretty(&BraidRetrackVideoConfig::default())?;
            println!("{}", default_buf);
            return Ok(());
        }
        None => {
            log::warn!("Nothing to do: no subcommand given.");
            return Ok(());
        }
    };

    let cfg_as_string = toml::to_string_pretty(cfg.valid()).unwrap();
    log::info!(
        "Generating output using the following configuration:\n\n```\n{}```\n",
        cfg_as_string
    );

    run_config(&cfg)
}
