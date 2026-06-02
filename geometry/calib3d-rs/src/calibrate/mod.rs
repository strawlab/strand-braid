//! Pure-Rust camera calibration (port of OpenCV `cv::calibrateCamera`).
//!
//! Work in progress, built bottom-up:
//!   1. per-view homography estimation ([`homography`]) — done,
//!   2. initial intrinsics (Zhang) — todo,
//!   3. initial extrinsics per view — todo,
//!   4. joint Levenberg-Marquardt refinement of intrinsics, distortion, and
//!      extrinsics — todo.

mod homography;
mod intrinsics;

pub use homography::find_homography;
pub use intrinsics::{InitialIntrinsics, init_intrinsics};
