// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Level-B detection injector test (plan §5): drive the real flydra2 tracker
//! in-process with synthetic 2D detections and score the output against ground
//! truth. Runs only with the non-default `inprocess` cargo feature.
#![cfg(feature = "inprocess")]

use braid_sim::Scenario;
use braid_sim::scenario::{
    Arena, BlobParams, CameraRig, InsectSpec, Lissajous, ObservationModel, TimingModel,
};

/// A multi-camera scenario with one smoothly-moving insect.
fn scenario(observation: ObservationModel) -> Scenario {
    Scenario {
        seed: 7,
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
        insects: vec![InsectSpec {
            id: 1,
            enter_t: 0.0,
            exit_t: None,
            motion: Lissajous {
                freq_hz: [0.11, 0.13, 0.07],
                phase: [0.0, 1.0, 2.0],
                fill: 0.6,
                maneuver_amp_m: 0.0,
                maneuver_freq_hz: 0.0,
            },
        }],
        blob: BlobParams::default(),
        bg_warmup_frames: 0,
        timing: TimingModel::default(),
        observation,
        reported_fps: None,
    }
}

/// With perfect detections, the in-process tracker should reconstruct the
/// insect accurately, completely, and as a single stable track.
#[tokio::test]
async fn perfect_detections_track_accurately() -> eyre::Result<()> {
    let s = scenario(ObservationModel::default());
    let tmp = tempfile::tempdir()?;
    let out = tmp.path().join("track.braid");

    let braidz = braid_sim::inject::inject_and_track(&s, 300, None, &out).await?;

    // Score the written recording against ground truth.
    let score = braid_sim::truth::score_against_truth(&braidz, &s, 0.02, 5)?;
    eprintln!("{}", score.report());

    assert_eq!(score.num_truth, 1);
    assert!(score.num_matched > 0, "no frames matched ground truth");
    assert!(score.rmse_m < 0.005, "rmse too high: {} m", score.rmse_m);
    assert!(score.coverage > 0.9, "coverage too low: {}", score.coverage);
    // One insect -> ideally one unbroken track.
    assert!(
        score.mean_fragments < 2.0,
        "too fragmented: {} frags/insect",
        score.mean_fragments
    );
    Ok(())
}

/// The injector is deterministic: same `(scenario, seed)` -> identical tracks,
/// even with detection imperfections enabled.
#[tokio::test]
async fn injection_is_deterministic_with_imperfections() -> eyre::Result<()> {
    let s = scenario(ObservationModel {
        pixel_noise_px: 0.3,
        dropout_prob: 0.05,
        clutter_per_frame: 0.2,
        ..Default::default()
    });

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

    assert_eq!(a, b, "in-process injection must be deterministic");
    // Even with imperfections, tracking should still largely succeed.
    assert!(
        a.coverage > 0.7,
        "coverage too low under noise: {}",
        a.coverage
    );
    Ok(())
}
