//! End-to-end calibration conformance.
//!
//! Detects the chessboard in every sample frame, feeds the corners (paired with
//! a synthetic planar object grid) into [`opencv_calibrate::calibrate_camera`],
//! and pins the resulting intrinsics + distortion against a committed golden.
//!
//! Together with `conformance.rs` (which pins detection alone) this exercises
//! the full image -> calibration pipeline and gives a ground-truth target for a
//! future pure-Rust reimplementation of `calibrate_camera`.
//!
//! Regenerate the golden after an intentional change with:
//!
//! ```text
//! BLESS_GOLDEN=1 cargo test -p opencv-calibrate --test calibration
//! ```

use std::path::{Path, PathBuf};

use image::GenericImageView;
use opencv_calibrate::{CorrespondingPoint, calibrate_camera};

/// 9x6 inner corners, present in every `left*.jpg` frame.
const COLS: usize = 9;
const ROWS: usize = 6;
const IMG_W: i32 = 640;
const IMG_H: i32 = 480;

/// Frames that detect a full board (see `conformance.rs`).
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

/// Absolute tolerances. The current OpenCV build is deterministic well within
/// these; the slack exists so a future pure-Rust solver can be validated too.
const TOL_FOCAL_PX: f64 = 1.0;
const TOL_CENTER_PX: f64 = 1.0;
const TOL_DISTORTION: f64 = 1e-2;
const TOL_REPROJ_PX: f64 = 0.1;

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}

fn golden_path() -> PathBuf {
    data_dir().join("golden").join("calibration.json")
}

/// Object grid for one board: column-major within a row, matching the row-major
/// ordering that the detector returns corners in. Units are arbitrary (one
/// square == 1.0), which only scales the recovered translations, not the
/// intrinsics we check here.
fn detect_correspondences(file: &str) -> Vec<CorrespondingPoint> {
    let path = data_dir().join(file);
    let img = image::open(&path).unwrap_or_else(|e| panic!("open {}: {e}", path.display()));
    let (w, h) = img.dimensions();
    let rgb = img.to_rgb8().into_raw();
    let corners = opencv_calibrate::find_chessboard_corners(&rgb, w, h, COLS, ROWS)
        .unwrap_or_else(|e| panic!("detection error on {file}: {e}"))
        .unwrap_or_else(|| panic!("no board found in {file}"));
    assert_eq!(
        corners.len(),
        COLS * ROWS,
        "{file}: unexpected corner count"
    );

    corners
        .iter()
        .enumerate()
        .map(|(i, &(x, y))| {
            let col = (i % COLS) as f64;
            let row = (i / COLS) as f64;
            CorrespondingPoint {
                object_point: (col, row, 0.0),
                image_point: (x as f64, y as f64),
            }
        })
        .collect()
}

#[test]
fn calibration_matches_golden() {
    let all_pts: Vec<Vec<CorrespondingPoint>> =
        FRAMES.iter().map(|f| detect_correspondences(f)).collect();

    let result = calibrate_camera(&all_pts, IMG_W, IMG_H).expect("calibration failed");

    if std::env::var_os("BLESS_GOLDEN").is_some() {
        let doc = serde_json::json!({
            "image_width": result.image_width,
            "image_height": result.image_height,
            "mean_reprojection_distance_pixels": result.mean_reprojection_distance_pixels,
            "camera_matrix": result.camera_matrix,
            "distortion_coeffs": result.distortion_coeffs,
        });
        let path = golden_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, serde_json::to_string_pretty(&doc).unwrap() + "\n").unwrap();
        eprintln!("blessed golden: {}", path.display());
        return;
    }

    let text = std::fs::read_to_string(golden_path()).unwrap_or_else(|e| {
        panic!("missing calibration golden ({e}); run with BLESS_GOLDEN=1 to generate")
    });
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

    // camera_matrix is row-major: [fx 0 cx; 0 fy cy; 0 0 1].
    approx::assert_abs_diff_eq!(result.camera_matrix[0], gm[0], epsilon = TOL_FOCAL_PX); // fx
    approx::assert_abs_diff_eq!(result.camera_matrix[4], gm[4], epsilon = TOL_FOCAL_PX); // fy
    approx::assert_abs_diff_eq!(result.camera_matrix[2], gm[2], epsilon = TOL_CENTER_PX); // cx
    approx::assert_abs_diff_eq!(result.camera_matrix[5], gm[5], epsilon = TOL_CENTER_PX); // cy

    for (&got, &want) in result.distortion_coeffs.iter().zip(gd.iter()) {
        approx::assert_abs_diff_eq!(got, want, epsilon = TOL_DISTORTION);
    }

    approx::assert_abs_diff_eq!(
        result.mean_reprojection_distance_pixels,
        g["mean_reprojection_distance_pixels"].as_f64().unwrap(),
        epsilon = TOL_REPROJ_PX
    );

    eprintln!(
        "calibration: fx={:.3} fy={:.3} cx={:.3} cy={:.3} reproj={:.4}px",
        result.camera_matrix[0],
        result.camera_matrix[4],
        result.camera_matrix[2],
        result.camera_matrix[5],
        result.mean_reprojection_distance_pixels,
    );
}
