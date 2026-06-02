//! Cross-check the pure-Rust chessboard binarization primitives against OpenCV
//! on the real sample images, requiring bit-exact agreement.

use std::path::PathBuf;

use checkerboard_calibrate::chessboard;
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

fn count_diffs(a: &[u8], b: &[u8]) -> usize {
    a.iter().zip(b.iter()).filter(|(x, y)| x != y).count()
}

#[test]
fn equalize_hist_matches_opencv() {
    for &file in FRAMES {
        let (g, w, h) = gray(file);
        let cv = opencv_calibrate::equalize_hist(&g, w, h);
        let rs = chessboard::equalize_hist(&g);
        assert_eq!(
            count_diffs(&cv, &rs),
            0,
            "{file}: equalize_hist differs from OpenCV"
        );
    }
}

#[test]
fn adaptive_threshold_matches_opencv() {
    // Block sizes and deltas spanning the range findChessboardCorners uses.
    let block_sizes = [3usize, 11, 31, 75, 151];
    let deltas = [0.0f64, 5.0, 10.0];

    for &file in FRAMES {
        let (g, w, h) = gray(file);
        for &bs in &block_sizes {
            for &c in &deltas {
                let cv = opencv_calibrate::adaptive_threshold_mean(&g, w, h, bs as i32, c);
                let rs = chessboard::adaptive_threshold_mean(&g, w as usize, h as usize, bs, c);
                let diffs = count_diffs(&cv, &rs);
                assert_eq!(
                    diffs, 0,
                    "{file}: adaptive_threshold(block={bs}, c={c}) differs from OpenCV in {diffs} px"
                );
            }
        }
    }
}
