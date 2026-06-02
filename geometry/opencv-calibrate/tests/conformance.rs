//! Conformance harness for chessboard corner detection.
//!
//! This pins the sub-pixel corner coordinates that the current (OpenCV-backed)
//! [`opencv_calibrate::find_chessboard_corners`] implementation produces for a
//! set of committed sample images. The golden coordinates are stored under
//! `tests/data/golden/` so the suite runs without any network access.
//!
//! The purpose is twofold:
//!   1. Regression-guard the current OpenCV implementation.
//!   2. Provide a ground-truth target for a future pure-Rust reimplementation,
//!      which must reproduce the same corners (same ordering) within [`TOL_PX`].
//!
//! To (re)generate the golden files after an intentional change, run:
//!
//! ```text
//! BLESS_GOLDEN=1 cargo test -p opencv-calibrate --test conformance
//! ```

use std::path::{Path, PathBuf};

use image::GenericImageView;

/// Per-corner tolerance, in pixels, when comparing against golden coordinates.
///
/// The current OpenCV build is deterministic to far below this, but a future
/// pure-Rust port is only expected to match OpenCV to roughly this precision,
/// so the harness is meaningful for both uses.
const TOL_PX: f32 = 0.5;

/// A positive fixture: an image that contains a fully detectable board.
struct Fixture {
    file: &'static str,
    /// Inner-corner counts: (columns, rows).
    pattern: (usize, usize),
}

/// All sample frames are from the OpenCV `samples/data/left*.jpg` series
/// (9x6 inner corners). See `tests/data/README.md` for provenance.
const FIXTURES: &[Fixture] = &[
    Fixture { file: "left01.jpg", pattern: (9, 6) },
    Fixture { file: "left02.jpg", pattern: (9, 6) },
    Fixture { file: "left03.jpg", pattern: (9, 6) },
    Fixture { file: "left04.jpg", pattern: (9, 6) },
    Fixture { file: "left05.jpg", pattern: (9, 6) },
    Fixture { file: "left06.jpg", pattern: (9, 6) },
    Fixture { file: "left07.jpg", pattern: (9, 6) },
    Fixture { file: "left08.jpg", pattern: (9, 6) },
    Fixture { file: "left09.jpg", pattern: (9, 6) },
    Fixture { file: "left11.jpg", pattern: (9, 6) },
    Fixture { file: "left12.jpg", pattern: (9, 6) },
    Fixture { file: "left13.jpg", pattern: (9, 6) },
    Fixture { file: "left14.jpg", pattern: (9, 6) },
];

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}

fn golden_path(file: &str) -> PathBuf {
    data_dir().join("golden").join(format!("{file}.json"))
}

fn blessing() -> bool {
    std::env::var_os("BLESS_GOLDEN").is_some()
}

/// Run detection on a fixture image, returning the detected corners.
fn detect(fx: &Fixture) -> Vec<(f32, f32)> {
    let path = data_dir().join(fx.file);
    let img = image::open(&path).unwrap_or_else(|e| panic!("open {}: {e}", path.display()));
    let (w, h) = img.dimensions();
    let rgb = img.to_rgb8().into_raw();
    let (cols, rows) = fx.pattern;
    opencv_calibrate::find_chessboard_corners(&rgb, w, h, cols, rows)
        .unwrap_or_else(|e| panic!("detection error on {}: {e}", fx.file))
        .unwrap_or_else(|| panic!("no board found in {} (expected one)", fx.file))
}

fn write_golden(fx: &Fixture, corners: &[(f32, f32)]) {
    let arr: Vec<[f32; 2]> = corners.iter().map(|&(x, y)| [x, y]).collect();
    let doc = serde_json::json!({
        "image": fx.file,
        "pattern_cols": fx.pattern.0,
        "pattern_rows": fx.pattern.1,
        "corners": arr,
    });
    let path = golden_path(fx.file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let text = serde_json::to_string_pretty(&doc).unwrap();
    std::fs::write(&path, text + "\n").unwrap();
    eprintln!("blessed golden: {}", path.display());
}

fn read_golden(fx: &Fixture) -> Vec<(f32, f32)> {
    let path = golden_path(fx.file);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "missing golden {} ({e}); run with BLESS_GOLDEN=1 to generate",
            path.display()
        )
    });
    let doc: serde_json::Value = serde_json::from_str(&text).unwrap();
    doc["corners"]
        .as_array()
        .expect("corners array")
        .iter()
        .map(|c| {
            let x = c[0].as_f64().expect("corner x") as f32;
            let y = c[1].as_f64().expect("corner y") as f32;
            (x, y)
        })
        .collect()
}

#[test]
fn chessboard_corners_match_golden() {
    let mut failures = Vec::new();

    for fx in FIXTURES {
        let detected = detect(fx);

        if blessing() {
            write_golden(fx, &detected);
            continue;
        }

        let expected = read_golden(fx);
        if detected.len() != expected.len() {
            failures.push(format!(
                "{}: corner count {} != golden {}",
                fx.file,
                detected.len(),
                expected.len()
            ));
            continue;
        }

        let mut worst = 0.0f32;
        for (i, (&(dx, dy), &(ex, ey))) in detected.iter().zip(expected.iter()).enumerate() {
            let d = ((dx - ex).powi(2) + (dy - ey).powi(2)).sqrt();
            if d > worst {
                worst = d;
            }
            if d > TOL_PX {
                failures.push(format!(
                    "{}: corner {i} drift {d:.3}px > {TOL_PX}px (got [{dx:.3},{dy:.3}], golden [{ex:.3},{ey:.3}])",
                    fx.file
                ));
            }
        }
        eprintln!("{}: {} corners, worst drift {worst:.4}px", fx.file, detected.len());
    }

    if blessing() {
        eprintln!("BLESS_GOLDEN set: regenerated goldens, skipping comparison");
        return;
    }

    assert!(failures.is_empty(), "conformance failures:\n{}", failures.join("\n"));
}

/// A blank image must yield no detection.
#[test]
fn blank_image_finds_no_board() {
    let path = data_dir().join("blank.png");
    let img = image::open(&path).unwrap();
    let (w, h) = img.dimensions();
    let rgb = img.to_rgb8().into_raw();
    let corners = opencv_calibrate::find_chessboard_corners(&rgb, w, h, 9, 6).unwrap();
    assert!(corners.is_none(), "expected no board in blank image");
}
