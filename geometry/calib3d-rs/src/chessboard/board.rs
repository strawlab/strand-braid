//! Board validation and corner extraction — stage 4 of `findChessboardCorners`.
//!
//! Given the lattice-assigned quad graph from stage 3, this checks that the
//! recovered inner corners form the requested `pattern_w x pattern_h` grid and
//! that the grid is geometrically sane (no folding), then emits the corners in
//! row-major order. The monotonicity test is a port of OpenCV's
//! `checkBoardMonotony`.
//!
//! Note: final *orientation* canonicalization — matching the exact start corner
//! and direction OpenCV emits — is handled when the end-to-end detector is
//! wired against the detection golden; here the corners are returned row-major
//! in lattice order.

use std::collections::HashMap;

use super::link::LinkedQuad;
use super::order::{QuadGrid, inner_corner_lattice};

/// Port of OpenCV `icvCheckBoardMonotony`.
///
/// `corners` are in row-major order with `w` columns and `h` rows. For each row
/// and each column, the intermediate corners must project monotonically (and
/// within `[0, 1]`) onto the segment between that row's/column's endpoints —
/// i.e. the board maps to a non-self-intersecting grid.
pub fn check_board_monotony(corners: &[(f32, f32)], w: usize, h: usize) -> bool {
    if corners.len() != w * h || w == 0 || h == 0 {
        return false;
    }

    for k in 0..2 {
        let max_i = if k == 0 { h } else { w };
        let max_j = (if k == 0 { w } else { h }) - 1;
        for i in 0..max_i {
            let (a, b) = if k == 0 {
                (corners[i * w], corners[i * w + (w - 1)])
            } else {
                (corners[i], corners[(h - 1) * w + i])
            };
            let dx0 = b.0 - a.0;
            let dy0 = b.1 - a.1;
            if dx0.abs() + dy0.abs() < f32::EPSILON {
                return false;
            }
            let denom = dx0 * dx0 + dy0 * dy0;
            let mut prevt = 0.0f32;
            for j in 1..max_j {
                let c = if k == 0 {
                    corners[i * w + j]
                } else {
                    corners[j * w + i]
                };
                let t = ((c.0 - a.0) * dx0 + (c.1 - a.1) * dy0) / denom;
                if t < prevt || t > 1.0 {
                    return false;
                }
                prevt = t;
            }
        }
    }
    true
}

/// Validate and extract a `pattern_w x pattern_h` board from a lattice-assigned
/// connected component.
///
/// Succeeds only when the inner corners form a fully-populated
/// `pattern_w x pattern_h` rectangle in lattice space and pass the monotonicity
/// check. Returns the corners row-major (lattice row then column).
pub fn extract_board(
    quads: &[LinkedQuad],
    coords: &HashMap<usize, QuadGrid>,
    pattern_w: usize,
    pattern_h: usize,
) -> Option<Vec<(f32, f32)>> {
    let pts = inner_corner_lattice(quads, coords);
    if pts.is_empty() {
        return None;
    }

    let min_x = pts.iter().map(|(l, _)| l.0).min().unwrap();
    let max_x = pts.iter().map(|(l, _)| l.0).max().unwrap();
    let min_y = pts.iter().map(|(l, _)| l.1).min().unwrap();
    let max_y = pts.iter().map(|(l, _)| l.1).max().unwrap();
    let width = (max_x - min_x + 1) as usize;
    let height = (max_y - min_y + 1) as usize;

    if width != pattern_w || height != pattern_h || pts.len() != pattern_w * pattern_h {
        return None;
    }

    // Index by lattice position, origin-shifted.
    let map: HashMap<(i32, i32), (f32, f32)> = pts
        .iter()
        .map(|(l, p)| ((l.0 - min_x, l.1 - min_y), *p))
        .collect();

    let mut ordered = Vec::with_capacity(pattern_w * pattern_h);
    for gy in 0..height as i32 {
        for gx in 0..width as i32 {
            // Missing lattice point => not a full rectangle.
            ordered.push(*map.get(&(gx, gy))?);
        }
    }

    if !check_board_monotony(&ordered, pattern_w, pattern_h) {
        return None;
    }
    Some(ordered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chessboard::link::{connected_components, link_quads};
    use crate::chessboard::order::{assign_grid, order_all_corners};
    use crate::chessboard::quad::Quad;

    /// Black squares of a `cells_x` x `cells_y` checkerboard, side `side`.
    fn black_square_board(cells_x: i32, cells_y: i32, side: i32) -> Vec<Quad> {
        let mut quads = Vec::new();
        for cy in 0..cells_y {
            for cx in 0..cells_x {
                if (cx + cy) % 2 != 0 {
                    continue;
                }
                let (x, y) = (cx * side, cy * side);
                quads.push(Quad {
                    corners: [(x, y), (x + side, y), (x + side, y + side), (x, y + side)],
                });
            }
        }
        quads
    }

    fn board_corners(
        cells_x: i32,
        cells_y: i32,
        side: i32,
        pw: usize,
        ph: usize,
    ) -> Option<Vec<(f32, f32)>> {
        let quads = black_square_board(cells_x, cells_y, side);
        let mut linked = link_quads(&quads);
        order_all_corners(&mut linked);
        let comps = connected_components(&linked);
        let grid = assign_grid(&linked, &comps[0]);
        extract_board(&linked, &grid, pw, ph)
    }

    #[test]
    fn monotony_accepts_regular_grid() {
        // 3x2 regular grid.
        let corners = [
            (0.0, 0.0),
            (10.0, 0.0),
            (20.0, 0.0),
            (0.0, 10.0),
            (10.0, 10.0),
            (20.0, 10.0),
        ];
        assert!(check_board_monotony(&corners, 3, 2));
    }

    #[test]
    fn monotony_rejects_folded_grid() {
        // Swap two corners in a row so the projection is non-monotonic.
        let corners = [
            (0.0, 0.0),
            (20.0, 0.0),
            (10.0, 0.0),
            (0.0, 10.0),
            (10.0, 10.0),
            (20.0, 10.0),
        ];
        assert!(!check_board_monotony(&corners, 3, 2));
    }

    #[test]
    fn extracts_non_square_board() {
        // 4x3 cells -> interior lattice x in 1..=3 (3), y in 1..=2 (2) -> 3x2.
        let side = 20;
        let corners = board_corners(4, 3, side, 3, 2).expect("board");
        assert_eq!(corners.len(), 6);
        let mut expected = Vec::new();
        for y in 1..=2 {
            for x in 1..=3 {
                expected.push(((x * side) as f32, (y * side) as f32));
            }
        }
        for (got, want) in corners.iter().zip(expected.iter()) {
            approx::assert_abs_diff_eq!(got.0, want.0, epsilon = 1e-3);
            approx::assert_abs_diff_eq!(got.1, want.1, epsilon = 1e-3);
        }
    }

    #[test]
    fn rejects_wrong_pattern_size() {
        // The board is 3x2; asking for 4x4 must fail.
        assert!(board_corners(4, 3, 20, 4, 4).is_none());
    }
}
