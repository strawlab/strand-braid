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

    /// timezone for times of day: `file` (the UTC offset stored in the
    /// file), `local` (this computer's timezone), `utc`, or a fixed UTC
    /// offset such as `+02:00`
    #[arg(long, default_value = "file", allow_hyphen_values = true)]
    pub timezone: DisplayTimezone,
}

/// The timezone in which times of day are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayTimezone {
    /// The UTC offset of the recording computer when the recording started,
    /// as stored in the file.
    File,
    /// The timezone of the computer running `braidz-cli`.
    Local,
    Utc,
    Fixed(chrono::FixedOffset),
}

impl std::str::FromStr for DisplayTimezone {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "file" => Ok(Self::File),
            "local" => Ok(Self::Local),
            "utc" => Ok(Self::Utc),
            _ => s.parse().map(Self::Fixed).map_err(|_| {
                format!(
                    "expected `file`, `local`, `utc` or a UTC offset such as `+02:00`, \
                    not `{s}`"
                )
            }),
        }
    }
}

/// Print the summary of `opt.input`, and any tables requested in `opt`, to
/// `out`.
pub fn run<W: Write>(opt: &Opt, out: &mut W) -> anyhow::Result<()> {
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
        let file_offset = archive
            .metadata
            .original_recording_time
            .map(|t| *t.offset());
        match (opt.timezone, file_offset) {
            (DisplayTimezone::File, Some(offset)) | (DisplayTimezone::Fixed(offset), _) => {
                writeln!(out, "Times of day are at UTC offset {offset}.")?;
                frame_by_frame::write_frame_by_frame(&mut archive, &offset, out)?;
            }
            (DisplayTimezone::File, None) => {
                tracing::warn!(
                    "{} does not store the recording's timezone; showing times of day \
                    in this computer's timezone.",
                    opt.input.display()
                );
                writeln!(out, "Times of day are in this computer's timezone.")?;
                frame_by_frame::write_frame_by_frame(&mut archive, &chrono::Local, out)?;
            }
            (DisplayTimezone::Local, _) => {
                writeln!(out, "Times of day are in this computer's timezone.")?;
                frame_by_frame::write_frame_by_frame(&mut archive, &chrono::Local, out)?;
            }
            (DisplayTimezone::Utc, _) => {
                writeln!(out, "Times of day are in UTC.")?;
                frame_by_frame::write_frame_by_frame(&mut archive, &chrono::Utc, out)?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::DisplayTimezone;

    #[test]
    fn parse_display_timezone() {
        let parse = |s: &str| s.parse::<DisplayTimezone>();
        assert_eq!(parse("file"), Ok(DisplayTimezone::File));
        assert_eq!(parse("local"), Ok(DisplayTimezone::Local));
        assert_eq!(parse("UTC"), Ok(DisplayTimezone::Utc));
        let plus2 = chrono::FixedOffset::east_opt(2 * 3600).unwrap();
        assert_eq!(parse("+02:00"), Ok(DisplayTimezone::Fixed(plus2)));
        let minus530 = chrono::FixedOffset::west_opt(5 * 3600 + 30 * 60).unwrap();
        assert_eq!(parse("-05:30"), Ok(DisplayTimezone::Fixed(minus530)));
        let err = parse("Europe/Berlin").unwrap_err();
        assert!(err.contains("`Europe/Berlin`"), "{err}");
    }
}
