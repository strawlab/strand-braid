// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Finish MP4 recordings which a crash interrupted.
//!
//! Strand Camera and Braid write a recording as `<name>.mp4.partial` and rename
//! it to `<name>.mp4` only once it is finished, so that `<name>.mp4` never names
//! anything but a finished recording with its capture times inside it.
//!
//! Through ffmpeg they record in two stages: ffmpeg encodes the video while each
//! frame's capture time is logged to a temporary `<name>-ffmpeg-rewriter.srt`
//! sidecar, and when the recording is closed the MP4 is re-muxed (without
//! transcoding) into `<name>.mp4-rewritten.partial`, so that every frame carries
//! its capture time as a precision-timestamp SEI NAL unit. A crash, power loss
//! or `kill -9` in between leaves that second stage undone: the video is there,
//! but its timestamps are only in the sidecar.
//!
//! This tool finds such recordings by those leftover files and finishes them,
//! producing exactly the MP4 the interrupted recording would have produced. A
//! `.partial` file which is in fact a finished recording, as the other encoders
//! leave if they are stopped between finishing and renaming, is renamed.
//!
//! What cannot be recovered is the tail: the sidecar is written one frame behind
//! (a frame's entry needs the *next* frame's time to close it), so the group of
//! pictures being encoded when the crash happened has no timing and is dropped.
//! Everything before it is kept.

use camino::Utf8PathBuf;
use clap::{Parser, Subcommand};
use eyre::Result;

use ffmpeg_rewriter::{Repair, State};

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Report which recordings were interrupted, changing nothing. Exits
    /// non-zero if any need repair.
    Check {
        /// Directories to search (recursively) for interrupted recordings, or
        /// individual recordings to examine, by the name they have when
        /// finished, `movie.mp4`. One of the leftover file names also works,
        /// e.g. `movie.mp4.partial`.
        #[arg(required = true, num_args = 1..)]
        inputs: Vec<Utf8PathBuf>,
    },
    /// Finish interrupted recordings in place: embed the capture times from the
    /// sidecar into the MP4, rename it from `.partial`, and remove the leftover
    /// files.
    Fix {
        /// Directories to search (recursively) for interrupted recordings, or
        /// individual recordings to repair, by the name they have when
        /// finished, `movie.mp4`. One of the leftover file names also works,
        /// e.g. `movie.mp4.partial`.
        #[arg(required = true, num_args = 1..)]
        inputs: Vec<Utf8PathBuf>,
    },
}

/// Expand the command line into the list of recordings to work on: every
/// directory is searched for interrupted recordings, while a file names one
/// directly (whether it is the recording itself or one of its leftover files).
/// Duplicates are dropped, keeping the order given.
fn recordings(inputs: &[Utf8PathBuf]) -> Result<Vec<Utf8PathBuf>> {
    let mut out: Vec<Utf8PathBuf> = Vec::new();
    for input in inputs {
        let found = if input.is_dir() {
            ffmpeg_rewriter::find_interrupted(input)?
                .into_iter()
                .map(|p| Utf8PathBuf::try_from(p).expect("path was UTF-8 when found"))
                .collect()
        } else {
            // Accept a leftover file name too: someone who has just found a
            // stray `.mp4.partial` or `-ffmpeg-rewriter.srt` will reach for it
            // rather than work out the recording's own name.
            match ffmpeg_rewriter::recording_for_leftover(input) {
                Some(mp4) => {
                    vec![Utf8PathBuf::try_from(mp4).expect("input path was UTF-8")]
                }
                None => vec![input.clone()],
            }
        };
        for mp4 in found {
            if !out.contains(&mp4) {
                out.push(mp4);
            }
        }
    }
    Ok(out)
}

/// One-line description of what a recording needs.
fn describe(mp4: &Utf8PathBuf, state: State) -> String {
    match state {
        State::NotInterrupted => format!("OK            {mp4}  (nothing to repair)"),
        State::NeedsRewrite => format!(
            "NEEDS-REPAIR  {mp4}  (video is present, but its capture times are still only in the \
            sidecar file)"
        ),
        State::RewrittenNotInPlace | State::FinishedNotInPlace => format!(
            "NEEDS-REPAIR  {mp4}  (recording is complete; it only needs renaming from its \
            `.partial` name)"
        ),
        State::StaleSidecars => format!(
            "NEEDS-REPAIR  {mp4}  (recording is complete; only its leftover sidecar files need \
            removing)"
        ),
        State::NothingRecorded => format!(
            "NEEDS-REPAIR  {mp4}  (no video was ever written; only its leftover sidecar files \
            need removing)"
        ),
    }
}

fn cmd_check(inputs: &[Utf8PathBuf]) -> Result<bool> {
    let mut any_to_do = false;
    for mp4 in recordings(inputs)? {
        match ffmpeg_rewriter::inspect(&mp4) {
            Ok(State::NotInterrupted) => println!("{}", describe(&mp4, State::NotInterrupted)),
            Ok(state) => {
                any_to_do = true;
                println!("{}", describe(&mp4, state));
            }
            Err(e) => {
                any_to_do = true;
                println!("UNKNOWN       {mp4}  (could not examine: {e})");
            }
        }
    }
    Ok(any_to_do)
}

fn cmd_fix(inputs: &[Utf8PathBuf]) -> Result<bool> {
    let mut any_failed = false;
    for mp4 in recordings(inputs)? {
        match ffmpeg_rewriter::repair(&mp4) {
            Ok(Repair::NotInterrupted) => println!("OK        {mp4}  (nothing to repair)"),
            Ok(Repair::Rewritten(outcome)) => {
                let frames = format!(
                    "{} of {} frames",
                    outcome.frames_written, outcome.total_frames
                );
                match &outcome.truncated_reason {
                    Some(reason) => {
                        println!("REPAIRED  {mp4}  (kept {frames}: {reason})")
                    }
                    None => println!("REPAIRED  {mp4}  ({frames})"),
                }
            }
            Ok(Repair::MovedIntoPlace) => println!(
                "REPAIRED  {mp4}  (recording was already complete; renamed it from its \
                `.partial` name)"
            ),
            Ok(Repair::RemovedStaleSidecars) => println!(
                "CLEANED   {mp4}  (recording was already complete; removed its leftover sidecar \
                files)"
            ),
            Ok(Repair::RemovedEmptyRecording) => println!(
                "CLEANED   {mp4}  (no video was ever written; removed its leftover sidecar files)"
            ),
            Err(e) => {
                any_failed = true;
                println!("FAILED    {mp4}  ({e})");
            }
        }
    }
    Ok(any_failed)
}

fn main() -> Result<()> {
    env_tracing_logger::init();
    let cli = Cli::parse();

    let exit_nonzero = match &cli.cmd {
        Cmd::Check { inputs } => cmd_check(inputs)?,
        Cmd::Fix { inputs } => cmd_fix(inputs)?,
    };
    if exit_nonzero {
        std::process::exit(1);
    }
    Ok(())
}
