// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Quad extraction — the part of OpenCV's `generateQuads` that turns a contour
//! into a candidate chessboard square.
//!
//! For each contour, `approxPolyDP` is run with increasing accuracy until the
//! polygon has four vertices; the result is kept if it is convex and large
//! enough. The geometry helpers ([`contour_area`], [`is_contour_convex`]) match
//! OpenCV's `contourArea` and `isContourConvex` exactly for integer contours.

use super::approx::approx_poly_dp;
use super::contour::Contour;

/// A candidate quadrilateral: four ordered corner pixels `(x, y)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Quad {
    pub corners: [(i32, i32); 4],
}

/// Polygon area via the shoelace formula, matching OpenCV `contourArea`
/// (absolute value, integer-exact for integer input).
pub fn contour_area(pts: &[(i32, i32)]) -> f64 {
    let n = pts.len();
    if n < 3 {
        return 0.0;
    }
    let mut acc = 0i64;
    let mut prev = pts[n - 1];
    for &p in pts {
        acc += prev.0 as i64 * p.1 as i64 - p.0 as i64 * prev.1 as i64;
        prev = p;
    }
    (acc as f64).abs() * 0.5
}

/// Whether a polygon is convex, matching OpenCV `isContourConvex`.
///
/// Like OpenCV, a consecutive collinear triple (cross product exactly zero) is
/// treated as non-convex: the orientation flag is set to both signs at once,
/// which forces a `false` result.
pub fn is_contour_convex(pts: &[(i32, i32)]) -> bool {
    let n = pts.len();
    if n < 3 {
        return false;
    }
    let mut prev = pts[n - 2];
    let mut cur = pts[n - 1];
    let mut dx0 = (cur.0 - prev.0) as i64;
    let mut dy0 = (cur.1 - prev.1) as i64;
    let mut orientation = 0u8;

    for &p in pts {
        prev = cur;
        cur = p;
        let dx = (cur.0 - prev.0) as i64;
        let dy = (cur.1 - prev.1) as i64;
        let cross = dx0 * dy - dx * dy0;
        orientation |= match cross.cmp(&0) {
            std::cmp::Ordering::Greater => 1,
            std::cmp::Ordering::Less => 2,
            std::cmp::Ordering::Equal => 3,
        };
        if orientation == 3 {
            return false;
        }
        dx0 = dx;
        dy0 = dy;
    }
    true
}

/// Extract candidate quads from contours, mirroring OpenCV's per-contour logic
/// in `generateQuads`: approximate with `approxPolyDP` at accuracy levels 1..=7
/// until four vertices remain, then keep convex quads with area `>= min_area`.
pub fn find_quads(contours: &[Contour], min_area: f64) -> Vec<Quad> {
    let mut quads = Vec::new();
    for contour in contours {
        let mut approx = Vec::new();
        for level in 1..=7 {
            approx = approx_poly_dp(&contour.points, level as f64, true);
            if approx.len() == 4 {
                break;
            }
        }
        if approx.len() != 4 {
            continue;
        }
        if !is_contour_convex(&approx) {
            continue;
        }
        if contour_area(&approx) < min_area {
            continue;
        }
        quads.push(Quad {
            corners: [approx[0], approx[1], approx[2], approx[3]],
        });
    }
    quads
}

#[cfg(test)]
mod tests {
    use super::super::contour::find_contours;
    use super::*;

    fn img(rows: &[&str]) -> (Vec<u8>, usize, usize) {
        let h = rows.len();
        let w = rows[0].len();
        let mut data = vec![0u8; w * h];
        for (r, row) in rows.iter().enumerate() {
            for (c, ch) in row.chars().enumerate() {
                data[r * w + c] = if ch == '#' { 255 } else { 0 };
            }
        }
        (data, w, h)
    }

    #[test]
    fn area_of_unit_and_large_squares() {
        // 10x10 axis-aligned square (corners inclusive 0..=10) -> area 100.
        let sq = [(0, 0), (10, 0), (10, 10), (0, 10)];
        approx::assert_abs_diff_eq!(contour_area(&sq), 100.0, epsilon = 1e-9);
        // A triangle with base 4, height 3 -> area 6.
        let tri = [(0, 0), (4, 0), (0, 3)];
        approx::assert_abs_diff_eq!(contour_area(&tri), 6.0, epsilon = 1e-9);
    }

    #[test]
    fn convexity() {
        let square = [(0, 0), (10, 0), (10, 10), (0, 10)];
        assert!(is_contour_convex(&square));
        // An arrowhead / concave quad.
        let concave = [(0, 0), (10, 0), (5, 2), (10, 10)];
        assert!(!is_contour_convex(&concave));
    }

    #[test]
    fn finds_two_square_quads() {
        let (data, w, h) = img(&[
            "..........",
            ".###..###.",
            ".###..###.",
            ".###..###.",
            "..........",
        ]);
        let contours = find_contours(&data, w, h);
        let quads = find_quads(&contours, 1.0);
        assert_eq!(quads.len(), 2, "expected two square quads, got {quads:?}");
        // Each quad's corners should be the 3x3 block corners.
        for q in &quads {
            let set: std::collections::BTreeSet<_> = q.corners.iter().copied().collect();
            assert_eq!(set.len(), 4, "degenerate quad {q:?}");
            assert!(is_contour_convex(&q.corners));
            approx::assert_abs_diff_eq!(contour_area(&q.corners), 4.0, epsilon = 1e-9);
        }
    }
}
