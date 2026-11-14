// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: BSD-2-Clause

const FNAME: &str = "movie-standard41h12.mkv";
const URL_BASE: &str = "https://strawlab-cdn.com/assets";
const SHA256SUM: &str = "ddd2932d74139cd6ab5500b40c5f0482d5036df2f766be3a5f28ae2345e23aed";

#[test]
fn test_detect_tags() -> eyre::Result<()> {
    let input_fname = camino::Utf8Path::new("scratch").join(FNAME);
    download_verify::download_verify(
        format!("{}/{}", URL_BASE, FNAME).as_str(),
        input_fname.as_str(),
        &download_verify::Hash::Sha256(SHA256SUM.into()),
    )?;

    std::fs::create_dir_all("scratch")?;
    let out_fname: camino::Utf8PathBuf = "scratch/movie-standard41h12.apriltag.csv".into();
    // The output has a fixed name. Remove what an earlier run left, so that the
    // checks below cannot pass on a stale file.
    match std::fs::remove_file(&out_fname) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }

    let cli_args = apriltag_track_movie::Cli {
        input: input_fname,
        max_num_frames: Some(2),
        output: out_fname.clone(),
        force: true,
        ..Default::default()
    };
    apriltag_track_movie::run_cli(cli_args)?;
    assert!(out_fname.exists());

    // The detections are written under a temporary name beside the output,
    // which must be gone once they are in place.
    let partial_files: Vec<_> = std::fs::read_dir("scratch")?
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name.ends_with(".partial"))
        .collect();
    assert!(partial_files.is_empty(), "{partial_files:?}");

    // Without `--force` an existing output is refused, and left as it was.
    let detections = std::fs::read(&out_fname)?;
    let cli_args = apriltag_track_movie::Cli {
        input: camino::Utf8Path::new("scratch").join(FNAME),
        max_num_frames: Some(1),
        output: out_fname.clone(),
        force: false,
        ..Default::default()
    };
    assert!(apriltag_track_movie::run_cli(cli_args).is_err());
    assert_eq!(std::fs::read(&out_fname)?, detections);

    // TODO: actually validate contents...

    Ok(())
}
