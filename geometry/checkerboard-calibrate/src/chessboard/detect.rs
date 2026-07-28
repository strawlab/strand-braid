// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

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

/// Re-index a detected `pattern_h`-columns by `pattern_w`-rows grid (the shape
/// found when a `pattern_w x pattern_h` board is imaged rotated 90 degrees, so
/// rows and columns come out swapped) into the canonical `pattern_w`-columns
/// by `pattern_h`-rows layout.
///
/// A plain transpose would flip the grid's handedness (determinant -1), which
/// no rigid camera rotation can produce, so this also reverses one axis to
/// keep it a proper rotation (determinant +1) of the board's local coordinate
/// frame. Either of the two 90-degree directions works equally well for
/// calibration: whichever one is picked here, the relabeling is just absorbed
/// into that image's own independently-fit extrinsics, so the true rotation
/// direction of the camera never needs to be known.
fn rotate90_grid(corners: &[(f32, f32)], pattern_w: usize, pattern_h: usize) -> Vec<(f32, f32)> {
    debug_assert_eq!(corners.len(), pattern_w * pattern_h);
    let mut out = vec![(0.0, 0.0); corners.len()];
    for row_out in 0..pattern_h {
        for col_out in 0..pattern_w {
            let src = (pattern_w - 1 - col_out) * pattern_h + row_out;
            out[row_out * pattern_w + col_out] = corners[src];
        }
    }
    out
}

/// Detect a `pattern_w x pattern_h` (inner corners) chessboard in a grayscale
/// image. Returns the inner corners row-major, or `None` if no board is found.
///
/// `pattern_w`/`pattern_h` are the inner-corner counts (e.g. 9x6). If the
/// board is found rotated 90 degrees (so the raw detection comes out as
/// `pattern_h x pattern_w`), the corners are re-indexed back to
/// `pattern_w x pattern_h` via [`rotate90_grid`] so callers always see a
/// consistent row length, regardless of how the camera was rotated when a
/// particular image was captured.
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
                    if pattern_w != pattern_h
                        && let Some(corners) = extract_board(&linked, &grid, pattern_h, pattern_w)
                    {
                        return Some(rotate90_grid(&corners, pattern_w, pattern_h));
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::super::board::check_board_monotony;
    use super::*;

    #[test]
    fn rotate90_grid_preserves_a_valid_monotone_grid() {
        let pattern_w = 9;
        let pattern_h = 6;

        // A grid as it would come back from `extract_board(pattern_h,
        // pattern_w)` when the board was imaged rotated 90 degrees: evenly
        // spaced points, laid out row-major with `pattern_h` columns and
        // `pattern_w` rows.
        let rotated: Vec<(f32, f32)> = (0..pattern_w)
            .flat_map(|r| (0..pattern_h).map(move |c| ((c * 10) as f32, (r * 10) as f32)))
            .collect();

        let fixed = rotate90_grid(&rotated, pattern_w, pattern_h);

        assert_eq!(fixed.len(), pattern_w * pattern_h);
        // Every detected point must survive, just re-indexed.
        let mut got = fixed.clone();
        got.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mut want = rotated.clone();
        want.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(got, want);

        // The re-indexed grid must read out as a valid, non-self-intersecting
        // pattern_w x pattern_h raster (this is exactly what would be wrong
        // if the fix scrambled rows/columns instead of properly rotating).
        assert!(check_board_monotony(&fixed, pattern_w, pattern_h));
    }

    #[test]
    fn rotate90_grid_is_a_bijection_on_indices() {
        let pattern_w = 4;
        let pattern_h = 3;
        let input: Vec<(f32, f32)> = (0..pattern_w * pattern_h)
            .map(|i| (i as f32, (i * 2) as f32))
            .collect();
        let out = rotate90_grid(&input, pattern_w, pattern_h);

        let mut got = out;
        got.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mut want = input;
        want.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(got, want);
    }
}
