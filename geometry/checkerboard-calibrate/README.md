# calib3d-rs

Pure-Rust reimplementations of the small set of OpenCV routines that
strand-braid uses for camera calibration. These replace the former C++ OpenCV
backend, which has been removed, so the build no longer depends on OpenCV. The
corner-refinement code is dependency-free; the calibration code uses `nalgebra`
for linear algebra.

The OpenCV surface being replaced is:

- `cv::cornerSubPix` — sub-pixel corner refinement (**done**, see
  [`corner_subpix`]).
- `cv::calibrateCamera` — intrinsic/distortion calibration (**done**, see the
  [`calibrate`] module: homography → initial intrinsics → per-view extrinsics →
  joint Levenberg-Marquardt refinement).
- `cv::findChessboardCorners` — chessboard corner detection (todo).

## Validation

Each routine was validated two ways during development:

1. **Synthetic unit tests** in this crate (no OpenCV), checking known-answer
   behavior. These run as part of the normal test suite.
2. **Cross-checks against OpenCV** on the committed `left*.jpg` sample images
   within a documented pixel tolerance. These lived in a separate OpenCV-linking
   harness that has since been removed along with the C++ backend.

This crate has no OpenCV dependency.
