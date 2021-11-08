use flydra_types::{CamHttpServerInfo, RawCamName};
use image_tracker::{FlyTracker, UfmfState};

const W: u32 = 32;
const H: u32 = 16;

// At some point, I was having trouble tracking small frames, so I wrote this
// test.

async fn track_small_with_error(handle: tokio::runtime::Handle) -> fmf::FMFResult<()> {
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
        &RawCamName::new("small-test-image".to_string()),
        W,
        H,
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

    let extra = Box::new(basic_frame::BasicExtra {
        host_framenumber: 0,
        host_timestamp: chrono::Utc::now(),
    });
    let buf = vec![0; (W * H) as usize];
    let pixel_format = machine_vision_formats::PixFmt::Mono8;
    let frame = basic_frame::DynamicFrame::new(W, H, W, extra, buf, pixel_format);
    let ufmf_state = UfmfState::Stopped;
    let maybe_found = ft
        .process_new_frame(&frame, ufmf_state, None, None)
        .expect("process frame");
    println!("maybe_found: {:?}", maybe_found);
    Ok(())
}

#[tokio::test]
async fn track_small() {
    env_logger::init();

    let runtime = tokio::runtime::Handle::current();

    track_small_with_error(runtime).await.unwrap();
}
