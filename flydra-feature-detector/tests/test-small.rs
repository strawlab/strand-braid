use flydra_feature_detector::{FlydraFeatureDetector, UfmfState};

fn init() {
    let _ = env_logger::builder().is_test(true).try_init();
}

#[tokio::test]
async fn track_small() -> anyhow::Result<()> {
    // At some point, I was having trouble tracking small frames, so I wrote
    // this test.

    const W: u32 = 32;
    const H: u32 = 16;

    init();

    let cfg = flydra_pt_detect_cfg::default_absdiff();

    let frame_offset = None;

    let mut ft = FlydraFeatureDetector::new(
        &flydra_types::RawCamName::new("small-test-image".to_string()),
        W,
        H,
        cfg,
        frame_offset,
        None,
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
    let maybe_found = ft.process_new_frame(&frame, ufmf_state, None, None, None)?;
    println!("maybe_found: {:?}", maybe_found);
    assert_eq!(maybe_found.0.points.len(), 0);
    Ok(())
}

#[tokio::test]
async fn track_moving_stride() -> anyhow::Result<()> {
    // Test with stride not equal width and moving point.
    const W: u32 = 31;
    const STRIDE: usize = 32;
    const H: u32 = 16;

    init();

    let cfg = flydra_pt_detect_cfg::default_absdiff();

    let frame_offset = None;

    let mut ft = FlydraFeatureDetector::new(
        &flydra_types::RawCamName::new("moving".to_string()),
        W,
        H,
        cfg,
        frame_offset,
        None,
        None,
    )?;

    for host_framenumber in 0..100 {
        let extra = Box::new(basic_frame::BasicExtra {
            host_framenumber,
            host_timestamp: chrono::Utc::now(),
        });

        let mut buf = vec![0; STRIDE * (H as usize - 1) + W as usize];

        let x_pos = host_framenumber % W as usize;
        let y_pos = host_framenumber % H as usize;

        let buf_idx = y_pos * STRIDE + x_pos;
        buf[buf_idx] = 255;

        let pixel_format = machine_vision_formats::PixFmt::Mono8;
        let frame = basic_frame::DynamicFrame::new(W, H, STRIDE as u32, extra, buf, pixel_format);
        let ufmf_state = UfmfState::Stopped;
        let maybe_found = ft.process_new_frame(&frame, ufmf_state, None, None, None)?;
        println!("maybe_found: {maybe_found:?}, {x_pos},{y_pos}");
    }
    Ok(())
}
