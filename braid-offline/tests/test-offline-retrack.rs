const URL_BASE: &str = "https://strawlab-cdn.com/assets/";

#[tokio::test]
async fn test_min_two_rays_needed() {
    // See https://gitlab.strawlab.org/straw/rust-cam/issues/99
    const FNAME: &str = "20201013_140707.braidz";
    const SHA256SUM: &str = "500b235c321b81ca27a442801e716ec3dd1f12488a60cc9c7d5781855e8d4424";

    // env_tracing_logger::init();

    download_verify::download_verify(
        format!("{}/{}", URL_BASE, FNAME).as_str(),
        FNAME,
        &download_verify::Hash::Sha256(SHA256SUM.into()),
    )
    .unwrap();

    let data_src =
        braidz_parser::incremental_parser::IncrementalParser::open_braidz_file(FNAME).unwrap();
    let data_src = data_src.parse_basics().unwrap();

    let output_root = tempfile::tempdir().unwrap(); // will cleanup on drop
    let output_braidz = output_root.path().join("output.braidz");

    // let output_root = std::path::PathBuf::from("test-output");

    let tracking_params: flydra_types::TrackingParams = data_src
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
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn test_retrack() -> anyhow::Result<()> {
    const FNAME: &str = "20210608_164911_mainbrain_2d_only_short.braidz";
    const SHA256SUM: &str = "6e453bc4c4e0ef8327ce47b3e30c8c0993ad77ff96c2ba79ca6c14eb76834835";

    // env_tracing_logger::init();

    download_verify::download_verify(
        format!("{}/{}", URL_BASE, FNAME).as_str(),
        FNAME,
        &download_verify::Hash::Sha256(SHA256SUM.into()),
    )?;

    let tmpdir = tempfile::tempdir()?; // cleanup on drop
    let output = tmpdir.path().to_path_buf().join("test_retrack.braidz");

    let opt = braid_offline::Cli {
        data_src: std::path::PathBuf::from(FNAME),
        output,
        no_progress: true,
        ..Default::default()
    };

    braid_offline::braid_offline_retrack(opt).await?;
    Ok(())
}
