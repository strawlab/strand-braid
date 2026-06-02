//! End-to-end chessboard detection — wiring stages 1-4 together.
//!
//! Mirrors the structure of OpenCV's `findChessboardCorners` with the
//! `ADAPTIVE_THRESH | NORMALIZE_IMAGE` flags: equalize the image, then for an
//! increasing number of dilations, binarize, generate quads, link them into a
//! board graph, and try to extract a board of the requested size. The first
//! dilation level that yields a complete, monotone board wins.
//!
//! Corner *order* matches the lattice row-major readout of [`extract_board`];
//! canonicalizing to OpenCV's exact start corner/direction is handled by the
//! caller's cross-check for now.

use super::binarize::{adaptive_threshold_mean, equalize_hist};
use super::board::extract_board;
use super::contour::find_contours;
use super::link::{connected_components, link_quads};
use super::order::{assign_grid, order_all_corners};
use super::quad::{Quad, contour_area, find_quads};

/// Maximum number of dilation iterations to try (matches OpenCV's range).
const MAX_DILATIONS: usize = 7;

/// 3x3 dilation (max filter) of a binary image, out-of-bounds treated as 0,
/// matching OpenCV's default `dilate` with a 3x3 rectangular kernel.
fn dilate3x3(src: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut dst = vec![0u8; src.len()];
    for y in 0..h {
        for x in 0..w {
            let mut m = 0u8;
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    let nx = x as i32 + dx;
                    let ny = y as i32 + dy;
                    if nx >= 0 && ny >= 0 && (nx as usize) < w && (ny as usize) < h {
                        m = m.max(src[ny as usize * w + nx as usize]);
                    }
                }
            }
            dst[y * w + x] = m;
        }
    }
    dst
}

/// Paint a `thickness`-pixel border of `value` around the image, as OpenCV does
/// to close squares that touch the image edge.
fn draw_border(img: &mut [u8], w: usize, h: usize, value: u8, thickness: usize) {
    for y in 0..h {
        for x in 0..w {
            if x < thickness || y < thickness || x + thickness >= w || y + thickness >= h {
                img[y * w + x] = value;
            }
        }
    }
}

/// Detect a `pattern_w x pattern_h` (inner corners) chessboard in a grayscale
/// image. Returns the inner corners row-major, or `None` if no board is found.
///
/// `pattern_w`/`pattern_h` are the inner-corner counts (e.g. 9x6).
pub fn find_chessboard_corners(
    gray: &[u8],
    w: usize,
    h: usize,
    pattern_w: usize,
    pattern_h: usize,
) -> Option<Vec<(f32, f32)>> {
    assert_eq!(gray.len(), w * h);
    let eq = equalize_hist(gray);

    // Adaptive-threshold block sizes scaled to the image (odd). Several scales
    // are tried because the right neighborhood depends on the square size and
    // perspective, as in OpenCV's multi-attempt loop.
    let smaller = w.min(h);
    let block_sizes: Vec<usize> = [smaller / 5, smaller / 9, smaller / 15]
        .iter()
        .map(|b| (b | 1).max(3))
        .collect();
    // Reject tiny noise quads and the whole-image background quad.
    let min_area = 25.0;
    let max_area = (w as f64) * (h as f64) * 0.5;

    for dilations in 0..=MAX_DILATIONS {
        for &block_size in &block_sizes {
            for &delta in &[0.0f64, 5.0, 9.0] {
                let mut bin = adaptive_threshold_mean(&eq, w, h, block_size, delta);
                draw_border(&mut bin, w, h, 255, 1);
                for _ in 0..dilations {
                    bin = dilate3x3(&bin, w, h);
                }

                let contours = find_contours(&bin, w, h);
                let all_quads = find_quads(&contours, min_area);
                let quads: Vec<Quad> = all_quads
                    .into_iter()
                    .filter(|q| {
                        let corners = [q.corners[0], q.corners[1], q.corners[2], q.corners[3]];
                        contour_area(&corners) <= max_area
                    })
                    .collect();
                if quads.len() < pattern_w * pattern_h / 4 {
                    continue;
                }

                let mut linked = link_quads(&quads);
                order_all_corners(&mut linked);
                for comp in connected_components(&linked) {
                    let grid = assign_grid(&linked, &comp);
                    if let Some(corners) = extract_board(&linked, &grid, pattern_w, pattern_h) {
                        return Some(corners);
                    }
                    if let Some(corners) = extract_board(&linked, &grid, pattern_h, pattern_w) {
                        return Some(corners);
                    }
                }
            }
        }
    }
    None
}
