// See https://github.com/strawlab/strand-braid/issues/3. This tests for a
// difficult case of covariance updating.
const FNAME: &str = "fail-small.braidz";
const URL_BASE: &str = "https://strawlab-cdn.com/assets/";
const SHA256SUM: &str = "51f7958afcbeb5cc72859f4ea2e34b93dd3c739351b35496753662cc3ac3ef3b";

#[tokio::test]
async fn test_covariance() {
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

    let tracking_params: flydra_types::TrackingParams = data_src
        .basic_info()
        .tracking_params
        .as_ref()
        .unwrap()
        .clone();

    let opts = braid_offline::KalmanizeOptions::default();

    let rt_handle = tokio::runtime::Handle::try_current().unwrap();

    let save_performance_histograms = false;

    braid_offline::kalmanize(
        data_src,
        output_braidz,
        None,
        tracking_params,
        opts,
        rt_handle,
        save_performance_histograms,
        &format!("{}:{}", file!(), line!()),
    )
    .await
    .unwrap();

    // Check that braidz parser can open our new file.
    let _archive =
        braidz_parser::braidz_parse_path(&output_root.path().join("output.braidz")).unwrap();
}
