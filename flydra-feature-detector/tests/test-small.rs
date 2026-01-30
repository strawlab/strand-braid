use chrono::DateTime;
use flydra_feature_detector::{FlydraFeatureDetector, UfmfState};
use strand_dynamic_frame::DynamicFrame;

fn init() {
    let _ = env_logger::builder().is_test(true).try_init();
}

#[tokio::test]
async fn track_small() -> eyre::Result<()> {
    // At some point, I was having trouble tracking small frames, so I wrote
    // this test.

    const W: u32 = 32;
    const H: u32 = 16;
    let stride = usize::try_from(W).unwrap();

    init();

    let cfg = flydra_pt_detect_cfg::default_absdiff();

    let mut ft = FlydraFeatureDetector::new(
        &braid_types::RawCamName::new("small-test-image".to_string()),
        W,
        H,
        cfg,
        None,
        None,
    )?;

    let buf = vec![0; stride * H as usize];
    let pixel_format = machine_vision_formats::PixFmt::Mono8;
    let frame = DynamicFrame::from_buf(W, H, stride, buf, pixel_format).unwrap();
    let ufmf_state = UfmfState::Stopped;
    let fno = 0;
    let timestamp = DateTime::from_timestamp(1431648000, 0).unwrap();
    let maybe_found = ft.process_new_frame(&frame, fno, timestamp, ufmf_state, None, None, None)?;
    println!("maybe_found: {:?}", maybe_found);
    assert_eq!(maybe_found.0.points.len(), 0);
    Ok(())
}

#[tokio::test]
async fn track_moving_stride() -> eyre::Result<()> {
    // Test with stride not equal width and moving point.
    const W: u32 = 31;
    const STRIDE: usize = 32;
    const H: u32 = 16;

    init();

    let cfg = flydra_pt_detect_cfg::default_absdiff();

    let mut ft = FlydraFeatureDetector::new(
        &braid_types::RawCamName::new("moving".to_string()),
        W,
        H,
        cfg,
        None,
        None,
    )?;

    for fno in 0..100 {
        let mut buf = vec![0; STRIDE * (H as usize - 1) + W as usize];

        let x_pos = fno % W as usize;
        let y_pos = fno % H as usize;

        let buf_idx = y_pos * STRIDE + x_pos;
        buf[buf_idx] = 255;

        let pixel_format = machine_vision_formats::PixFmt::Mono8;
        let frame = DynamicFrame::from_buf(W, H, STRIDE, buf, pixel_format).unwrap();
        let ufmf_state = UfmfState::Stopped;
        let timestamp = DateTime::from_timestamp(1431648000, 0).unwrap();
        let found_points = ft
            .process_new_frame(&frame, fno, timestamp, ufmf_state, None, None, None)?
            .0
            .points
            .into_iter()
            .map(|pt| (pt.x0_abs, pt.y0_abs))
            .collect::<Vec<_>>();
        println!("maybe_found: {found_points:?}, {x_pos},{y_pos}");
    }
    Ok(())
}
