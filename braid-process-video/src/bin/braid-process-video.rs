use clap::Parser;
use eyre::{self as anyhow, Result, WrapErr};

use braid_process_video::{auto_config, run_config, BraidRetrackVideoConfig, Validate};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
enum Commands {
    /// Process video using a TOML file as configuration.
    ConfigToml {
        /// Input configuration TOML file
        #[arg(short, long)]
        config_toml: std::path::PathBuf,
    },

    /// Process video using an auto-generated configuration.
    AutoConfig {
        /// Directory with input files
        #[arg(short, long)]
        input_dir: std::path::PathBuf,

        /// Maximum number of frames in output
        #[arg(short, long)]
        max_num_frames: Option<usize>,

        /// If true, include debug output
        #[arg(short, long)]
        debug: bool,

        #[arg(short, long)]
        time_dilation_factor: Option<f32>,
    },

    /// Print an example configuration TOML.
    PrintExampleConfigToml,
}

#[tokio::main]
async fn main() -> Result<()> {
    std::panic::set_hook(Box::new(tracing_panic::panic_hook));

    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "info");
    }

    env_tracing_logger::init();

    let command = Commands::parse();

    let cfg = match &command {
        Commands::ConfigToml { config_toml } => {
            // Get directory of configuration file. Works if config_toml is
            // relative or absolute.
            let abs_cfg_path = config_toml.canonicalize()?;
            let cfg_dir = abs_cfg_path.parent();

            let cfg_str = std::fs::read_to_string(config_toml)
                .with_context(|| format!("Reading config file '{}'", config_toml.display()))?;

            let cfg: BraidRetrackVideoConfig = toml::from_str(&cfg_str).with_context(|| {
                anyhow::anyhow!(
                    "Parse error reading config toml file at \"{}\"",
                    config_toml.display()
                )
            })?;

            cfg.validate(cfg_dir).with_context(|| {
                anyhow::anyhow!(
                    "Validation error with config toml file at \"{}\"",
                    config_toml.display()
                )
            })?
        }
        Commands::AutoConfig {
            input_dir,
            max_num_frames,
            debug,
            time_dilation_factor,
        } => auto_config(input_dir, *max_num_frames, *debug, *time_dilation_factor)?,
        Commands::PrintExampleConfigToml => {
            let default_buf = toml::to_string_pretty(&BraidRetrackVideoConfig::default())?;
            println!("{}", default_buf);
            return Ok(());
        }
    };

    let cfg_as_string = toml::to_string_pretty(cfg.valid()).unwrap();
    tracing::info!(
        "Generating output using the following configuration:\n\n```\n{}```\n",
        cfg_as_string
    );

    run_config(&cfg).await?;
    Ok(())
}
