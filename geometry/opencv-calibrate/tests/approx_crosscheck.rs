//! Cross-check the pure-Rust `approx_poly_dp` against OpenCV's `approxPolyDP`,
//! requiring identical output vertices.
//!
//! Both implementations are fed the *same* contours (traced by the pure-Rust
//! Suzuki-Abe tracer on adaptive-thresholded real frames), isolating the
//! polygon-approximation step from any contour-representation differences.

use std::path::PathBuf;

use calib3d_rs::chessboard;
use image::GenericImageView;

const FRAMES: &[&str] = &["left01.jpg", "left05.jpg", "left09.jpg", "left14.jpg"];
const EPSILONS: &[f64] = &[1.0, 2.0, 3.0, 5.0, 7.0];

fn gray(file: &str) -> (Vec<u8>, u32, u32) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(file);
    let img = image::open(&path).unwrap();
    let (w, h) = img.dimensions();
    let rgb = img.to_rgb8().into_raw();
    let gray: Vec<u8> = rgb.iter().step_by(3).copied().collect();
    (gray, w, h)
}

#[test]
fn approx_poly_dp_matches_opencv() {
    let mut total = 0usize;
    for &file in FRAMES {
        let (g, w, h) = gray(file);
        let bin = chessboard::adaptive_threshold_mean(&g, w as usize, h as usize, 31, 5.0);
        let contours = chessboard::find_contours(&bin, w as usize, h as usize);

        for contour in &contours {
            for &eps in EPSILONS {
                let rs = chessboard::approx_poly_dp(&contour.points, eps, true);
                let cv = opencv_calibrate::approx_poly_dp(&contour.points, eps, true);
                assert_eq!(
                    rs,
                    cv,
                    "{file}: approx mismatch (eps={eps}, contour len {})",
                    contour.points.len()
                );
                total += 1;
            }
        }
    }
    assert!(total > 1000, "expected many comparisons, got {total}");
    eprintln!("approx_poly_dp cross-check: {total} contour/eps comparisons matched OpenCV");
}
