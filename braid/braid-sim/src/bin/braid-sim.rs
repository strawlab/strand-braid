// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Driver for the Braid live 3D simulation test harness.
//!
//! `generate` writes the calibration XML and Braid config TOML for a scenario.
//! (Launching `braid-run` and scoring the result are added in later
//! milestones.)

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use eyre::Result;

use braid_sim::Scenario;

#[derive(Debug, Parser)]
#[command(author, version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Generate the calibration XML and Braid config TOML for a scenario.
    Generate {
        /// Path to the `sim.toml` scenario file.
        sim_toml: PathBuf,
        /// Output directory for the generated artifacts.
        #[arg(short, long, default_value = "braid-sim-out")]
        out_dir: PathBuf,
        /// Address for Braid's control HTTP API.
        #[arg(long, default_value = "127.0.0.1:0")]
        http_api_server_addr: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Generate {
            sim_toml,
            out_dir,
            http_api_server_addr,
        } => {
            let text = std::fs::read_to_string(&sim_toml)?;
            let scenario = Scenario::from_toml_str(&text)?;
            let generated =
                braid_sim::harness::generate_run(&scenario, &out_dir, &http_api_server_addr)?;
            println!(
                "wrote calibration: {}",
                generated.calibration_path.display()
            );
            println!("wrote braid config: {}", generated.config_path.display());
            println!(
                "braidz output dir:  {}",
                generated.braidz_output_dir.display()
            );
            println!("\nlaunch with:");
            println!(
                "  STRAND_CAM_SIM_SPEC={} braid run {}",
                sim_toml.display(),
                generated.config_path.display()
            );
        }
    }
    Ok(())
}
