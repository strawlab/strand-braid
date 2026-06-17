// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Project 3D ground-truth points into each camera's distorted pixel, with
//! field-of-view culling.

use braid_mvg::PointWorldFrame;
use flydra_mvg::FlydraMultiCameraSystem;

use crate::scenario::Scenario;

/// A single camera's observation of a 3D point: the distorted pixel, or `None`
/// if the point projects outside the image (culled).
#[derive(Debug, Clone, PartialEq)]
pub struct Observation {
    /// Camera name (see [`Scenario::camera_name`]).
    pub cam_name: String,
    /// Pixel `(x, y)`, or `None` if outside the image bounds.
    pub pixel: Option<(f64, f64)>,
}

/// Whether pixel `(x, y)` lies within a `width` x `height` image.
fn in_image(x: f64, y: f64, width: usize, height: usize) -> bool {
    x >= 0.0 && y >= 0.0 && x < width as f64 && y < height as f64
}

/// Project a 3D point into a single named camera, returning the pixel `(x, y)`
/// or `None` if the camera is unknown or the point lands outside a
/// `width` x `height` image.
pub fn project_pixel(
    system: &FlydraMultiCameraSystem<f64>,
    cam_name: &str,
    width: usize,
    height: usize,
    pt: &PointWorldFrame<f64>,
) -> Option<(f64, f64)> {
    let cam = system.cam_by_name(cam_name)?;
    let dp = cam.project_3d_to_distorted_pixel(pt);
    let (x, y) = (dp.coords.x, dp.coords.y);
    if in_image(x, y, width, height) {
        Some((x, y))
    } else {
        None
    }
}

/// Project a 3D point into every camera of `system`, culling points that land
/// outside the image.
///
/// Note: this culls on image bounds only. For the perfect-world ring geometry,
/// all arena points are in front of all cameras; if cameras are later placed so
/// that points can fall behind a camera, add a camera-frame depth (`z > 0`)
/// check here.
pub fn project_all(
    system: &FlydraMultiCameraSystem<f64>,
    scenario: &Scenario,
    pt: &PointWorldFrame<f64>,
) -> Vec<Observation> {
    (0..scenario.cameras.count)
        .map(|k| {
            let cam_name = Scenario::camera_name(k);
            let pixel = system.cam_by_name(&cam_name).and_then(|cam| {
                let dp = cam.project_3d_to_distorted_pixel(pt);
                let (x, y) = (dp.coords.x, dp.coords.y);
                if in_image(
                    x,
                    y,
                    scenario.cameras.image_width,
                    scenario.cameras.image_height,
                ) {
                    Some((x, y))
                } else {
                    None
                }
            });
            Observation { cam_name, pixel }
        })
        .collect()
}
