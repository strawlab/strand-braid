// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::process::Command;

use braidz_cli::{DisplayTimezone, Opt};

const URL_BASE: &str = "https://strawlab-cdn.com/assets";
const FNAME: &str = "20201104_174158.braidz";
const SHA256SUM: &str = "d9e742336cf924f378e49055f3a709e52817ed90385c4f777f443952cf0557d6";

/// Expected start of `--frame-by-frame` output for [FNAME].
///
/// Times of day are in the timezone stored in the file, so this does not
/// depend on the machine running the test.
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

#[test]
fn frame_by_frame_snapshot() {
    let opt = Opt {
        input: test_file().into(),
        data2d_distorted: false,
        frame_by_frame: true,
        timezone: DisplayTimezone::File,
    };
    let mut buf = Vec::new();
    braidz_cli::run(&opt, &mut buf).unwrap();
    let output = String::from_utf8(buf).unwrap();

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

/// The first time of day in the frame-by-frame table.
fn first_time_of_day(stdout: &str) -> &str {
    let header = stdout.find("timestamp").expect("timestamp column");
    let next_line = &stdout[header..][stdout[header..].find('\n').unwrap() + 1..];
    next_line
        .split_whitespace()
        .find(|word| word.contains(':'))
        .expect("time of day")
}

#[test]
fn timezone_option() {
    let run = |timezone: &[&str]| {
        let output = braidz_cli()
            .args(["--frame-by-frame", &test_file()])
            .args(timezone)
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap()
    };

    // The file was recorded at UTC+01:00.
    let default = run(&[]);
    assert!(default.contains("Times of day are at UTC offset +01:00."));
    assert_eq!(first_time_of_day(&default), "17:41:58.035798072");

    let utc = run(&["--timezone", "utc"]);
    assert!(utc.contains("Times of day are in UTC."));
    assert_eq!(first_time_of_day(&utc), "16:41:58.035798072");

    let fixed = run(&["--timezone", "-05:00"]);
    assert!(fixed.contains("Times of day are at UTC offset -05:00."));
    assert_eq!(first_time_of_day(&fixed), "11:41:58.035798072");

    let output = braidz_cli()
        .args(["--timezone", "Europe/Berlin", &test_file()])
        .output()
        .unwrap();
    assert!(!output.status.success());
}
