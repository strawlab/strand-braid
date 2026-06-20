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

use crate::scenario::Scenario;

/// Build the synthetic calibration: `count` ideal-pinhole cameras evenly spaced
/// on a horizontal ring around the arena center, each looking at the center.
///
/// The same calibration is used both to project ground truth (generate
/// observations) and, after serialization, by Braid to reconstruct — so in the
/// perfect-world baseline the reconstruction can recover ground truth exactly up
/// to numerical precision.
pub fn build_calibration(scenario: &Scenario) -> eyre::Result<FlydraMultiCameraSystem<f64>> {
    let c = scenario.arena.center();
    let center = Vector3::new(c[0], c[1], c[2]);
    let up = Unit::new_normalize(Vector3::new(0.0, 0.0, 1.0));
    let rig = &scenario.cameras;

    let cx = rig.image_width as f64 / 2.0;
    let cy = rig.image_height as f64 / 2.0;

    let mut cams_by_name = BTreeMap::new();
    for k in 0..rig.count {
        let angle = 2.0 * PI * (k as f64) / (rig.count as f64);
        let camcenter = Vector3::new(
            c[0] + rig.radius_m * angle.cos(),
            c[1] + rig.radius_m * angle.sin(),
            rig.height_m,
        );
        let extrinsics = ExtrinsicParameters::from_view(&camcenter, &center, &up);
        let intrinsics =
            RosOpenCvIntrinsics::from_params(rig.focal_length_px, 0.0, rig.focal_length_px, cx, cy);
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
