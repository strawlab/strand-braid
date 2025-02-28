const FNAME: &str = "movie-standard41h12.mkv";
const URL_BASE: &str = "https://strawlab-cdn.com/assets";
const SHA256SUM: &str = "ddd2932d74139cd6ab5500b40c5f0482d5036df2f766be3a5f28ae2345e23aed";

#[test]
fn test_detect_tags() -> eyre::Result<()> {
    download_verify::download_verify(
        format!("{}/{}", URL_BASE, FNAME).as_str(),
        FNAME,
        &download_verify::Hash::Sha256(SHA256SUM.into()),
    )?;

    let out_fname: camino::Utf8PathBuf = "movie-standard41h12.apriltag.csv".into();

    let cli_args = apriltag_track_movie::Cli {
        input: FNAME.into(),
        max_num_frames: Some(2),
        output: out_fname.clone(),
        force: true,
        ..Default::default()
    };
    apriltag_track_movie::run_cli(cli_args)?;
    assert!(out_fname.exists());

    // TODO: actually validate contents...

    Ok(())
}
