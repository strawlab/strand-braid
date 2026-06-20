// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Generate a synthetic multi-camera calibration from a [`Scenario`] and
//! serialize it to the flydra XML format that Braid loads.

use std::collections::BTreeMap;
use std::f64::consts::PI;

use braid_mvg::Camera;
use cam_geom::ExtrinsicParameters;
use flydra_mvg::FlydraMultiCameraSystem;
use nalgebra::{Unit, Vector3};
use opencv_ros_camera::RosOpenCvIntrinsics;

use crate::scenario::{CalibrationPerturbation, Scenario};

/// Build the perfect *generation* calibration: `count` ideal-pinhole cameras
/// evenly spaced on a horizontal ring around the arena center, each looking at
/// the center.
///
/// This is the calibration used to project ground truth (generate observations),
/// i.e. "what was imaged". In the perfect-world baseline it is also the tracking
/// calibration (see [`build_tracking_calibration`]), so the reconstruction
/// recovers ground truth exactly up to numerical precision.
pub fn build_calibration(scenario: &Scenario) -> eyre::Result<FlydraMultiCameraSystem<f64>> {
    build(scenario, None)
}

/// Build the calibration Braid *tracks* with: the perfect generation calibration
/// of [`build_calibration`] with the scenario's
/// [`CalibrationPerturbation`](crate::scenario::CalibrationPerturbation) applied
/// (pose / intrinsic error). With the default (identity) perturbation this is
/// byte-identical to [`build_calibration`]; with a nonzero perturbation the
/// tracker reconstructs with a slightly wrong calibration while the detections
/// were generated with the perfect one, so reprojection error is realistic.
pub fn build_tracking_calibration(
    scenario: &Scenario,
) -> eyre::Result<FlydraMultiCameraSystem<f64>> {
    if scenario.calibration_perturbation.is_identity() {
        return build_calibration(scenario);
    }
    build(scenario, Some(&scenario.calibration_perturbation))
}

/// Build a ring calibration, optionally perturbing each camera's pose and
/// intrinsics by deterministic per-camera offsets.
fn build(
    scenario: &Scenario,
    perturbation: Option<&CalibrationPerturbation>,
) -> eyre::Result<FlydraMultiCameraSystem<f64>> {
    let c = scenario.arena.center();
    let up = Unit::new_normalize(Vector3::new(0.0, 0.0, 1.0));
    let rig = &scenario.cameras;

    let mut cams_by_name = BTreeMap::new();
    for k in 0..rig.count {
        let off = perturbation.map(|p| p.offsets(scenario.seed, k));
        let dpos = off.map_or([0.0; 3], |o| o.d_position_m);
        let dlook = off.map_or([0.0; 3], |o| o.d_look_at_m);
        let dfocal = off.map_or(0.0, |o| o.d_focal_px);
        let dcx = off.map_or(0.0, |o| o.d_cx_px);
        let dcy = off.map_or(0.0, |o| o.d_cy_px);

        let angle = 2.0 * PI * (k as f64) / (rig.count as f64);
        let camcenter = Vector3::new(
            c[0] + rig.radius_m * angle.cos() + dpos[0],
            c[1] + rig.radius_m * angle.sin() + dpos[1],
            rig.height_m + dpos[2],
        );
        // Perturbing the look-at target rotates the camera (a pointing error)
        // without moving its center.
        let target = Vector3::new(c[0] + dlook[0], c[1] + dlook[1], c[2] + dlook[2]);
        let extrinsics = ExtrinsicParameters::from_view(&camcenter, &target, &up);
        let f = rig.focal_length_px + dfocal;
        let cx = rig.image_width as f64 / 2.0 + dcx;
        let cy = rig.image_height as f64 / 2.0 + dcy;
        let intrinsics = RosOpenCvIntrinsics::from_params(f, 0.0, f, cx, cy);
        let cam = Camera::new(rig.image_width, rig.image_height, extrinsics, intrinsics)
            .map_err(|e| eyre::eyre!("building camera {k}: {e}"))?;
        cams_by_name.insert(Scenario::camera_name(k), cam);
    }

    Ok(FlydraMultiCameraSystem::new(cams_by_name, None))
}

/// Serialize a calibration to flydra XML (the format referenced by
/// `mainbrain.cal_fname`).
pub fn to_flydra_xml_string(system: &FlydraMultiCameraSystem<f64>) -> eyre::Result<String> {
    let mut buf = Vec::new();
    system
        .to_flydra_xml(&mut buf)
        .map_err(|e| eyre::eyre!("serializing calibration to flydra XML: {e}"))?;
    Ok(String::from_utf8(buf)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::CalibrationPerturbation;

    fn demo_scenario() -> Scenario {
        Scenario::from_toml_str(include_str!("../example-sim.toml")).unwrap()
    }

    /// With the default (identity) perturbation, the tracking calibration is the
    /// perfect generation calibration: their flydra-XML serializations match.
    #[test]
    fn identity_perturbation_matches_perfect() {
        let s = demo_scenario();
        assert!(s.calibration_perturbation.is_identity());
        let perfect = to_flydra_xml_string(&build_calibration(&s).unwrap()).unwrap();
        let tracking = to_flydra_xml_string(&build_tracking_calibration(&s).unwrap()).unwrap();
        assert_eq!(perfect, tracking);
    }

    /// A nonzero perturbation changes the tracking calibration (so reprojection
    /// error becomes nonzero) and is deterministic for a fixed `(scenario, seed)`.
    #[test]
    fn nonzero_perturbation_differs_and_is_deterministic() {
        let mut s = demo_scenario();
        s.calibration_perturbation = CalibrationPerturbation {
            camera_position_m: 0.005,
            look_at_m: 0.005,
            focal_length_px: 3.0,
            principal_point_px: 2.0,
        };
        let perfect = to_flydra_xml_string(&build_calibration(&s).unwrap()).unwrap();
        let tracking = to_flydra_xml_string(&build_tracking_calibration(&s).unwrap()).unwrap();
        assert_ne!(
            perfect, tracking,
            "perturbation should change the calibration"
        );

        // Deterministic: rebuilding yields the identical perturbed calibration.
        let again = to_flydra_xml_string(&build_tracking_calibration(&s).unwrap()).unwrap();
        assert_eq!(tracking, again);
    }
}
