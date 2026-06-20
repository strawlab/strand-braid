// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Calibration-perturbation knob, validated end-to-end through the in-process
//! flydra2 tracker: the detections are projected with the *perfect*
//! generation calibration, but the tracker reconstructs with a *perturbed* one.
//! A nonzero perturbation must raise the reconstruction error against ground
//! truth (realistic reprojection error) while tracking still largely succeeds.
//! Deterministic, so `(scenario, seed)` reproduces both runs exactly.
//!
//! Runs only with the non-default `inprocess` cargo feature.
#![cfg(feature = "inprocess")]

use braid_sim::Scenario;
use braid_sim::scenario::{
    Arena, BlobParams, CalibrationPerturbation, CameraRig, InsectSpec, Lissajous, ObservationModel,
    TimingModel,
};

/// A smoothly-moving insect viewed by a 5-camera ring, with a configurable
/// tracking-calibration perturbation.
fn scenario(calibration_perturbation: CalibrationPerturbation) -> Scenario {
    Scenario {
        seed: 3,
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
        observation: ObservationModel::default(),
        reported_fps: None,
        calibration_perturbation,
    }
}

async fn track_rmse(s: &Scenario, label: &str) -> eyre::Result<braid_sim::truth::GroundTruthScore> {
    let tmp = tempfile::tempdir()?;
    let out = tmp.path().join("track.braid");
    let braidz = braid_sim::inject::inject_and_track(s, 300, None, &out).await?;
    let score = braid_sim::truth::score_against_truth(&braidz, s, 0.03, 5)?;
    eprintln!("[{label}] {}", score.report());
    Ok(score)
}

/// Perturbing the tracking calibration raises the reconstruction error against
/// ground truth (the detections were projected with the perfect calibration, so
/// triangulating with a wrong one is biased), yet the insect is still tracked
/// continuously. The control (perfect calibration) reconstructs near-exactly.
#[tokio::test]
async fn perturbation_raises_reconstruction_error_but_still_tracks() -> eyre::Result<()> {
    let perfect = track_rmse(&scenario(CalibrationPerturbation::default()), "perfect cal").await?;
    let perturbed = track_rmse(
        &scenario(CalibrationPerturbation {
            camera_position_m: 0.005,
            look_at_m: 0.005,
            focal_length_px: 3.0,
            principal_point_px: 2.0,
        }),
        "perturbed cal",
    )
    .await?;

    // Control: perfect calibration reconstructs ground truth near-exactly.
    assert!(
        perfect.rmse_m < 0.001,
        "perfect-cal RMSE unexpectedly high: {} m",
        perfect.rmse_m
    );

    // The perturbation has a real, measurable effect on reconstruction accuracy.
    assert!(
        perturbed.rmse_m > perfect.rmse_m * 5.0,
        "perturbation did not raise RMSE: perturbed {} m vs perfect {} m",
        perturbed.rmse_m,
        perfect.rmse_m
    );

    // ...but tracking does not fall apart: still well within the gate, covered,
    // and not fragmented into many tracks.
    assert!(
        perturbed.rmse_m < 0.02,
        "perturbed-cal RMSE too high to be a useful robustness test: {} m",
        perturbed.rmse_m
    );
    assert!(
        perturbed.coverage > 0.9,
        "perturbed-cal coverage too low: {}",
        perturbed.coverage
    );
    assert!(
        perturbed.mean_fragments < 3.0,
        "perturbed-cal over-fragmented: {} frags/insect",
        perturbed.mean_fragments
    );
    Ok(())
}

/// The in-process run is deterministic even with a perturbed calibration.
#[tokio::test]
async fn perturbed_run_is_deterministic() -> eyre::Result<()> {
    let s = scenario(CalibrationPerturbation {
        camera_position_m: 0.004,
        look_at_m: 0.004,
        focal_length_px: 2.0,
        principal_point_px: 1.5,
    });
    let a = track_rmse(&s, "perturbed a").await?;
    let b = track_rmse(&s, "perturbed b").await?;
    assert_eq!(a, b, "perturbed in-process run must be deterministic");
    Ok(())
}
