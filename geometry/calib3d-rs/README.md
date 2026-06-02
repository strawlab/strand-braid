# calib3d-rs

Pure-Rust, dependency-free reimplementations of the small set of OpenCV
routines that strand-braid uses for camera calibration. The goal is to
eventually replace the C++ `opencv-calibrate` crate (and its OpenCV build
dependency) with equivalent Rust code.

The OpenCV surface being replaced is:

- `cv::cornerSubPix` — sub-pixel corner refinement (**implemented here**, see
  [`corner_subpix`]).
- `cv::findChessboardCorners` — chessboard corner detection (todo).
- `cv::calibrateCamera` — intrinsic/distortion calibration (todo).

## Validation

Each routine is validated two ways:

1. **Synthetic unit tests** in this crate (no OpenCV), checking known-answer
   behavior.
2. **Cross-checks against OpenCV** living in the `opencv-calibrate` crate (which
   links OpenCV), comparing this implementation against OpenCV on the committed
   `left*.jpg` sample images within a documented pixel tolerance.

This crate itself has no OpenCV dependency.
