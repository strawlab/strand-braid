use flydra_feature_detector::{BackgroundUpdateMode, FlydraFeatureDetector, TimingInfo, UfmfState};

const FNAME: &str = "movie20190115_221756.fmf";
const URL_BASE: &str = "https://strawlab-cdn.com/assets";
const SHA256SUM: &str = "8c9733b7741ae6c0dbe9bd5595db17d0c8eeede743736aac3bf51e55b372f3d9";

#[tokio::test]
async fn track_fmf() -> eyre::Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    let local_fname = format!("scratch/{}", FNAME);
    download_verify::download_verify(
        format!("{}/{}", URL_BASE, FNAME).as_str(),
        &local_fname,
        &download_verify::Hash::Sha256(SHA256SUM.into()),
    )?;

    let reader = fmf::FMFReader::new(&local_fname)?;
    let cfg = flydra_pt_detect_cfg::default_absdiff();

    let mut ft = FlydraFeatureDetector::new(
        &braid_types::RawCamName::new("fmf".to_string()),
        reader.width(),
        reader.height(),
        cfg,
        None,
        None,
        BackgroundUpdateMode::Synchronous,
    )?;

    // Buffer all frames first to exclude IO from timing of processing.
    let buffered_frames: Vec<_> = Result::from_iter(reader)?;

    let start = std::time::Instant::now();
    let mut count = 0;
    let mut n_pts = 0;
    for (fno, res_frame_ts) in buffered_frames.into_iter().enumerate() {
        let (frame, timestamp) = res_frame_ts;
        let ufmf_state = UfmfState::Stopped;
        let maybe_found = ft.process_new_frame(
            &frame.borrow(),
            ufmf_state,
            TimingInfo::minimal(fno, timestamp),
        )?;
        count += 1;
        n_pts += maybe_found.0.points.len();
    }
    let dur = start.elapsed();
    let fps = count as f64 / dur.as_secs_f64();
    println!(
        "processed {} frames in {} seconds ({} fps). Found {} points total.",
        count,
        dur.as_secs_f32(),
        fps,
        n_pts
    );

    Ok(())
}

/// Per-frame detected points, used to compare two runs for bit-identical output.
type FrameResults = Vec<Vec<(f64, f64, f64)>>;

/// With [BackgroundUpdateMode::Synchronous], processing identical input must
/// produce bit-identical output on every run. (With the asynchronous worker
/// thread, the frame at which a background-model update lands depends on thread
/// scheduling, so threshold-edge detections flip and the total point count
/// varies run-to-run.) Enough frames are processed to trigger several
/// background-model updates (`bg_update_interval` defaults to 200).
#[tokio::test]
async fn deterministic_across_runs() -> eyre::Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    let local_fname = format!("scratch/{}", FNAME);
    download_verify::download_verify(
        format!("{}/{}", URL_BASE, FNAME).as_str(),
        &local_fname,
        &download_verify::Hash::Sha256(SHA256SUM.into()),
    )?;

    let reader = fmf::FMFReader::new(&local_fname)?;
    let width = reader.width();
    let height = reader.height();
    let buffered_frames: Vec<_> = Result::from_iter(reader)?;

    // 3 cycles over 120 frames = 360 frames, enough to fire background updates.
    const N_CYCLES: usize = 3;

    let run_once = || -> eyre::Result<FrameResults> {
        let cfg = flydra_pt_detect_cfg::default_absdiff();
        let mut ft = FlydraFeatureDetector::new(
            &braid_types::RawCamName::new("fmf".to_string()),
            width,
            height,
            cfg,
            None,
            None,
            BackgroundUpdateMode::Synchronous,
        )?;
        let mut per_frame = FrameResults::new();
        for _ in 0..N_CYCLES {
            for (fno, (frame, timestamp)) in buffered_frames.iter().enumerate() {
                let found = ft.process_new_frame(
                    &frame.borrow(),
                    UfmfState::Stopped,
                    TimingInfo::minimal(fno, *timestamp),
                )?;
                per_frame.push(
                    found
                        .0
                        .points
                        .iter()
                        .map(|p| (p.x0_abs, p.y0_abs, p.area))
                        .collect(),
                );
            }
        }
        Ok(per_frame)
    };

    let run_a = run_once()?;
    let run_b = run_once()?;
    assert_eq!(
        run_a, run_b,
        "synchronous background updates must yield identical output run-to-run"
    );

    Ok(())
}
