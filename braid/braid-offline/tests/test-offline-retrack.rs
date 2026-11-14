// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

use eyre::{self as anyhow};

const URL_BASE: &str = "https://strawlab-cdn.com/assets/";

#[tokio::test]
async fn test_min_two_rays_needed() {
    // See https://gitlab.strawlab.org/straw/rust-cam/issues/99
    const FNAME: &str = "20201013_140707.braidz";
    const SHA256SUM: &str = "500b235c321b81ca27a442801e716ec3dd1f12488a60cc9c7d5781855e8d4424";

    let local_fname = format!("scratch/{}", FNAME);

    // env_tracing_logger::init();

    download_verify::download_verify(
        format!("{}/{}", URL_BASE, FNAME).as_str(),
        &local_fname,
        &download_verify::Hash::Sha256(SHA256SUM.into()),
    )
    .unwrap();

    let data_src =
        braidz_parser::incremental_parser::IncrementalParser::open_braidz_file(&local_fname)
            .unwrap();
    let data_src = data_src.parse_basics().unwrap();

    let output_root = tempfile::tempdir().unwrap(); // will cleanup on drop
    let output_braidz = output_root.path().join("output.braidz");

    // let output_root = std::path::PathBuf::from("test-output");

    let tracking_params: braid_types::TrackingParams = data_src
        .basic_info()
        .tracking_params
        .as_ref()
        .unwrap()
        .clone();

    let opts = braid_offline::KalmanizeOptions::default();

    let save_performance_histograms = true;

    braid_offline::kalmanize(
        data_src,
        output_braidz,
        None,
        tracking_params,
        opts,
        save_performance_histograms,
        &format!("{}:{}", file!(), line!()),
        true,
        None,
        None,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn test_retrack() -> anyhow::Result<()> {
    const FNAME: &str = "20210608_164911_mainbrain_2d_only_short.braidz";
    const SHA256SUM: &str = "6e453bc4c4e0ef8327ce47b3e30c8c0993ad77ff96c2ba79ca6c14eb76834835";

    // env_tracing_logger::init();

    let local_fname = format!("scratch/{}", FNAME);
    download_verify::download_verify(
        format!("{}/{}", URL_BASE, FNAME).as_str(),
        &local_fname,
        &download_verify::Hash::Sha256(SHA256SUM.into()),
    )?;

    let tmpdir = tempfile::tempdir()?; // cleanup on drop
    let output = tmpdir.path().to_path_buf().join("test_retrack.braidz");

    let opt = braid_offline::Cli {
        data_src: std::path::PathBuf::from(local_fname),
        output,
        no_progress: true,
        ..Default::default()
    };

    braid_offline::braid_offline_retrack(opt).await?;
    Ok(())
}

/// Make, in `dir`, a copy of the short test recording as a `.braid` directory
/// whose 2D data ends in a row which cannot be parsed. Tracking it gets most of
/// the way through and then fails.
fn broken_input(dir: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
    use std::io::Read;

    const FNAME: &str = "20210608_164911_mainbrain_2d_only_short.braidz";
    const SHA256SUM: &str = "6e453bc4c4e0ef8327ce47b3e30c8c0993ad77ff96c2ba79ca6c14eb76834835";

    let local_fname = format!("scratch/{}", FNAME);
    download_verify::download_verify(
        format!("{}/{}", URL_BASE, FNAME).as_str(),
        &local_fname,
        &download_verify::Hash::Sha256(SHA256SUM.into()),
    )?;

    let input = dir.join("broken.braid");
    zip::ZipArchive::new(std::fs::File::open(&local_fname)?)?.extract(&input)?;

    let gz_fname = input.join("data2d_distorted.csv.gz");
    let mut csv = String::new();
    libflate::gzip::Decoder::new(std::fs::File::open(&gz_fname)?)?.read_to_string(&mut csv)?;
    csv.push_str("this is not a row of 2D data\n");
    std::fs::remove_file(&gz_fname)?;
    std::fs::write(input.join("data2d_distorted.csv"), csv)?;
    Ok(input)
}

/// Retrack the broken input and return the names left in the output directory.
async fn names_left_by_failed_retrack(keep_braid_dir_on_fail: bool) -> anyhow::Result<Vec<String>> {
    let input_dir = tempfile::tempdir()?;
    let data_src = broken_input(input_dir.path())?;

    let output_dir = tempfile::tempdir()?;
    let opt = braid_offline::Cli {
        data_src,
        output: output_dir.path().join("retracked.braidz"),
        no_progress: true,
        keep_braid_dir_on_fail,
        ..Default::default()
    };
    let result = braid_offline::braid_offline_retrack(opt).await;
    assert!(result.is_err(), "retracking the broken input must fail");

    let mut names: Vec<String> = std::fs::read_dir(output_dir.path())?
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    Ok(names)
}

/// A retrack which fails partway must leave no `.braidz` file: it would be
/// indistinguishable from a complete one. By default it leaves nothing at all.
#[tokio::test]
async fn test_failed_retrack_leaves_nothing() -> anyhow::Result<()> {
    let names = names_left_by_failed_retrack(false).await?;
    assert!(names.is_empty(), "left behind: {names:?}");
    Ok(())
}

/// With `--keep-braid-dir-on-fail`, the directory the output was being
/// assembled in stays for inspection, and still no `.braidz` file is written.
#[tokio::test]
async fn test_failed_retrack_can_keep_braid_dir() -> anyhow::Result<()> {
    let names = names_left_by_failed_retrack(true).await?;
    assert_eq!(names, ["retracked.braid"]);
    Ok(())
}

/// The `.braid` directory is deleted when tracking fails, so tracking must
/// refuse to start in one which already exists, and must leave it alone.
#[tokio::test]
async fn test_retrack_refuses_an_existing_braid_dir() -> anyhow::Result<()> {
    const FNAME: &str = "20210608_164911_mainbrain_2d_only_short.braidz";
    const SHA256SUM: &str = "6e453bc4c4e0ef8327ce47b3e30c8c0993ad77ff96c2ba79ca6c14eb76834835";

    let local_fname = format!("scratch/{}", FNAME);
    download_verify::download_verify(
        format!("{}/{}", URL_BASE, FNAME).as_str(),
        &local_fname,
        &download_verify::Hash::Sha256(SHA256SUM.into()),
    )?;

    let output_dir = tempfile::tempdir()?;
    let existing = output_dir.path().join("retracked.braid");
    std::fs::create_dir(&existing)?;
    let precious = existing.join("precious.txt");
    std::fs::write(&precious, "not ours to delete")?;

    let opt = braid_offline::Cli {
        data_src: std::path::PathBuf::from(local_fname),
        output: output_dir.path().join("retracked.braidz"),
        no_progress: true,
        ..Default::default()
    };
    let result = braid_offline::braid_offline_retrack(opt).await;
    assert!(result.is_err(), "must refuse the existing directory");
    assert_eq!(std::fs::read_to_string(&precious)?, "not ours to delete");
    assert!(!output_dir.path().join("retracked.braidz").exists());
    Ok(())
}
