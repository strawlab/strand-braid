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

fn create_timelapse(input: &str, output: &Path) -> std::process::ExitStatus {
    Command::new(env!("CARGO_BIN_EXE_create-timelapse"))
        .args(["--interval", "1", "--input", input, "--output"])
        .arg(output)
        .status()
        .unwrap()
}

/// The timelapse is written under a temporary name beside its output and
/// moved into place when it is finished. Nothing calls back into the program
/// at the end of the input, so this checks that the move happens at all.
#[test]
fn timelapse_appears_at_its_output() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("timelapse.mp4");

    assert!(create_timelapse(FIXTURE, &output).success());
    assert_eq!(names_in(dir.path()), ["timelapse.mp4"]);
    let movie = std::fs::read(&output).unwrap();
    assert!(!movie.is_empty());

    // An existing output is refused, and left as it was.
    assert!(!create_timelapse(FIXTURE, &output).success());
    assert_eq!(names_in(dir.path()), ["timelapse.mp4"]);
    assert_eq!(std::fs::read(&output).unwrap(), movie);
}

/// A run which fails leaves nothing behind.
#[test]
fn failed_timelapse_leaves_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let not_a_movie = dir.path().join("input.mp4");
    std::fs::write(&not_a_movie, "this is not an MP4 file").unwrap();

    let output = dir.path().join("timelapse.mp4");
    assert!(!create_timelapse(not_a_movie.to_str().unwrap(), &output).success());
    assert_eq!(names_in(dir.path()), ["input.mp4"]);
}
