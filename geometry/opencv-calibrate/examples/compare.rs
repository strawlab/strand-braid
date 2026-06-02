//! Side-by-side comparison of the OpenCV C++ and pure-Rust (`checkerboard-calibrate`)
//! chessboard-calibration pipelines, end to end.
//!
//! For each image it runs chessboard detection both ways (OpenCV
//! `findChessboardCorners` vs the pure-Rust detector + pure-Rust
//! `cornerSubPix`) and reports the corner counts and the order-independent
//! position difference. It then runs camera calibration with both the OpenCV
//! and pure-Rust solvers on the same detected corners and prints the recovered
//! intrinsics side by side.
//!
//! Run over the bundled sample frames:
//!
//! ```sh
//! cargo run -p opencv-calibrate --example compare
//! ```
//!
//! Or on a specific image (with inner-corner counts):
//!
//! ```sh
//! cargo run -p opencv-calibrate --example compare -- path/to/board.png 9 6
//! ```

use std::path::PathBuf;

use checkerboard_calibrate::chessboard;
use checkerboard_calibrate::{CornerSubPixParams, GrayImageRef, corner_subpix};
use image::GenericImageView;

/// Default sample frames (OpenCV `left*.jpg`), all 9x6 inner corners.
const SAMPLE_FRAMES: &[&str] = &[
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

struct Detection {
    width: u32,
    height: u32,
    opencv: Option<Vec<(f32, f32)>>,
    rust: Option<Vec<(f32, f32)>>,
}

fn load_rgb_and_gray(path: &PathBuf) -> (Vec<u8>, Vec<u8>, u32, u32) {
    let img = image::open(path).unwrap_or_else(|e| panic!("open {}: {e}", path.display()));
    let (w, h) = img.dimensions();
    let rgb = img.to_rgb8().into_raw();
    let gray: Vec<u8> = rgb.iter().step_by(3).copied().collect();
    (rgb, gray, w, h)
}

fn detect(path: &PathBuf, cols: usize, rows: usize) -> Detection {
    let (rgb, gray, w, h) = load_rgb_and_gray(path);

    let opencv = opencv_calibrate::find_chessboard_corners(&rgb, w, h, cols, rows)
        .expect("opencv detection error");

    let rust =
        chessboard::find_chessboard_corners(&gray, w as usize, h as usize, cols, rows).map(|raw| {
            corner_subpix(
                GrayImageRef::new(&gray, w as usize, h as usize),
                &raw,
                &CornerSubPixParams::default(),
            )
        });

    Detection {
        width: w,
        height: h,
        opencv,
        rust,
    }
}

/// Order-independent max distance: for each `a` corner, the nearest `b` corner.
fn set_distance(a: &[(f32, f32)], b: &[(f32, f32)]) -> f64 {
    a.iter()
        .map(|p| {
            b.iter()
                .map(|q| ((p.0 - q.0) as f64).hypot((p.1 - q.1) as f64))
                .fold(f64::MAX, f64::min)
        })
        .fold(0.0f64, f64::max)
}

/// Object grid for a planar board (unit spacing), matching the row-major corner
/// order both detectors use within a view.
fn object_grid(cols: usize, rows: usize) -> Vec<(f64, f64, f64)> {
    (0..cols * rows)
        .map(|i| ((i % cols) as f64, (i / cols) as f64, 0.0))
        .collect()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let data_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data");

    // Either a single user-supplied image, or the bundled samples.
    let (jobs, cols, rows): (Vec<PathBuf>, usize, usize) = if args.len() >= 4 {
        let cols = args[2].parse().expect("cols");
        let rows = args[3].parse().expect("rows");
        (vec![PathBuf::from(&args[1])], cols, rows)
    } else {
        (
            SAMPLE_FRAMES.iter().map(|f| data_dir.join(f)).collect(),
            9,
            6,
        )
    };

    println!("== chessboard detection: OpenCV C++ vs pure-Rust ==");
    println!("(set-dist = order-independent max corner distance, pixels)\n");
    println!(
        "{:<14} {:>10} {:>10} {:>12}",
        "image", "opencv", "rust", "set-dist"
    );

    let mut detections = Vec::new();
    for path in &jobs {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let det = detect(path, cols, rows);
        let oc = det
            .opencv
            .as_ref()
            .map_or("none".to_string(), |c| format!("{}", c.len()));
        let rc = det
            .rust
            .as_ref()
            .map_or("none".to_string(), |c| format!("{}", c.len()));
        let dist = match (&det.opencv, &det.rust) {
            (Some(o), Some(r)) if o.len() == r.len() => format!("{:.4}", set_distance(o, r)),
            (Some(_), Some(_)) => "len!=".to_string(),
            _ => "-".to_string(),
        };
        println!("{name:<14} {oc:>10} {rc:>10} {dist:>12}");
        detections.push((det, cols, rows));
    }

    // Calibration comparison on the OpenCV-detected corners (consistent order),
    // run through both calibrators.
    let mut cv_views: Vec<Vec<opencv_calibrate::CorrespondingPoint>> = Vec::new();
    let mut rs_views: Vec<Vec<checkerboard_calibrate::calibrate::CorrespondingPoint>> = Vec::new();
    let (mut w, mut h) = (0u32, 0u32);
    for (det, c, r) in &detections {
        let Some(corners) = &det.opencv else { continue };
        if corners.len() != c * r {
            continue;
        }
        w = det.width;
        h = det.height;
        let obj = object_grid(*c, *r);
        cv_views.push(
            corners
                .iter()
                .zip(&obj)
                .map(|(&(x, y), &o)| opencv_calibrate::CorrespondingPoint {
                    object_point: o,
                    image_point: (x as f64, y as f64),
                })
                .collect(),
        );
        rs_views.push(
            corners
                .iter()
                .zip(&obj)
                .map(|(&(x, y), &o)| checkerboard_calibrate::calibrate::CorrespondingPoint {
                    object_point: o,
                    image_point: (x as f64, y as f64),
                })
                .collect(),
        );
    }

    if cv_views.len() < 3 {
        println!("\n(not enough detected boards for a calibration comparison)");
        return;
    }

    let cv = opencv_calibrate::calibrate_camera(&cv_views, w as i32, h as i32)
        .expect("opencv calibrate");
    let rs = checkerboard_calibrate::calibrate::calibrate_camera(&rs_views, w, h).expect("rust calibrate");

    println!(
        "\n== camera calibration on {} shared views ==",
        cv_views.len()
    );
    println!(
        "{:<10} {:>16} {:>16} {:>12}",
        "param", "opencv", "rust", "diff"
    );
    let row = |name: &str, a: f64, b: f64| {
        println!("{name:<10} {a:>16.6} {b:>16.6} {:>12.2e}", (a - b).abs());
    };
    row("fx", cv.camera_matrix[0], rs.camera_matrix[0]);
    row("fy", cv.camera_matrix[4], rs.camera_matrix[4]);
    row("cx", cv.camera_matrix[2], rs.camera_matrix[2]);
    row("cy", cv.camera_matrix[5], rs.camera_matrix[5]);
    row("k1", cv.distortion_coeffs[0], rs.distortion_coeffs[0]);
    row("k2", cv.distortion_coeffs[1], rs.distortion_coeffs[1]);
    row("p1", cv.distortion_coeffs[2], rs.distortion_coeffs[2]);
    row("p2", cv.distortion_coeffs[3], rs.distortion_coeffs[3]);
    row(
        "rms",
        cv.mean_reprojection_distance_pixels,
        rs.rms_reprojection_error,
    );
}
