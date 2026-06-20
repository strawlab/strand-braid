// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Native parametric calibration format for single cameras.
//!
//! Unlike the flydra XML format (which stores intrinsics twice — once as a 3×4
//! projection matrix and once as the distortion model's `fc1,fc2,cc1,cc2` — and
//! reconciles the two with an abused rectification matrix), this format stores a
//! single, fully broken-down set of parameters per camera:
//!
//! - intrinsics: `fx, fy, cx, cy, skew` plus an explicit, extensible distortion
//!   model ([`NativeIntrinsics`]),
//! - extrinsics: a rotation unit quaternion and a translation vector.
//!
//! A [`Camera`] is faithfully representable in this format if and only if its
//! rectification matrix is the identity (see [`Camera::is_native_representable`]).
//! When that holds, the linear intrinsics and the distortion-model intrinsics
//! agree and a single parameter set describes the camera exactly.

use serde::{Deserialize, Serialize};

use nalgebra::{Point3, Quaternion, RealField, UnitQuaternion, Vector5};

use opencv_ros_camera::{Distortion, RosOpenCvIntrinsics};

use crate::{Camera, MvgError, Result};

/// Tolerance for the "rectification matrix is identity" representability check.
const RECT_IDENTITY_EPSILON: f64 = 1.0e-7;

fn zero_r<R: RealField>() -> R {
    nalgebra::convert(0.0)
}

/// Intrinsic camera model, fully broken down into named parameters.
///
/// This is a tagged enum (serialized with a `model` key) so that additional
/// camera models with entirely different parameters (e.g. EUCM,
/// Kannala-Brandt) can be added as new variants without changing the existing
/// ones.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "model", rename_all = "kebab-case")]
pub enum NativeIntrinsics<R: RealField> {
    /// Linear pinhole model with no lens distortion.
    Pinhole {
        /// Focal length, x (pixels).
        fx: R,
        /// Focal length, y (pixels).
        fy: R,
        /// Principal point, x (pixels).
        cx: R,
        /// Principal point, y (pixels).
        cy: R,
        /// Axis skew (`k[(0,1)]`); usually zero.
        #[serde(default = "zero_r")]
        skew: R,
    },
    /// Pinhole model with OpenCV ("Brown-Conrady") radial and tangential
    /// distortion.
    OpencvBrownConrady {
        /// Focal length, x (pixels).
        fx: R,
        /// Focal length, y (pixels).
        fy: R,
        /// Principal point, x (pixels).
        cx: R,
        /// Principal point, y (pixels).
        cy: R,
        /// Axis skew (`k[(0,1)]`); usually zero.
        #[serde(default = "zero_r")]
        skew: R,
        /// Radial distortion coefficient k1.
        k1: R,
        /// Radial distortion coefficient k2.
        k2: R,
        /// Radial distortion coefficient k3.
        #[serde(default = "zero_r")]
        k3: R,
        /// Tangential distortion coefficient p1.
        p1: R,
        /// Tangential distortion coefficient p2.
        p2: R,
    },
}

/// Camera extrinsic parameters, broken down into rotation and translation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NativeExtrinsics<R: RealField> {
    /// Rotation as a unit quaternion in `[w, x, y, z]` order.
    pub quaternion: [R; 4],
    /// Translation vector `[x, y, z]` (the `t` in `x = K[R|t]X`).
    pub translation: [R; 3],
}

/// A single camera calibration in the native parametric format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NativeCameraCalibration<R: RealField> {
    /// Camera identifier.
    pub name: String,
    /// Image width (pixels).
    pub width: usize,
    /// Image height (pixels).
    pub height: usize,
    /// Intrinsic parameters.
    pub intrinsics: NativeIntrinsics<R>,
    /// Extrinsic parameters.
    pub extrinsics: NativeExtrinsics<R>,
}

impl<R: RealField + Copy + serde::Serialize> Camera<R> {
    /// Returns `true` if this camera can be faithfully represented in the native
    /// parametric calibration format.
    ///
    /// This holds exactly when the rectification matrix is the identity. A
    /// non-identity rectification matrix indicates the "dual-copy" intrinsics
    /// problem (the linear projection intrinsics differ from the distortion
    /// model intrinsics), which the native format intentionally cannot express.
    pub fn is_native_representable(&self) -> bool {
        self.intrinsics()
            .rect
            .is_identity(nalgebra::convert(RECT_IDENTITY_EPSILON))
    }

    /// Convert this camera to the native parametric calibration format.
    ///
    /// Returns [`MvgError::RectificationMatrixNotSupported`] if the camera is
    /// not representable (see [`Camera::is_native_representable`]).
    pub fn to_native(&self, name: &str) -> Result<NativeCameraCalibration<R>> {
        if !self.is_native_representable() {
            return Err(MvgError::RectificationMatrixNotSupported);
        }

        let k = &self.intrinsics().k;
        let fx = k[(0, 0)];
        let skew = k[(0, 1)];
        let fy = k[(1, 1)];
        let cx = k[(0, 2)];
        let cy = k[(1, 2)];

        let d = &self.intrinsics().distortion;
        let intrinsics = if d.is_linear() {
            NativeIntrinsics::Pinhole {
                fx,
                fy,
                cx,
                cy,
                skew,
            }
        } else {
            NativeIntrinsics::OpencvBrownConrady {
                fx,
                fy,
                cx,
                cy,
                skew,
                k1: d.radial1(),
                k2: d.radial2(),
                k3: d.radial3(),
                p1: d.tangential1(),
                p2: d.tangential2(),
            }
        };

        let q = UnitQuaternion::from_rotation_matrix(self.extrinsics().rotation());
        let t = self.extrinsics().translation();
        let extrinsics = NativeExtrinsics {
            quaternion: [q.w, q.i, q.j, q.k],
            translation: [t.x, t.y, t.z],
        };

        Ok(NativeCameraCalibration {
            name: name.to_string(),
            width: self.width(),
            height: self.height(),
            intrinsics,
            extrinsics,
        })
    }

    /// Build a camera from the native parametric calibration format.
    ///
    /// Returns the camera name together with the [`Camera`].
    pub fn from_native(cal: &NativeCameraCalibration<R>) -> Result<(String, Self)> {
        let (fx, fy, cx, cy, skew, distortion) = match &cal.intrinsics {
            NativeIntrinsics::Pinhole {
                fx,
                fy,
                cx,
                cy,
                skew,
            } => (
                *fx,
                *fy,
                *cx,
                *cy,
                *skew,
                Distortion::from_opencv_vec(Vector5::zeros()),
            ),
            NativeIntrinsics::OpencvBrownConrady {
                fx,
                fy,
                cx,
                cy,
                skew,
                k1,
                k2,
                k3,
                p1,
                p2,
            } => (
                *fx,
                *fy,
                *cx,
                *cy,
                *skew,
                Distortion::from_opencv_vec(Vector5::new(*k1, *k2, *p1, *p2, *k3)),
            ),
        };

        let intrinsics =
            RosOpenCvIntrinsics::from_params_with_distortion(fx, skew, fy, cx, cy, distortion);

        let q = &cal.extrinsics.quaternion;
        let rquat = UnitQuaternion::from_quaternion(Quaternion::new(q[0], q[1], q[2], q[3]));
        let t = &cal.extrinsics.translation;
        let translation = Point3::new(t[0], t[1], t[2]);
        let extrinsics = crate::extrinsics::from_rquat_translation(rquat, translation);

        let cam = Self::new(cal.width, cal.height, extrinsics, intrinsics)?;
        Ok((cal.name.clone(), cam))
    }
}

#[cfg(test)]
mod tests {
    use crate::Camera;

    #[test]
    fn roundtrip_native_cameras() {
        for (name, cam) in crate::tests::get_test_cameras() {
            if !cam.is_native_representable() {
                // Cameras with a non-identity rect matrix cannot round-trip
                // through the native format by design.
                continue;
            }
            let native = cam.to_native(&name).unwrap();
            let (name2, cam2) = Camera::from_native(&native).unwrap();
            assert_eq!(name, name2);
            assert_eq!(cam.width(), cam2.width());
            assert_eq!(cam.height(), cam2.height());
            // Intrinsics and extrinsics should match within tight tolerance.
            approx::assert_relative_eq!(cam.intrinsics().k, cam2.intrinsics().k, epsilon = 1e-10);
            approx::assert_relative_eq!(
                cam.extrinsics().matrix(),
                cam2.extrinsics().matrix(),
                epsilon = 1e-10
            );
        }
    }
}
