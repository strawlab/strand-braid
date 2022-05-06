use flydra_feature_detector::{FlydraFeatureDetector, UfmfState};
use timestamped_frame::ExtraTimeData;

const FNAME: &str = "movie20190115_221756.fmf";
const URL_BASE: &str = "https://strawlab-cdn.com/assets";
const SHA256SUM: &str = "8c9733b7741ae6c0dbe9bd5595db17d0c8eeede743736aac3bf51e55b372f3d9";

#[tokio::test]
async fn track_fmf() -> anyhow::Result<()> {
    env_logger::init();

    download_verify::download_verify(
        format!("{}/{}", URL_BASE, FNAME).as_str(),
        FNAME,
        &download_verify::Hash::Sha256(SHA256SUM.into()),
    )?;

    let reader = fmf::FMFReader::new(FNAME)?;

    let cfg = flydra_pt_detect_cfg::default_absdiff();

    let frame_offset = None;

    #[cfg(feature = "debug-images")]
    let (_, valve) = stream_cancel::Valve::new();

    #[cfg(feature = "debug-images")]
    let addr: std::net::SocketAddr = "127.0.0.1:4338".parse()?;

    #[cfg(feature = "debug-images")]
    let (_shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let mut ft = FlydraFeatureDetector::new(
        &flydra_types::RawCamName::new("fmf".to_string()),
        reader.width(),
        reader.height(),
        cfg,
        frame_offset,
        #[cfg(feature = "debug-images")]
        addr,
        None,
        None,
        #[cfg(feature = "debug-images")]
        valve,
        #[cfg(feature = "debug-images")]
        Some(shutdown_rx),
        None,
    )?;

    for frame in reader {
        let frame = frame?;
        println!(
            "frame {:?}: {:?}",
            frame.extra().host_framenumber(),
            frame.extra().host_timestamp()
        );
        let ufmf_state = UfmfState::Stopped;
        let maybe_found = ft.process_new_frame(&frame, ufmf_state, None, None)?;
        println!("maybe_found: {:?}", maybe_found);
    }
    Ok(())
}
