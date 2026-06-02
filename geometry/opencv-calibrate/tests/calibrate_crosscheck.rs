//! Cross-check the pure-Rust `checkerboard_calibrate::calibrate::calibrate_camera` against
//! OpenCV's calibration golden.
//!
//! Both calibrations consume the identical OpenCV-detected corners from the
//! sample frames and the identical synthetic planar object grid, so this
//! isolates the calibration solver. The OpenCV result is the committed golden
//! `tests/data/golden/calibration.json` (produced by `calibration.rs`).

use std::path::{Path, PathBuf};

use checkerboard_calibrate::calibrate::{CorrespondingPoint, calibrate_camera};
use image::GenericImageView;

const FRAMES: &[&str] = &[
    "left01.jpg",
    "left02.jpg",
    "left03.jpg",
    "left04.jpg",
    "left05.jpg",
    "left06.jpg",
    "left07.jpg",
    "left08.jpg",
    "left09.jpg",
    "left11.jpg",
    "left12.jpg",
    "left13.jpg",
    "left14.jpg",
];

const COLS: usize = 9;
const ROWS: usize = 6;
const IMG_W: u32 = 640;
const IMG_H: u32 = 480;

// Agreement vs OpenCV. Both solvers minimize the same reprojection cost to the
// same optimum from different initializations and LM implementations, yet agree
// to ~1e-7 px on focals and ~1e-9 on distortion. These bounds keep a large
// margin over the observed differences for robustness across platforms/BLAS.
const TOL_FOCAL_PX: f64 = 1e-3;
const TOL_CENTER_PX: f64 = 1e-3;
const TOL_DISTORTION: f64 = 1e-5;
const TOL_RMS_PX: f64 = 1e-5;

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}

fn correspondences(file: &str) -> Vec<CorrespondingPoint> {
    let path = data_dir().join(file);
    let img = image::open(&path).unwrap();
    let (w, h) = img.dimensions();
    let rgb = img.to_rgb8().into_raw();
    let corners = opencv_calibrate::find_chessboard_corners(&rgb, w, h, COLS, ROWS)
        .unwrap()
        .unwrap_or_else(|| panic!("no board in {file}"));
    assert_eq!(corners.len(), COLS * ROWS);

    corners
        .iter()
        .enumerate()
        .map(|(i, &(x, y))| CorrespondingPoint {
            object_point: ((i % COLS) as f64, (i / COLS) as f64, 0.0),
            image_point: (x as f64, y as f64),
        })
        .collect()
}

#[test]
fn pure_calibration_matches_opencv() {
    let views: Vec<Vec<CorrespondingPoint>> = FRAMES.iter().map(|f| correspondences(f)).collect();

    let res = calibrate_camera(&views, IMG_W, IMG_H).expect("pure calibration");

    let text = std::fs::read_to_string(data_dir().join("golden").join("calibration.json")).unwrap();
    let g: serde_json::Value = serde_json::from_str(&text).unwrap();
    let gm: Vec<f64> = g["camera_matrix"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();
    let gd: Vec<f64> = g["distortion_coeffs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();
    let g_rms = g["mean_reprojection_distance_pixels"].as_f64().unwrap();

    eprintln!(
        "pure : fx={:.4} fy={:.4} cx={:.4} cy={:.4} k1={:.5} k2={:.5} p1={:.5} p2={:.5} rms={:.5}",
        res.camera_matrix[0],
        res.camera_matrix[4],
        res.camera_matrix[2],
        res.camera_matrix[5],
        res.distortion_coeffs[0],
        res.distortion_coeffs[1],
        res.distortion_coeffs[2],
        res.distortion_coeffs[3],
        res.rms_reprojection_error,
    );
    eprintln!(
        "cv   : fx={:.4} fy={:.4} cx={:.4} cy={:.4} k1={:.5} k2={:.5} p1={:.5} p2={:.5} rms={:.5}",
        gm[0], gm[4], gm[2], gm[5], gd[0], gd[1], gd[2], gd[3], g_rms,
    );

    eprintln!(
        "abs diffs: fx={:.2e} fy={:.2e} cx={:.2e} cy={:.2e} k1={:.2e} k2={:.2e} p1={:.2e} p2={:.2e} rms={:.2e}",
        (res.camera_matrix[0] - gm[0]).abs(),
        (res.camera_matrix[4] - gm[4]).abs(),
        (res.camera_matrix[2] - gm[2]).abs(),
        (res.camera_matrix[5] - gm[5]).abs(),
        (res.distortion_coeffs[0] - gd[0]).abs(),
        (res.distortion_coeffs[1] - gd[1]).abs(),
        (res.distortion_coeffs[2] - gd[2]).abs(),
        (res.distortion_coeffs[3] - gd[3]).abs(),
        (res.rms_reprojection_error - g_rms).abs(),
    );

    approx::assert_abs_diff_eq!(res.camera_matrix[0], gm[0], epsilon = TOL_FOCAL_PX);
    approx::assert_abs_diff_eq!(res.camera_matrix[4], gm[4], epsilon = TOL_FOCAL_PX);
    approx::assert_abs_diff_eq!(res.camera_matrix[2], gm[2], epsilon = TOL_CENTER_PX);
    approx::assert_abs_diff_eq!(res.camera_matrix[5], gm[5], epsilon = TOL_CENTER_PX);
    approx::assert_abs_diff_eq!(res.distortion_coeffs[0], gd[0], epsilon = TOL_DISTORTION);
    approx::assert_abs_diff_eq!(res.distortion_coeffs[1], gd[1], epsilon = TOL_DISTORTION);
    approx::assert_abs_diff_eq!(res.distortion_coeffs[2], gd[2], epsilon = TOL_DISTORTION);
    approx::assert_abs_diff_eq!(res.distortion_coeffs[3], gd[3], epsilon = TOL_DISTORTION);
    approx::assert_abs_diff_eq!(res.rms_reprojection_error, g_rms, epsilon = TOL_RMS_PX);
}
