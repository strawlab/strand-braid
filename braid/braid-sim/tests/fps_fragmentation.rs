// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Regression test: lock the live-vs-retrack fps-mismatch fragmentation
//! mechanism as a deterministic, in-process `cargo test`.
//!
//! Background (see `smoke-tests/flydratrax-fps-fragmentation.sh` for the
//! full-process version): a real flydratrax recording produced hundreds of
//! short live trajectories that collapsed to a few on retracking with the same
//! parameters. The root cause was a wrong effective frame rate: the Kalman
//! `dt = 1 / fps` did not match the true frame spacing. With a too-high fps the
//! process noise is too small, the filter becomes over-confident, the real
//! per-frame motion of a *maneuvering* target falls outside the acceptance gate,
//! observations are rejected, the track coasts, covariance kill fires, and the
//! track re-births with a new `obj_id` — fragmenting one continuous insect into
//! many tracks. The fix estimates the live frame rate from the hardware
//! timestamp; the two `flydratrax-fps-*` smoke tests exercise the full live
//! process.
//!
//! This test reproduces the *mechanism* in isolation, without the live process,
//! detector, UDP, or retrack: the in-process injector feeds the same perfect
//! detections (a maneuvering insect imaged at a true 30 fps) into the real
//! flydra2 tracker twice — once at the matched fps (continuous track, the
//! control) and once at a mismatched, too-high fps (fragmented). The assertion
//! is the fragmentation *gap*. Because there is no detector or network
//! nondeterminism, `(scenario, seed)` reproduces both runs exactly.
//!
//! Runs only with the non-default `inprocess` cargo feature.
#![cfg(feature = "inprocess")]

use braid_sim::Scenario;
use braid_sim::scenario::{
    Arena, BlobParams, CameraRig, InsectSpec, Lissajous, ObservationModel, TimingModel,
};

/// A multi-camera scenario with one sharply *maneuvering* insect at a true
/// 30 fps. The small, fast maneuver overlay gives high acceleration (sharp
/// turns) that a constant-velocity filter cannot predict when its `dt` is wrong.
/// Multiple cameras let the in-process tracker triangulate (track birth needs
/// `min_cameras` views).
fn maneuvering_scenario() -> Scenario {
    Scenario {
        seed: 1,
        fps: 30.0,
        arena: Arena {
            min: [-0.15, -0.15, 0.0],
            max: [0.15, 0.15, 0.30],
        },
        cameras: CameraRig {
            count: 5,
            radius_m: 0.6,
            height_m: 0.7,
            focal_length_px: 900.0,
            image_width: 640,
            image_height: 512,
        },
        insects: vec![InsectSpec {
            id: 1,
            enter_t: 0.0,
            exit_t: None,
            motion: Lissajous {
                freq_hz: [0.11, 0.13, 0.07],
                phase: [0.0, 1.0, 2.0],
                fill: 0.5,
                maneuver_amp_m: 0.004,
                maneuver_freq_hz: 6.0,
            },
        }],
        blob: BlobParams::default(),
        bg_warmup_frames: 0,
        timing: TimingModel::default(),
        observation: ObservationModel::default(),
        reported_fps: None,
        calibration_perturbation: Default::default(),
    }
}

/// Inject the scenario's perfect detections and track them at `tracker_fps`,
/// returning the ground-truth score of the resulting recording.
async fn track_at_fps(
    scenario: &Scenario,
    num_frames: usize,
    tracker_fps: Option<f64>,
    label: &str,
) -> eyre::Result<braid_sim::truth::GroundTruthScore> {
    let tmp = tempfile::tempdir()?;
    let out = tmp.path().join("track.braid");
    let braidz =
        braid_sim::inject::inject_and_track(scenario, num_frames, tracker_fps, &out).await?;
    let score = braid_sim::truth::score_against_truth(&braidz, scenario, 0.02, 5)?;
    eprintln!("[{label}] {}", score.report());
    Ok(score)
}

/// The control: at the matched (true) fps the maneuvering insect tracks
/// continuously — high coverage, essentially one unbroken track. If this ever
/// fails, the harness itself is fragmenting tracks and any "repro" below is
/// suspect (fix the harness first).
#[tokio::test]
async fn matched_fps_tracks_continuously() -> eyre::Result<()> {
    let s = maneuvering_scenario();
    let score = track_at_fps(&s, 450, None, "matched 30 fps").await?;

    assert_eq!(score.num_truth, 1);
    assert!(
        score.coverage > 0.9,
        "matched-fps coverage too low: {}",
        score.coverage
    );
    assert!(
        score.mean_fragments < 3.0,
        "matched fps should not fragment: {} frags/insect",
        score.mean_fragments
    );
    Ok(())
}

/// The bug mechanism: tracking the *same* detections at a too-high fps fragments
/// the one insect into many more tracks than the matched run. This is the
/// regression the hardware-timestamp fps fix prevents in the live system; here
/// we lock that a wrong tracker fps is what drives the fragmentation, with no
/// other imperfection in play.
#[tokio::test]
async fn mismatched_fps_fragments_the_track() -> eyre::Result<()> {
    let s = maneuvering_scenario();

    // Same data, two tracker frame rates.
    let matched = track_at_fps(&s, 450, None, "matched 30 fps").await?;
    let mismatched = track_at_fps(&s, 450, Some(100.0), "mismatched 100 fps").await?;

    // The matched run is continuous (control); see the dedicated test above.
    assert!(
        matched.mean_fragments < 3.0,
        "control regressed: matched fps fragmented ({} frags)",
        matched.mean_fragments
    );

    // The too-high fps must fragment meaningfully more. Use the raw track count
    // (distinct obj_ids) so a fully-killed insect that re-births repeatedly is
    // counted even where coverage drops.
    assert!(
        mismatched.num_tracks >= matched.num_tracks + 3,
        "expected fps-mismatch fragmentation: mismatched num_tracks={} not >> matched num_tracks={}",
        mismatched.num_tracks,
        matched.num_tracks
    );
    assert!(
        mismatched.mean_fragments > matched.mean_fragments + 2.0,
        "expected fps-mismatch fragmentation: mismatched={} frags vs matched={} frags",
        mismatched.mean_fragments,
        matched.mean_fragments
    );
    Ok(())
}
