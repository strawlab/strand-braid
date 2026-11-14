// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::{path::Path, process::Command};

/// The names in `dir`, sorted.
fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

/// The recording is written under a temporary name beside its output and
/// moved into place once Rerun has flushed and closed it.
#[test]
fn recording_appears_at_its_output() {
    const FNAME: &str = "20201013_140707.braidz";
    const URL_BASE: &str = "https://strawlab-cdn.com/assets/";
    const SHA256SUM: &str = "500b235c321b81ca27a442801e716ec3dd1f12488a60cc9c7d5781855e8d4424";

    let local_fname = format!("scratch/{FNAME}");
    download_verify::download_verify(
        &format!("{URL_BASE}/{FNAME}"),
        &local_fname,
        &download_verify::Hash::Sha256(SHA256SUM.into()),
    )
    .unwrap();

    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("tracking.rrd");
    let status = Command::new(env!("CARGO_BIN_EXE_braidz-export-rrd"))
        .arg(&local_fname)
        .arg("--output")
        .arg(&output)
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(names_in(dir.path()), ["tracking.rrd"]);
    // An RRD file starts with this marker. A file which was moved into place
    // before Rerun wrote to it would be empty.
    assert!(std::fs::read(&output).unwrap().starts_with(b"RRF2"));
}

/// A run which fails leaves nothing behind.
#[test]
fn failed_recording_leaves_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let not_a_recording = dir.path().join("input.braidz");
    std::fs::write(&not_a_recording, "this is not a braidz file").unwrap();

    let status = Command::new(env!("CARGO_BIN_EXE_braidz-export-rrd"))
        .arg(&not_a_recording)
        .arg("--output")
        .arg(dir.path().join("tracking.rrd"))
        .status()
        .unwrap();
    assert!(!status.success());
    assert_eq!(names_in(dir.path()), ["input.braidz"]);
}
