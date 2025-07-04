use flydra_feature_detector::{FlydraFeatureDetector, UfmfState};

const FNAME: &str = "movie20190115_221756.fmf";
const URL_BASE: &str = "https://strawlab-cdn.com/assets";
const SHA256SUM: &str = "8c9733b7741ae6c0dbe9bd5595db17d0c8eeede743736aac3bf51e55b372f3d9";

#[tokio::test]
async fn track_fmf() -> anyhow::Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    download_verify::download_verify(
        format!("{}/{}", URL_BASE, FNAME).as_str(),
        FNAME,
        &download_verify::Hash::Sha256(SHA256SUM.into()),
    )?;

    let reader = fmf::FMFReader::new(FNAME)?;

    let cfg = flydra_pt_detect_cfg::default_absdiff();

    let mut ft = FlydraFeatureDetector::new(
        &braid_types::RawCamName::new("fmf".to_string()),
        reader.width(),
        reader.height(),
        cfg,
        None,
        None,
    )?;

    let start = std::time::Instant::now();
    let mut count = 0;
    let mut n_pts = 0;
    for (fno, res_frame_ts) in reader.enumerate() {
        let (frame, timestamp) = res_frame_ts?;
        let ufmf_state = UfmfState::Stopped;

        let maybe_found = ft.process_new_frame(
            &frame.borrow(),
            fno,
            timestamp,
            ufmf_state,
            None,
            None,
            None,
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
