// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! M2 verification: drive the `ci2-sim` backend and feed its rendered frames
//! through the *real* `flydra-feature-detector`, confirming the simulated
//! insect is detected at the location `braid-sim` projects it to — i.e. the M0
//! blob->detector contract holds end-to-end through the sim camera.

use chrono::{DateTime, TimeDelta, Utc};
use ci2::{Camera, CameraModule};
use flydra_feature_detector::{BackgroundUpdateMode, FlydraFeatureDetector, TimingInfo, UfmfState};

use braid_sim::Scenario;
use braid_sim::calibration::build_calibration;
use braid_sim::projection::project_pixel;
use braid_sim::world::World;

// A small, fast scenario (no pacing is exercised: we never call
// acquisition_start, so next_frame does not sleep).
const SIM_TOML: &str = r#"
seed = 1
fps = 200.0
[arena]
min = [-0.15, -0.15, 0.0]
max = [0.15, 0.15, 0.30]
[cameras]
count = 3
radius_m = 0.6
height_m = 0.7
focal_length_px = 450.0
image_width = 320
image_height = 256
[blob]
peak = 160
sigma = 1.5
background = 0
[[insects]]
id = 1
[insects.motion]
freq_hz = [0.11, 0.13, 0.07]
phase = [0.0, 1.0, 2.0]
fill = 0.7
"#;

#[test]
fn sim_camera_frames_are_detected_at_projected_location() -> eyre::Result<()> {
    // Point the backend at a temporary scenario file.
    let dir = std::env::temp_dir();
    let path = dir.join(format!("braid-sim-m2-{}.toml", std::process::id()));
    std::fs::write(&path, SIM_TOML)?;
    // SAFETY: single-threaded test; the variable is read by ci2-sim during this
    // test only.
    unsafe { std::env::set_var(ci2_sim::SIM_SPEC_ENV, &path) };

    let scenario = Scenario::from_toml_str(SIM_TOML)?;
    let warmup = scenario.bg_warmup_frames as usize;
    let dt = 1.0 / scenario.fps;

    // Independent ground truth for validation.
    let system = build_calibration(&scenario)?;
    let world = World::new(scenario.clone());
    let cam_name = Scenario::camera_name(0);

    // Open the sim camera through the ci2 module interface.
    let module = ci2_sim::new_module()?;
    let mut module_ref: &ci2_sim::WrappedModule = &module;

    let names: Vec<String> = module_ref
        .camera_infos()?
        .iter()
        .map(|i| i.name().to_string())
        .collect();
    assert_eq!(names, vec!["simcam0", "simcam1", "simcam2"]);

    let mut cam = module_ref.camera(&cam_name)?;
    assert_eq!(cam.width()?, 320);
    assert_eq!(cam.height()?, 256);

    // Real feature detector with default (absdiff) config.
    let mut ft = FlydraFeatureDetector::new(
        &braid_types::RawCamName::new(cam_name.clone()),
        cam.width()?,
        cam.height()?,
        flydra_pt_detect_cfg::default_absdiff(),
        None,
        None,
        BackgroundUpdateMode::Synchronous,
    )?;

    let base: DateTime<Utc> = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
    let n_insect_frames = 30usize;
    let mut detected_frames = 0usize;
    let mut checked_frames = 0usize;

    for fno in 0..(warmup + n_insect_frames) {
        let frame = cam.next_frame()?;
        let ts = base + TimeDelta::milliseconds((fno as f64 * dt * 1000.0) as i64);
        let pts = ft
            .process_new_frame(
                &frame.image.borrow(),
                UfmfState::Stopped,
                TimingInfo::minimal(fno, ts),
            )?
            .0
            .points;

        if fno < warmup {
            // Background-warmup phase: no insect rendered, so no detections.
            assert!(
                pts.is_empty(),
                "frame {fno} (warmup) unexpectedly produced {} detections",
                pts.len()
            );
            continue;
        }

        // Insect present: compare against the independently-projected center.
        let t = (fno - warmup) as f64 * dt;
        let truth = world.state_at(t);
        assert_eq!(truth.len(), 1);
        let expected = project_pixel(&system, &cam_name, 320, 256, &truth[0].pos)
            .expect("insect must be in view of simcam0");

        checked_frames += 1;
        if let Some(p) = pts.first() {
            let err = ((p.x0_abs - expected.0).powi(2) + (p.y0_abs - expected.1).powi(2)).sqrt();
            assert!(
                err < 1.5,
                "frame {fno}: detection ({:.2},{:.2}) far from projected ({:.2},{:.2}), err={err:.2}px",
                p.x0_abs,
                p.y0_abs,
                expected.0,
                expected.1
            );
            detected_frames += 1;
        }
    }

    // Essentially every post-warmup frame should be detected.
    assert!(
        detected_frames as f64 >= 0.95 * checked_frames as f64,
        "only {detected_frames}/{checked_frames} insect frames detected"
    );

    let _ = std::fs::remove_file(&path);
    Ok(())
}
