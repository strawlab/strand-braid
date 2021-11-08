use flydra_types::{CamHttpServerInfo, RawCamName};
use image_tracker::{FlyTracker, UfmfState};
use timestamped_frame::ExtraTimeData;

const FNAME: &str = "movie20190115_221756.fmf";
const URL_BASE: &str = "https://strawlab-cdn.com/assets";
const SHA256SUM: &str = "8c9733b7741ae6c0dbe9bd5595db17d0c8eeede743736aac3bf51e55b372f3d9";

async fn track_fmf_with_error(handle: tokio::runtime::Handle) -> fmf::FMFResult<()> {
    let reader = fmf::FMFReader::new(FNAME)?;

    let cfg = im_pt_detect_config::default_absdiff();

    let frame_offset = None;
    let http_addr = CamHttpServerInfo::NoServer;

    let ros_periodic_update_interval = std::time::Duration::from_secs(1);

    let (_, fake_rx) = futures::channel::mpsc::channel(10);
    let (_, valve) = stream_cancel::Valve::new();

    #[cfg(feature = "debug-images")]
    let addr: std::net::SocketAddr = "127.0.0.1:4338".parse().unwrap();

    #[cfg(feature = "debug-images")]
    let (_shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let mut ft = FlyTracker::new(
        &handle,
        &RawCamName::new("fmf".to_string()),
        reader.width(),
        reader.height(),
        cfg,
        None,
        "test".to_string(),
        frame_offset,
        http_addr,
        ros_periodic_update_interval,
        #[cfg(feature = "debug-images")]
        addr,
        None,
        None,
        fake_rx,
        valve,
        #[cfg(feature = "debug-images")]
        Some(shutdown_rx),
        None,
    )
    .unwrap();

    for frame in reader {
        let frame = frame?;
        println!(
            "frame {:?}: {:?}",
            frame.extra().host_framenumber(),
            frame.extra().host_timestamp()
        );
        let ufmf_state = UfmfState::Stopped;
        let maybe_found = ft
            .process_new_frame(&frame, ufmf_state, None, None)
            .expect("process frame");
        println!("maybe_found: {:?}", maybe_found);
    }
    Ok(())
}

#[tokio::test]
async fn track_fmf() {
    env_logger::init();

    download_verify::download_verify(
        format!("{}/{}", URL_BASE, FNAME).as_str(),
        FNAME,
        &download_verify::Hash::Sha256(SHA256SUM.into()),
    )
    .unwrap();

    let runtime = tokio::runtime::Handle::current();

    track_fmf_with_error(runtime).await.unwrap();
}
