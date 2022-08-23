use flydra_feature_detector::{FlydraFeatureDetector, UfmfState};

const W: u32 = 32;
const H: u32 = 16;

// At some point, I was having trouble tracking small frames, so I wrote this
// test.

#[tokio::test]
async fn track_small() -> anyhow::Result<()> {
    env_logger::init();

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
    Ok(())
}
