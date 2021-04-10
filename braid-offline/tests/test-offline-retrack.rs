use anyhow::Context;
use std::convert::TryInto;

// See https://gitlab.strawlab.org/straw/rust-cam/issues/99
const FNAME: &str = "20201013_140707.braidz";
const URL_BASE: &str = "https://strawlab-cdn.com/assets/";
const SHA256SUM: &str = "500b235c321b81ca27a442801e716ec3dd1f12488a60cc9c7d5781855e8d4424";

#[tokio::test]
async fn test_min_two_rays_needed() {
    env_tracing_logger::init();

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

    let tracking_params_parsed = data_src.basic_info().tracking_params.as_ref();

    let tracking_params: flydra_types::TrackingParamsInner3D = tracking_params_parsed
        .map(|p| p.try_into().unwrap())
        .unwrap();

    let opts = braid_offline::KalmanizeOptions::default();

    let rt_handle = tokio::runtime::Handle::try_current().unwrap();

    let save_performance_histograms = true;

    braid_offline::kalmanize(
        data_src,
        output_braidz,
        None,
        tracking_params,
        opts,
        rt_handle,
        save_performance_histograms,
    )
    .await
    .unwrap();
}
