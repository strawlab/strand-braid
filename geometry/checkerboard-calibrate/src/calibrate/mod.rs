// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Pure-Rust camera calibration (port of OpenCV `cv::calibrateCamera`).
//!
//! Built bottom-up:
//!   1. per-view homography estimation ([`find_homography`]),
//!   2. initial intrinsics ([`init_intrinsics`]),
//!   3. initial extrinsics per view ([`init_extrinsics`]),
//!   4. joint Levenberg-Marquardt refinement of intrinsics, distortion, and
//!      extrinsics ([`calibrate_camera`]).

mod extrinsics;
mod homography;
mod intrinsics;
mod solver;

pub use extrinsics::{Extrinsics, init_extrinsics};
pub use homography::find_homography;
pub use intrinsics::{InitialIntrinsics, init_intrinsics};
pub use solver::{CalibrateError, CalibrationResult, CorrespondingPoint, calibrate_camera};
