use eyre::Result;

use flydra_feature_detector::{FlydraFeatureDetector, UfmfState};

const FNAME: &str = "movie20190115_221756.fmf";
const URL_BASE: &str = "https://strawlab-cdn.com/assets";
const SHA256SUM: &str = "8c9733b7741ae6c0dbe9bd5595db17d0c8eeede743736aac3bf51e55b372f3d9";

#[tokio::main]
async fn main() -> eyre::Result<()> {
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

    // Buffer all frames first to exclude IO from timing of processing.
    let buffered_frames: Vec<_> = Result::from_iter(reader)?;

    let start = std::time::Instant::now();
    const N_CYCLES: usize = 100;
    let mut count = 0;
    let mut n_pts = 0;
    for _ in 0..N_CYCLES {
        for (fno, res_frame_ts) in buffered_frames.iter().enumerate() {
            let (frame, timestamp) = res_frame_ts;
            let ufmf_state = UfmfState::Stopped;

            let maybe_found = ft.process_new_frame(
                &frame.borrow(),
                fno,
                *timestamp,
                ufmf_state,
                None,
                None,
                None,
            )?;
            count += 1;
            n_pts += maybe_found.0.points.len();
        }
    }
    let dur = start.elapsed();
    let fps = count as f64 / dur.as_secs_f64();
    #[cfg(feature = "use_ipp")]
    let impl_str = " with IPP";
    #[cfg(feature = "do_not_use_ipp")]
    let impl_str = "";

    let target_feature_string = target::features().join(", ");
    println!(
        "{target_feature_string}{impl_str}: processed {count} frames in {:.2} seconds ({fps:.1} fps). Found {n_pts} points total.",
        dur.as_secs_f32()
    );

    Ok(())
}
