// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Multi-object tracking tests: drive the real flydra2 tracker in-process with
//! synthetic 2D detections for *more than one* insect at a time and score the
//! output against ground truth. Runs only with the non-default `inprocess`
//! cargo feature.
//!
//! These are deliberately lightweight "does it run?" checks: they confirm the
//! simulation + tracker handle simultaneous targets at all (two insects present
//! together are both reconstructed, runs stay deterministic, and staggered
//! entry/exit windows are honored). They are intentionally lenient about track
//! stability (fragmentation, ID switches) because two insects sharing one arena
//! will occasionally pass close enough to challenge data association — tightening
//! those into correctness/benchmark thresholds is future work.
#![cfg(feature = "inprocess")]

use braid_sim::Scenario;
use braid_sim::scenario::{
    Arena, BlobParams, CameraRig, InsectSpec, Lissajous, ObservationModel, TimingModel,
};

/// A smooth, bounded Lissajous path. Distinct `freq`/`phase` per insect trace
/// different curves through the shared arena.
fn motion(freq_hz: [f64; 3], phase: [f64; 3]) -> Lissajous {
    Lissajous {
        freq_hz,
        phase,
        fill: 0.6,
        maneuver_amp_m: 0.0,
        maneuver_freq_hz: 0.0,
    }
}

/// A multi-camera scenario carrying an arbitrary set of insects. The arena and
/// camera rig match the single-insect injector test so behavior is comparable.
fn scenario(insects: Vec<InsectSpec>, observation: ObservationModel) -> Scenario {
    Scenario {
        seed: 11,
        fps: 100.0,
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
        insects,
        blob: BlobParams::default(),
        bg_warmup_frames: 0,
        timing: TimingModel::default(),
        observation,
        reported_fps: None,
        calibration_perturbation: Default::default(),
    }
}

/// Two insects on distinct paths, both present for the whole run.
fn two_insects() -> Vec<InsectSpec> {
    vec![
        InsectSpec {
            id: 1,
            enter_t: 0.0,
            exit_t: None,
            motion: motion([0.11, 0.13, 0.07], [0.0, 1.0, 2.0]),
        },
        InsectSpec {
            id: 2,
            enter_t: 0.0,
            exit_t: None,
            motion: motion([0.07, 0.09, 0.12], [3.1, 4.2, 0.5]),
        },
    ]
}

/// With perfect detections, two simultaneous insects are both reconstructed
/// accurately and largely completely. Coverage is aggregated over all present
/// object-frames, so with both insects present for the whole run, coverage well
/// above 0.5 means *neither* insect was dropped — i.e. both were genuinely
/// tracked at once.
#[tokio::test]
async fn two_insects_track_accurately() -> eyre::Result<()> {
    let s = scenario(two_insects(), ObservationModel::default());
    let tmp = tempfile::tempdir()?;
    let out = tmp.path().join("two.braid");

    let braidz = braid_sim::inject::inject_and_track(&s, 300, None, &out).await?;

    let score = braid_sim::truth::score_against_truth(&braidz, &s, 0.02, 5)?;
    eprintln!("{}", score.report());

    assert_eq!(score.num_truth, 2);
    // At least one track per insect must survive.
    assert!(
        score.num_tracks >= 2,
        "expected >=2 tracks for 2 insects, got {}",
        score.num_tracks
    );
    assert!(score.num_matched > 0, "no frames matched ground truth");
    assert!(score.rmse_m < 0.01, "rmse too high: {} m", score.rmse_m);
    // > 0.5 is only reachable if both simultaneously-present insects are tracked.
    assert!(
        score.coverage > 0.8,
        "coverage too low for both insects: {}",
        score.coverage
    );
    Ok(())
}

/// The injector stays deterministic with multiple insects, even under detection
/// imperfections: same `(scenario, seed)` -> identical scored tracks.
#[tokio::test]
async fn multi_object_injection_is_deterministic() -> eyre::Result<()> {
    let s = scenario(
        two_insects(),
        ObservationModel {
            pixel_noise_px: 0.3,
            dropout_prob: 0.05,
            clutter_per_frame: 0.2,
            ..Default::default()
        },
    );

    let run = |dir: std::path::PathBuf| {
        let s = s.clone();
        async move {
            let braidz = braid_sim::inject::inject_and_track(&s, 200, None, &dir).await?;
            braid_sim::truth::score_against_truth(&braidz, &s, 0.02, 5)
        }
    };

    let tmp = tempfile::tempdir()?;
    let a = run(tmp.path().join("a.braid")).await?;
    let b = run(tmp.path().join("b.braid")).await?;

    assert_eq!(a, b, "multi-object injection must be deterministic");
    assert_eq!(a.num_truth, 2);
    assert!(
        a.coverage > 0.6,
        "coverage too low under noise: {}",
        a.coverage
    );
    Ok(())
}

/// Staggered entry/exit: a second insect appears partway through and the first
/// leaves before the end, so the count of simultaneously-present insects varies
/// over the run (0 -> 1 -> 2 -> 1). The tracker should handle the changing
/// population and still recover both insects' tracks.
#[tokio::test]
async fn staggered_entry_exit_is_tracked() -> eyre::Result<()> {
    let insects = vec![
        InsectSpec {
            id: 1,
            enter_t: 0.0,
            exit_t: Some(2.0),
            motion: motion([0.11, 0.13, 0.07], [0.0, 1.0, 2.0]),
        },
        InsectSpec {
            id: 2,
            enter_t: 1.0,
            exit_t: None,
            motion: motion([0.07, 0.09, 0.12], [3.1, 4.2, 0.5]),
        },
    ];
    let s = scenario(insects, ObservationModel::default());
    let tmp = tempfile::tempdir()?;
    let out = tmp.path().join("staggered.braid");

    // 300 frames @ 100 fps = 3 s: insect 1 present [0,2), insect 2 present [1,3).
    let braidz = braid_sim::inject::inject_and_track(&s, 300, None, &out).await?;

    let score = braid_sim::truth::score_against_truth(&braidz, &s, 0.02, 5)?;
    eprintln!("{}", score.report());

    assert_eq!(score.num_truth, 2);
    // Both insects appear at distinct times, so each needs at least its own track.
    assert!(
        score.num_tracks >= 2,
        "expected >=2 tracks, got {}",
        score.num_tracks
    );
    assert!(score.num_matched > 0, "no frames matched ground truth");
    assert!(score.rmse_m < 0.01, "rmse too high: {} m", score.rmse_m);
    // Both insects are present for most of the run; both being tracked keeps
    // aggregate coverage high.
    assert!(score.coverage > 0.8, "coverage too low: {}", score.coverage);
    Ok(())
}
