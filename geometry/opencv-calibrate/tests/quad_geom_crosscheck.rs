//! Cross-check the pure-Rust quad geometry helpers (`contour_area`,
//! `is_contour_convex`) against OpenCV on real, approximated contours.

use std::path::PathBuf;

use checkerboard_calibrate::chessboard;
use image::GenericImageView;

const FRAMES: &[&str] = &["left01.jpg", "left07.jpg", "left14.jpg"];
const EPSILONS: &[f64] = &[1.0, 2.0, 3.0, 5.0];

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
fn area_and_convexity_match_opencv() {
    let mut total = 0usize;
    for &file in FRAMES {
        let (g, w, h) = gray(file);
        let bin = chessboard::adaptive_threshold_mean(&g, w as usize, h as usize, 31, 5.0);
        let contours = chessboard::find_contours(&bin, w as usize, h as usize);

        for contour in &contours {
            for &eps in EPSILONS {
                let approx = chessboard::approx_poly_dp(&contour.points, eps, true);
                if approx.len() < 3 {
                    continue;
                }
                // contourArea is integer-exact, so require exact equality.
                let area_rs = chessboard::contour_area(&approx);
                let area_cv = opencv_calibrate::contour_area(&approx);
                assert_eq!(area_rs, area_cv, "{file}: area mismatch (eps={eps})");

                let convex_rs = chessboard::is_contour_convex(&approx);
                let convex_cv = opencv_calibrate::is_contour_convex(&approx);
                assert_eq!(
                    convex_rs, convex_cv,
                    "{file}: convexity mismatch (eps={eps})"
                );
                total += 1;
            }
        }
    }
    assert!(total > 500, "expected many comparisons, got {total}");
    eprintln!("quad geometry cross-check: {total} polygons matched OpenCV");
}
