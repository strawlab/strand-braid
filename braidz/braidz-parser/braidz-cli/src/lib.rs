// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Inspect and query `.braidz` tracking data files.
//!
//! The `braidz-cli` binary is a thin wrapper around [run]. The logic lives in
//! this library so that it can be tested with a fixed timezone and with the
//! output captured.

use anyhow::Context;
use clap::Parser;
use std::{io::Write, path::PathBuf};

mod frame_by_frame;

/// Command line options of the `braidz-cli` binary.
#[derive(Debug, Parser)]
#[command(author, version)]
pub struct Opt {
    /// Input braidz filename
    pub input: PathBuf,

    /// print all data in the `data2d_distorted` table
    #[arg(short, long)]
    pub data2d_distorted: bool,

    /// print frame-by-frame view of all data
    #[arg(short, long)]
    pub frame_by_frame: bool,
}

/// Print the summary of `opt.input`, and any tables requested in `opt`, to
/// `out`.
///
/// Times of day in the frame-by-frame view are shown in timezone `tz`.
pub fn run<Tz: chrono::TimeZone, W: Write>(opt: &Opt, tz: &Tz, out: &mut W) -> anyhow::Result<()> {
    let attr = std::fs::metadata(&opt.input)
        .with_context(|| format!("Getting file metadata for {}", opt.input.display()))?;

    let mut archive = braidz_parser::braidz_parse_path(&opt.input)
        .with_context(|| format!("Parsing file {}", opt.input.display()))?;

    let summary =
        braidz_parser::summarize_braidz(&archive, opt.input.display().to_string(), attr.len());

    let yaml_buf = serde_yaml::to_string(&summary)?;
    writeln!(out, "{yaml_buf}")?;

    if opt.data2d_distorted {
        writeln!(out, "data2d_distorted table: --------------")?;
        for row in archive.iter_data2d_distorted()? {
            let row = row.context("Reading data2d_distorted table")?;
            writeln!(out, "{row:?}")?;
        }
    }

    if opt.frame_by_frame {
        frame_by_frame::write_frame_by_frame(&mut archive, tz, out)?;
    }

    Ok(())
}
