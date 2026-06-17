// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! M0 spike for the live 3D simulation test harness
//! (see `scratch/2026-06-17_braid-sim-bug1-shortened-trajectories-plan.md`).
//!
//! De-risks the *blob -> detector contract*: will the real `flydra-feature-detector`
//! (default absdiff config) reliably find a synthetic Gaussian blob rendered into a
//! Mono8 image, and where (sub-pixel) does it report it? We sweep blob size, peak
//! intensity, and background level over a *moving* blob (a stationary one is absorbed
//! into the background model) and print a table, then assert that at least one
//! plausible setting yields a clean single, well-localized detection.
//!
//! Run with output:
//!   cargo test -p flydra-feature-detector --test m0_blob_detector_contract -- --nocapture

use chrono::{DateTime, TimeDelta, Utc};
use flydra_feature_detector::{BackgroundUpdateMode, FlydraFeatureDetector, TimingInfo, UfmfState};
use strand_dynamic_frame::DynamicFrame;

const W: u32 = 96;
const H: u32 = 96;
/// Insect-free frames used to establish a clean background model before the blob
/// enters. With the default `bg_update_interval: 200`, the model is initialized
/// from the first frames, so the blob must be *absent* during this phase or its
/// early path gets baked into the background (biasing later localization).
const N_BG_WARMUP: usize = 30;
/// Frames with the moving blob present (these are the ones we score).
const N_MOVE: usize = 60;

/// Render a Gaussian blob of peak amplitude `peak` and width `sigma` centered at
/// `(cx, cy)` onto a flat background `bg`, as a Mono8 buffer (stride == W).
fn render_blob(bg: u8, peak: f64, sigma: f64, cx: f64, cy: f64) -> Vec<u8> {
    let stride = W as usize;
    let mut buf = vec![bg; stride * H as usize];
    let two_sig2 = 2.0 * sigma * sigma;
    for py in 0..H as usize {
        for px in 0..W as usize {
            let dx = px as f64 - cx;
            let dy = py as f64 - cy;
            let g = peak * (-(dx * dx + dy * dy) / two_sig2).exp();
            let v = (bg as f64 + g).round().clamp(0.0, 255.0) as u8;
            buf[py * stride + px] = v;
        }
    }
    buf
}

/// True blob center at move-phase frame `i` (0..N_MOVE): a slow diagonal sweep
/// kept well away from the image edges (so the feature window never clips).
fn center_at(i: usize) -> (f64, f64) {
    let t = i as f64 / (N_MOVE as f64 - 1.0);
    let cx = 28.0 + t * 40.0; // 28 -> 68
    let cy = 36.0 + t * 24.0; // 36 -> 60
    (cx, cy)
}

struct Outcome {
    first_detect: Option<usize>,
    eligible: usize,
    single_hits: usize,
    multi_frames: usize,
    sum_loc_err: f64,
    n_loc: usize,
    sum_area: f64,
}

/// Run the detector over a moving blob for one (bg, peak, sigma) setting.
fn run_one(bg: u8, peak: f64, sigma: f64) -> eyre::Result<Outcome> {
    let stride = W as usize;
    let cfg = flydra_pt_detect_cfg::default_absdiff();
    let mut ft = FlydraFeatureDetector::new(
        &braid_types::RawCamName::new("m0-blob".to_string()),
        W,
        H,
        cfg,
        None,
        None,
        BackgroundUpdateMode::Synchronous,
    )?;

    let base = DateTime::from_timestamp(1_431_648_000, 0).unwrap();
    let mut fno = 0usize;

    // Phase 1: establish the background on insect-free frames (not scored).
    for _ in 0..N_BG_WARMUP {
        let buf = vec![bg; stride * H as usize];
        let frame = DynamicFrame::from_buf(W, H, stride, buf, machine_vision_formats::PixFmt::Mono8)
            .unwrap();
        let ts: DateTime<Utc> = base + TimeDelta::milliseconds(10 * fno as i64);
        ft.process_new_frame(&frame, UfmfState::Stopped, TimingInfo::minimal(fno, ts))?;
        fno += 1;
    }

    let mut out = Outcome {
        first_detect: None,
        eligible: 0,
        single_hits: 0,
        multi_frames: 0,
        sum_loc_err: 0.0,
        n_loc: 0,
        sum_area: 0.0,
    };

    // Phase 2: moving blob present; these frames are scored.
    for i in 0..N_MOVE {
        let (cx, cy) = center_at(i);
        let buf = render_blob(bg, peak, sigma, cx, cy);
        let frame = DynamicFrame::from_buf(W, H, stride, buf, machine_vision_formats::PixFmt::Mono8)
            .unwrap();
        let ts: DateTime<Utc> = base + TimeDelta::milliseconds(10 * fno as i64);
        let pts = ft
            .process_new_frame(&frame, UfmfState::Stopped, TimingInfo::minimal(fno, ts))?
            .0
            .points;
        fno += 1;

        if !pts.is_empty() && out.first_detect.is_none() {
            out.first_detect = Some(i);
        }
        out.eligible += 1;
        match pts.len() {
            0 => {}
            1 => out.single_hits += 1,
            _ => out.multi_frames += 1,
        }
        // Localization error against the nearest reported point.
        if let Some(best) = pts
            .iter()
            .map(|p| ((p.x0_abs - cx).powi(2) + (p.y0_abs - cy).powi(2)).sqrt())
            .min_by(|a, b| a.partial_cmp(b).unwrap())
        {
            out.sum_loc_err += best;
            out.n_loc += 1;
        }
        if let Some(p) = pts.first() {
            out.sum_area += p.area;
        }
    }
    Ok(out)
}

#[tokio::test]
async fn m0_blob_detector_contract() -> eyre::Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    let bgs = [0u8, 20];
    let peaks = [40.0, 80.0, 160.0, 255.0];
    let sigmas = [1.0, 1.5, 2.5];

    println!();
    println!(
        "{:>3} {:>5} {:>5} | {:>9} {:>11} {:>6} {:>9} {:>8}",
        "bg", "peak", "sigma", "warmup", "detect%", "multi", "loc_err", "area"
    );
    println!("{}", "-".repeat(72));

    // Track the best clean config to summarize and assert on.
    let mut best_clean: Option<(u8, f64, f64, f64, f64)> = None; // bg,peak,sigma,detect_rate,loc_err

    for &bg in &bgs {
        for &peak in &peaks {
            for &sigma in &sigmas {
                let o = run_one(bg, peak, sigma)?;
                let detect_rate = if o.eligible > 0 {
                    o.single_hits as f64 / o.eligible as f64
                } else {
                    0.0
                };
                let loc_err = if o.n_loc > 0 {
                    o.sum_loc_err / o.n_loc as f64
                } else {
                    f64::NAN
                };
                let area = if o.n_loc > 0 {
                    o.sum_area / o.n_loc as f64
                } else {
                    f64::NAN
                };
                let warmup = o
                    .first_detect
                    .map(|f| format!("{f}"))
                    .unwrap_or_else(|| "none".to_string());
                println!(
                    "{:>3} {:>5.0} {:>5.1} | {:>9} {:>10.1}% {:>6} {:>9.3} {:>8.1}",
                    bg, peak, sigma, warmup, detect_rate * 100.0, o.multi_frames, loc_err, area
                );

                // "Clean" = detects on essentially every eligible frame, exactly one
                // point, sub-pixel-ish localization.
                if detect_rate >= 0.95 && o.multi_frames == 0 && loc_err < 1.0 {
                    let better = match best_clean {
                        None => true,
                        Some((_, _, _, _, prev_err)) => loc_err < prev_err,
                    };
                    if better {
                        best_clean = Some((bg, peak, sigma, detect_rate, loc_err));
                    }
                }
            }
        }
    }

    println!("{}", "-".repeat(72));
    match best_clean {
        Some((bg, peak, sigma, rate, err)) => {
            println!(
                "M0 contract OK: best clean config bg={bg} peak={peak:.0} sigma={sigma:.1} \
                 -> detect={:.1}% loc_err={err:.3}px",
                rate * 100.0
            );
        }
        None => {}
    }

    assert!(
        best_clean.is_some(),
        "M0 FAILED: no (bg,peak,sigma) gave a clean single, well-localized detection. \
         See the table above; the blob->detector contract needs revisiting."
    );
    Ok(())
}
