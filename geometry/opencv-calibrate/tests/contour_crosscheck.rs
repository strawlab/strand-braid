//! Cross-check the pure-Rust Suzuki-Abe tracer against OpenCV `findContours`.
//!
//! Both implement Suzuki-Abe, so the *set* of border pixels must be identical.
//! We compare the union of all contour pixels (order- and segmentation-
//! independent) on binary images derived from the real sample frames, requiring
//! zero differing pixels.

use std::path::PathBuf;

use calib3d_rs::chessboard;
use image::GenericImageView;

const FRAMES: &[&str] = &[
    "left01.jpg",
    "left03.jpg",
    "left07.jpg",
    "left11.jpg",
    "left14.jpg",
];

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

/// Build the mask of border pixels found by the pure-Rust tracer.
fn rust_mask(bin: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut mask = vec![0u8; bin.len()];
    for c in chessboard::find_contours(bin, w, h) {
        for (x, y) in c.points {
            mask[y as usize * w + x as usize] = 255;
        }
    }
    mask
}

#[test]
fn contour_pixels_match_opencv() {
    for &file in FRAMES {
        let (g, w, h) = gray(file);
        // Threshold to a realistic binary image (same primitive used by the
        // detector), then trace contours both ways.
        let bin = chessboard::adaptive_threshold_mean(&g, w as usize, h as usize, 31, 5.0);

        let cv = opencv_calibrate::contours_mask(&bin, w, h);
        let rs = rust_mask(&bin, w as usize, h as usize);

        let diffs = cv.iter().zip(rs.iter()).filter(|(a, b)| a != b).count();
        assert_eq!(
            diffs, 0,
            "{file}: contour border pixels differ from OpenCV in {diffs} px"
        );
    }
}
