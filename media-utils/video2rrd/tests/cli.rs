// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::{path::Path, process::Command};

/// A small MP4 file committed for `frame-source`'s tests.
const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../frame-source/tests/data/bframes.mp4"
);

/// The names in `dir`, sorted.
fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

fn video2rrd(input: &str, output: &Path) -> std::process::ExitStatus {
    Command::new(env!("CARGO_BIN_EXE_video2rrd"))
        .args(["--no-progress", "--start-time", "2026-01-01T00:00:00Z"])
        .args(["--input", input, "--output"])
        .arg(output)
        .status()
        .unwrap()
}

/// The recording is written under a temporary name beside its output and
/// moved into place once Rerun has flushed and closed it.
#[test]
fn recording_appears_at_its_output() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("movie.rrd");

    assert!(video2rrd(FIXTURE, &output).success());
    assert_eq!(names_in(dir.path()), ["movie.rrd"]);
    // An RRD file starts with this marker. A file which was moved into place
    // before Rerun wrote to it would be empty.
    assert!(std::fs::read(&output).unwrap().starts_with(b"RRF2"));
}

/// A run which fails leaves nothing behind.
#[test]
fn failed_recording_leaves_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let not_a_movie = dir.path().join("input.mp4");
    std::fs::write(&not_a_movie, "this is not an MP4 file").unwrap();

    let output = dir.path().join("movie.rrd");
    assert!(!video2rrd(not_a_movie.to_str().unwrap(), &output).success());
    assert_eq!(names_in(dir.path()), ["input.mp4"]);
}
