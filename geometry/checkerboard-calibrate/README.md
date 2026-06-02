# calib3d-rs

Pure-Rust reimplementations of the small set of OpenCV routines that
strand-braid uses for camera calibration. The goal is to eventually replace the
C++ `opencv-calibrate` crate (and its OpenCV build dependency) with equivalent
Rust code. The corner-refinement code is dependency-free; the calibration code
uses `nalgebra` for linear algebra.

The OpenCV surface being replaced is:

- `cv::cornerSubPix` — sub-pixel corner refinement (**done**, see
  [`corner_subpix`]).
- `cv::calibrateCamera` — intrinsic/distortion calibration (**done**, see the
  [`calibrate`] module: homography → initial intrinsics → per-view extrinsics →
  joint Levenberg-Marquardt refinement).
- `cv::findChessboardCorners` — chessboard corner detection (todo).

## Validation

Each routine is validated two ways:

1. **Synthetic unit tests** in this crate (no OpenCV), checking known-answer
   behavior.
2. **Cross-checks against OpenCV** living in the `opencv-calibrate` crate (which
   links OpenCV), comparing this implementation against OpenCV on the committed
   `left*.jpg` sample images within a documented pixel tolerance.

This crate itself has no OpenCV dependency.
