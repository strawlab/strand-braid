// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::process::Command;

use braidz_cli::Opt;

const URL_BASE: &str = "https://strawlab-cdn.com/assets";
const FNAME: &str = "20201104_174158.braidz";
const SHA256SUM: &str = "d9e742336cf924f378e49055f3a709e52817ed90385c4f777f443952cf0557d6";

/// Expected start of `--frame-by-frame` output for [FNAME], with times in UTC.
const SNAPSHOT: &str = "tests/snapshots/20201104_174158.txt";
/// Set this environment variable to rewrite [SNAPSHOT] instead of checking it.
const UPDATE_ENV_VAR: &str = "BRAIDZ_CLI_UPDATE_SNAPSHOT";
/// Number of lines of the frame-by-frame table kept in [SNAPSHOT].
const SNAPSHOT_TABLE_LINES: usize = 150;

/// Download the test file (if needed) and return its path.
///
/// Integration tests run with the crate directory as working directory, so
/// this is `scratch/` in the crate (ignored by git).
fn test_file() -> String {
    let local_fname = format!("scratch/{FNAME}");
    download_verify::download_verify(
        &format!("{URL_BASE}/{FNAME}"),
        &local_fname,
        &download_verify::Hash::Sha256(SHA256SUM.into()),
    )
    .unwrap();
    local_fname
}

/// Make `output` independent of the machine's timezone.
///
/// The summary stores the recording time in the machine's local timezone;
/// rewrite it in UTC.
fn normalize_recording_time(output: &str) -> String {
    const KEY: &str = "  original_recording_time: ";
    output
        .lines()
        .map(|line| match line.strip_prefix(KEY) {
            Some(value) => {
                let t = chrono::DateTime::parse_from_rfc3339(value).unwrap();
                format!("{KEY}{}", t.with_timezone(&chrono::Utc).to_rfc3339())
            }
            None => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn frame_by_frame_snapshot() {
    let opt = Opt {
        input: test_file().into(),
        data2d_distorted: false,
        frame_by_frame: true,
    };
    let mut buf = Vec::new();
    braidz_cli::run(&opt, &chrono::Utc, &mut buf).unwrap();
    let output = normalize_recording_time(&String::from_utf8(buf).unwrap());

    // Keep the summary and the start of the (very long) table.
    let table_start = output
        .lines()
        .position(|line| line.starts_with('┌'))
        .expect("frame-by-frame table");
    let mut actual = output
        .lines()
        .take(table_start + SNAPSHOT_TABLE_LINES)
        .collect::<Vec<_>>()
        .join("\n");
    actual.push('\n');

    if std::env::var_os(UPDATE_ENV_VAR).is_some() {
        std::fs::write(SNAPSHOT, &actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(SNAPSHOT).unwrap();
    assert!(
        actual == expected,
        "output differs from {SNAPSHOT}. If the change is intended, rerun with \
        {UPDATE_ENV_VAR}=1 and review the diff.\n\nactual output:\n{actual}"
    );
}

fn braidz_cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_braidz-cli"))
}

#[test]
fn missing_file_is_an_error_naming_the_path() {
    let output = braidz_cli().arg("does-not-exist.braidz").output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("does-not-exist.braidz"), "{stderr}");
}

#[test]
fn summary_only_by_default() {
    let output = braidz_cli().arg(test_file()).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.starts_with(&format!("filename: scratch/{FNAME}\n")),
        "{stdout}"
    );
    assert!(!stdout.contains("data2d_distorted table:"));
    assert!(!stdout.contains('┌'));
}

#[test]
fn data2d_distorted_flag_prints_table() {
    let output = braidz_cli()
        .args(["--data2d-distorted", &test_file()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let (_summary, table) = stdout
        .split_once("data2d_distorted table: --------------\n")
        .expect("table header");
    assert!(table.lines().count() > 1000);
    assert!(table.lines().all(|l| l.starts_with("Data2dDistortedRow {")));
}

#[test]
fn frame_by_frame_flag_prints_table() {
    let output = braidz_cli()
        .args(["--frame-by-frame", &test_file()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("│ data2d_distorted_rows "), "{stdout}");
    assert!(stdout.trim_end().ends_with('┘'));
}
