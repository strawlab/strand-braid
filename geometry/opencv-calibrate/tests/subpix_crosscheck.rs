//! Cross-check the pure-Rust `checkerboard_calibrate::corner_subpix` against OpenCV.
//!
//! For each sample image we take OpenCV's *pre-refinement* corners (via
//! [`opencv_calibrate::find_chessboard_corners_no_refine`]), run the pure-Rust
//! refinement on them, and compare against the OpenCV-refined golden coordinates
//! committed under `tests/data/golden/`. This isolates `cornerSubPix`: both
//! implementations start from identical corners on the identical image.
//!
//! The grayscale image fed to refinement is channel 0 of the decoded RGB. For
//! these grayscale JPEGs that equals OpenCV's `cvtColor(BGR2GRAY)` output
//! exactly (R==G==B, and the fixed-point luma weights sum to unity), so the
//! comparison is faithful.

use std::path::{Path, PathBuf};

use checkerboard_calibrate::{CornerSubPixParams, GrayImageRef, corner_subpix};
use image::GenericImageView;

/// Tolerance vs OpenCV. The two implementations follow the same algorithm, so
/// agreement is expected to be tight; the bound is the contract for the port.
const TOL_PX: f64 = 0.05;

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

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}

fn golden_corners(file: &str) -> Vec<(f32, f32)> {
    let path = data_dir().join("golden").join(format!("{file}.json"));
    let text = std::fs::read_to_string(&path).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&text).unwrap();
    doc["corners"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| (c[0].as_f64().unwrap() as f32, c[1].as_f64().unwrap() as f32))
        .collect()
}

#[test]
fn pure_subpix_matches_opencv() {
    let mut failures = Vec::new();
    let mut global_worst = 0.0f64;

    for &file in FRAMES {
        let path = data_dir().join(file);
        let img = image::open(&path).unwrap();
        let (w, h) = img.dimensions();
        let rgb = img.to_rgb8().into_raw();

        // Channel 0 == OpenCV gray for these grayscale images.
        let gray: Vec<u8> = rgb.iter().step_by(3).copied().collect();
        let gray_img = GrayImageRef::new(&gray, w as usize, h as usize);

        // Identical starting corners for both implementations.
        let raw = opencv_calibrate::find_chessboard_corners_no_refine(&rgb, w, h, COLS, ROWS)
            .unwrap()
            .unwrap_or_else(|| panic!("no board found in {file}"));

        let refined = corner_subpix(gray_img, &raw, &CornerSubPixParams::default());
        let expected = golden_corners(file);
        assert_eq!(
            refined.len(),
            expected.len(),
            "{file}: corner count mismatch"
        );

        let mut worst = 0.0f64;
        for (i, (&(rx, ry), &(ex, ey))) in refined.iter().zip(expected.iter()).enumerate() {
            let d = ((rx - ex) as f64).hypot((ry - ey) as f64);
            worst = worst.max(d);
            if d > TOL_PX {
                failures.push(format!(
                    "{file}: corner {i} drift {d:.4}px > {TOL_PX}px (pure [{rx:.4},{ry:.4}], opencv [{ex:.4},{ey:.4}])"
                ));
            }
        }
        global_worst = global_worst.max(worst);
        eprintln!(
            "{file}: {} corners, worst drift {worst:.4}px",
            refined.len()
        );
    }

    eprintln!("global worst drift across all frames: {global_worst:.4}px");
    assert!(
        failures.is_empty(),
        "subpix cross-check failures:\n{}",
        failures.join("\n")
    );
}
