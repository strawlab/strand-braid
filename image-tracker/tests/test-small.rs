use image_tracker::{FlydraFeatureDetector, UfmfState};

const W: u32 = 32;
const H: u32 = 16;

// At some point, I was having trouble tracking small frames, so I wrote this
// test.

#[tokio::test]
async fn track_small() -> anyhow::Result<()> {
    env_logger::init();

    let handle = tokio::runtime::Handle::current();

    let cfg = im_pt_detect_config::default_absdiff();

    let frame_offset = None;

    #[cfg(feature = "debug-images")]
    let (_, valve) = stream_cancel::Valve::new();

    #[cfg(feature = "debug-images")]
    let addr: std::net::SocketAddr = "127.0.0.1:4338".parse()?;

    #[cfg(feature = "debug-images")]
    let (_shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let mut ft = FlydraFeatureDetector::new(
        &handle,
        &flydra_types::RawCamName::new("small-test-image".to_string()),
        W,
        H,
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

    let extra = Box::new(basic_frame::BasicExtra {
        host_framenumber: 0,
        host_timestamp: chrono::Utc::now(),
    });
    let buf = vec![0; (W * H) as usize];
    let pixel_format = machine_vision_formats::PixFmt::Mono8;
    let frame = basic_frame::DynamicFrame::new(W, H, W, extra, buf, pixel_format);
    let ufmf_state = UfmfState::Stopped;
    let maybe_found = ft.process_new_frame(&frame, ufmf_state, None, None)?;
    println!("maybe_found: {:?}", maybe_found);
    Ok(())
}
