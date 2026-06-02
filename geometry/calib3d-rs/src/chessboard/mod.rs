//! Pure-Rust port of OpenCV's `cv::findChessboardCorners` (the classic
//! contour-based detector, not `findChessboardCornersSB`).
//!
//! This is the largest of the OpenCV routines being replaced. It is built
//! bottom-up:
//!   1. binarization primitives ([`binarize`]) — histogram equalization
//!      (`CALIB_CB_NORMALIZE_IMAGE`) and adaptive-mean thresholding
//!      (`CALIB_CB_ADAPTIVE_THRESH`) — **done**, bit-exact vs OpenCV,
//!   2. quad generation from contours (dilate, find contours, approximate
//!      polygons, keep convex quadrilaterals) — todo,
//!   3. linking quads into a board graph and ordering corners — todo,
//!   4. board validation (size, monotonicity) and corner extraction — todo.
//!
//! The detector flags requested by the strand-braid C++ wrapper are
//! `CALIB_CB_ADAPTIVE_THRESH | CALIB_CB_NORMALIZE_IMAGE | CALIB_CB_FAST_CHECK`.

mod binarize;

pub use binarize::{adaptive_threshold_mean, equalize_hist};
