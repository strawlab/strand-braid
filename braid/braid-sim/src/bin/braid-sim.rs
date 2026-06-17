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
    /// Compare a live `.braidz` recording against an offline retrack of it, to
    /// detect live trajectories that are shorter / more fragmented than what
    /// retracking recovers.
    Score {
        /// The live `.braidz` recording to evaluate.
        braidz: PathBuf,
        /// Path to the `braid-offline-retrack` executable. Defaults to one next
        /// to this binary.
        #[arg(long)]
        retrack_exe: Option<PathBuf>,
        /// Where to write the retracked `.braidz` (default: alongside input).
        #[arg(long)]
        retrack_out: Option<PathBuf>,
        /// Fraction of the retrack longest-span below which the live recording
        /// is flagged as shortened.
        #[arg(long, default_value_t = 0.9)]
        span_frac: f64,
    },
}

/// Find `braid-offline-retrack` next to the current executable.
fn default_retrack_exe() -> eyre::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let dir = exe
        .parent()
        .ok_or_else(|| eyre::eyre!("current exe has no parent dir"))?;
    Ok(dir.join("braid-offline-retrack"))
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
        Command::Score {
            braidz,
            retrack_exe,
            retrack_out,
            span_frac,
        } => {
            let retrack_exe = match retrack_exe {
                Some(p) => p,
                None => default_retrack_exe()?,
            };
            let retrack_out = retrack_out.unwrap_or_else(|| {
                let mut p = braidz.clone();
                p.set_extension("retrack.braidz");
                p
            });

            let diff = braid_sim::score::differential(&retrack_exe, &braidz, &retrack_out)?;
            println!("{}", diff.report());
            println!();
            if diff.live_is_shortened(span_frac) {
                println!(
                    "RESULT: live tracks are SHORTER / more fragmented than retrack \
                     (bug reproduced)."
                );
            } else {
                println!("RESULT: live and retrack agree (no shortening detected).");
            }
        }
    }
    Ok(())
}
