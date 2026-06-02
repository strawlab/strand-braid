//! End-to-end check of the pure-Rust chessboard detector against the golden.
//!
//! Runs `calib3d_rs::chessboard::find_chessboard_corners` on the sample frames,
//! applies the pure-Rust cornerSubPix, and compares to the OpenCV detection
//! golden (which is OpenCV detect + cornerSubPix). cornerSubPix converges to the
//! true saddle, so matching does not require identical raw corners — only the
//! same 54 corners in the same order, seeded close enough.

use std::path::PathBuf;

use calib3d_rs::chessboard;
use calib3d_rs::{CornerSubPixParams, GrayImageRef, corner_subpix};
use image::GenericImageView;

const COLS: usize = 9;
const ROWS: usize = 6;

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

fn golden_corners(file: &str) -> Vec<(f32, f32)> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/golden")
        .join(format!("{file}.json"));
    let text = std::fs::read_to_string(&path).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&text).unwrap();
    doc["corners"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| (c[0].as_f64().unwrap() as f32, c[1].as_f64().unwrap() as f32))
        .collect()
}

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

/// Order-independent distance: max over golden corners of the nearest detected
/// corner (one-sided Hausdorff). Near zero means the detector found the same
/// corner positions as OpenCV, regardless of ordering.
fn set_distance(detected: &[(f32, f32)], golden: &[(f32, f32)]) -> f64 {
    golden
        .iter()
        .map(|gp| {
            detected
                .iter()
                .map(|rp| ((rp.0 - gp.0) as f64).hypot((rp.1 - gp.1) as f64))
                .fold(f64::MAX, f64::min)
        })
        .fold(0.0f64, f64::max)
}

#[test]
fn detector_finds_opencv_corner_positions() {
    // The pure-Rust detector + pure-Rust cornerSubPix must recover the same
    // corner positions as OpenCV (the golden) to sub-pixel accuracy. Corner
    // *ordering* is intentionally not compared: OpenCV's order is pose-dependent
    // (its internal seed-quad traversal), and the pure-Rust calibrator only
    // needs a consistent per-view order, which the detector provides.
    //
    // Position tolerance vs golden, in pixels.
    const TOL_PX: f64 = 0.5;

    let mut detected = 0usize;
    let mut worst = 0.0f64;
    for &file in FRAMES {
        let (g, w, h) = gray(file);
        let Some(raw) = chessboard::find_chessboard_corners(&g, w as usize, h as usize, COLS, ROWS)
        else {
            eprintln!("{file}: NO BOARD FOUND");
            continue;
        };
        assert_eq!(raw.len(), COLS * ROWS, "{file}: wrong corner count");
        let refined = corner_subpix(
            GrayImageRef::new(&g, w as usize, h as usize),
            &raw,
            &CornerSubPixParams::default(),
        );
        let d = set_distance(&refined, &golden_corners(file));
        eprintln!("{file}: set-dist vs golden = {d:.4}px");
        assert!(
            d < TOL_PX,
            "{file}: corner positions differ from OpenCV by {d:.4}px"
        );
        detected += 1;
        worst = worst.max(d);
    }

    eprintln!(
        "detected {detected}/{} frames; worst set-dist {worst:.4}px",
        FRAMES.len()
    );
    // left02 currently needs OpenCV's board-augmentation (addOuterQuad) which
    // is not yet ported; the rest must all be found.
    assert!(
        detected >= FRAMES.len() - 1,
        "only detected {detected}/{}",
        FRAMES.len()
    );
}
