// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Build collision masks from polygons, for use with the Parry geometry
//! library.
//!
//! The mask is a [`Mask`] (a Parry `Compound`) covering the polygon's interior,
//! so a point is inside the polygon exactly when its solid distance to the mask
//! is zero.
//!
//! # Concave polygons
//!
//! [`mask_from_points`] honors concave outlines: the points are read as a ring,
//! in the order given, and the mask covers what that ring encloses.
//!
//! A ring that crosses itself has no well-defined interior. Rather than
//! guessing, such a polygon falls back to the convex hull of its vertices,
//! which is what every polygon used to produce.

pub type Mask = parry2d_f64::shape::Compound;

use parry2d_f64::math::Vector;

/// Why a set of points does not describe a polygon.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MaskError {
    /// Fewer than three distinct vertices were given.
    #[error("a polygon needs at least 3 distinct points, got {0}")]
    TooFewPoints(usize),
    /// Three or more vertices, but they enclose no area (e.g. all collinear).
    #[error("the polygon points enclose no area")]
    NoArea,
}

/// Build a mask covering the interior of the polygon through `viewport_points`.
///
/// The points are the polygon's vertices in order, either winding. The first
/// and last need not be equal; the ring is closed implicitly.
///
/// Concave outlines are honored. A self-crossing ring has no well-defined
/// interior and falls back to the convex hull of its vertices.
///
/// # Errors
///
/// Returns [`MaskError`] if the points do not enclose any area, rather than
/// building a degenerate mask.
pub fn mask_from_points(viewport_points: &[(f64, f64)]) -> Result<Mask, MaskError> {
    let ring = clean_ring(viewport_points);
    if ring.len() < 3 {
        return Err(MaskError::TooFewPoints(ring.len()));
    }

    // A ring that crosses itself does not enclose a well-defined region, so
    // keep the historical convex-hull reading for it instead of inventing one.
    let triangles = if is_simple(&ring) {
        triangulate_simple(&ring)
    } else {
        triangulate_convex_hull(&ring)
    };

    build_mask(&triangles)
}

/// Drop repeated vertices, including a closing vertex equal to the first.
///
/// Duplicates are what an interactive editor produces from a double click, and
/// they upset the ear-clipping below without changing the region described.
fn clean_ring(points: &[(f64, f64)]) -> Vec<Vector> {
    let mut ring: Vec<Vector> = Vec::with_capacity(points.len());
    for &(x, y) in points {
        let p = Vector::new(x, y);
        if ring.last().map(|last| *last == p).unwrap_or(false) {
            continue;
        }
        ring.push(p);
    }
    while ring.len() > 1 && ring[0] == ring[ring.len() - 1] {
        ring.pop();
    }
    ring
}

/// Twice the signed area of the ring. Positive when counter-clockwise (in a
/// y-down image, clockwise on screen; the distinction does not matter here,
/// only that the sign tells the two windings apart).
fn signed_area2(ring: &[Vector]) -> f64 {
    let mut acc = 0.0;
    for i in 0..ring.len() {
        let a = ring[i];
        let b = ring[(i + 1) % ring.len()];
        acc += a.x * b.y - b.x * a.y;
    }
    acc
}

/// Cross product of `b - a` and `c - a`; positive when `a`, `b`, `c` turn left.
fn cross(a: Vector, b: Vector, c: Vector) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

/// Whether the closed segments `p0..p1` and `q0..q1` share any point.
fn segments_intersect(p0: Vector, p1: Vector, q0: Vector, q1: Vector) -> bool {
    let d1 = cross(q0, q1, p0);
    let d2 = cross(q0, q1, p1);
    let d3 = cross(p0, p1, q0);
    let d4 = cross(p0, p1, q1);

    if ((d1 > 0.0) != (d2 > 0.0)) && ((d3 > 0.0) != (d4 > 0.0)) {
        // Both straddle: they cross, unless an endpoint lies exactly on the
        // other segment, which the collinear cases below decide.
        if d1 != 0.0 && d2 != 0.0 && d3 != 0.0 && d4 != 0.0 {
            return true;
        }
    }

    // Touching or collinear-overlapping cases.
    let on = |a: Vector, b: Vector, p: Vector| {
        cross(a, b, p) == 0.0
            && p.x >= a.x.min(b.x)
            && p.x <= a.x.max(b.x)
            && p.y >= a.y.min(b.y)
            && p.y <= a.y.max(b.y)
    };
    on(q0, q1, p0) || on(q0, q1, p1) || on(p0, p1, q0) || on(p0, p1, q1)
}

/// Whether the ring is a simple polygon: no two non-adjacent edges meet.
///
/// O(n^2) in the vertex count, which is fine — masks are built when a
/// configuration changes, not per frame, and these polygons are hand-written.
fn is_simple(ring: &[Vector]) -> bool {
    let n = ring.len();
    for i in 0..n {
        let (p0, p1) = (ring[i], ring[(i + 1) % n]);
        for j in (i + 1)..n {
            // Skip edges sharing a vertex; they always "intersect" there.
            if j == i || (j + 1) % n == i || (i + 1) % n == j {
                continue;
            }
            let (q0, q1) = (ring[j], ring[(j + 1) % n]);
            if segments_intersect(p0, p1, q0, q1) {
                return false;
            }
        }
    }
    true
}

/// Whether `p` lies inside or on triangle `a`, `b`, `c` (assumed CCW).
fn point_in_triangle(a: Vector, b: Vector, c: Vector, p: Vector) -> bool {
    cross(a, b, p) >= 0.0 && cross(b, c, p) >= 0.0 && cross(c, a, p) >= 0.0
}

/// Ear-clip a simple polygon into triangles covering exactly its interior.
fn triangulate_simple(ring: &[Vector]) -> Vec<[Vector; 3]> {
    // Work counter-clockwise so "convex vertex" is a left turn.
    let mut idx: Vec<usize> = (0..ring.len()).collect();
    if signed_area2(ring) < 0.0 {
        idx.reverse();
    }

    let mut out = Vec::with_capacity(idx.len().saturating_sub(2));
    // Each successful clip removes one vertex; the guard bounds the work if no
    // ear can be found (which should not happen for a simple polygon, but
    // floating-point coordinates do not owe us that).
    let mut guard = idx.len() * idx.len() + 8;
    while idx.len() > 3 && guard > 0 {
        guard -= 1;
        let n = idx.len();
        let mut clipped = false;
        for k in 0..n {
            let (ia, ib, ic) = (idx[(k + n - 1) % n], idx[k], idx[(k + 1) % n]);
            let (a, b, c) = (ring[ia], ring[ib], ring[ic]);
            if cross(a, b, c) <= 0.0 {
                // Reflex (or straight) vertex: not an ear.
                continue;
            }
            // An ear's triangle must be empty of the other vertices.
            let contains_other = idx
                .iter()
                .any(|&i| i != ia && i != ib && i != ic && point_in_triangle(a, b, c, ring[i]));
            if contains_other {
                continue;
            }
            out.push([a, b, c]);
            idx.remove(k);
            clipped = true;
            break;
        }
        if !clipped {
            break;
        }
    }
    if idx.len() == 3 {
        out.push([ring[idx[0]], ring[idx[1]], ring[idx[2]]]);
    }
    out
}

/// Fan-triangulate the convex hull of the points.
///
/// This is the reading every polygon got before concave outlines were
/// supported, kept for rings that cross themselves.
fn triangulate_convex_hull(ring: &[Vector]) -> Vec<[Vector; 3]> {
    let hull = convex_hull(ring);
    if hull.len() < 3 {
        return Vec::new();
    }
    (1..hull.len() - 1)
        .map(|i| [hull[0], hull[i], hull[i + 1]])
        .collect()
}

/// Andrew's monotone chain hull, counter-clockwise.
fn convex_hull(points: &[Vector]) -> Vec<Vector> {
    let mut pts = points.to_vec();
    pts.sort_by(|a, b| {
        a.x.partial_cmp(&b.x)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal))
    });
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }

    let build = |iter: &mut dyn Iterator<Item = Vector>| -> Vec<Vector> {
        let mut chain: Vec<Vector> = Vec::new();
        for p in iter {
            while chain.len() >= 2
                && cross(chain[chain.len() - 2], chain[chain.len() - 1], p) <= 0.0
            {
                chain.pop();
            }
            chain.push(p);
        }
        chain
    };

    let mut lower = build(&mut pts.iter().copied());
    let mut upper = build(&mut pts.iter().rev().copied());
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// Turn triangles into a Parry compound, skipping any with no area.
fn build_mask(triangles: &[[Vector; 3]]) -> Result<Mask, MaskError> {
    use parry2d_f64::shape::{Compound, ConvexPolygon, SharedShape};

    let delta = parry2d_f64::math::Pose::IDENTITY;
    let shapes: Vec<_> = triangles
        .iter()
        .filter_map(|tri| {
            // `from_convex_hull` returns None for a degenerate (zero-area)
            // triangle. Those contribute nothing to the mask, and passing one
            // to `Compound::new` would be an error, so drop them.
            ConvexPolygon::from_convex_hull(tri).map(|poly| (delta, SharedShape::new(poly)))
        })
        .collect();

    if shapes.is_empty() {
        // `Compound::new` panics on an empty shape list.
        return Err(MaskError::NoArea);
    }
    Ok(Compound::new(shapes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use parry2d_f64::query::PointQuery;

    /// Is `(x, y)` inside the mask? This mirrors how the feature detector uses
    /// it, minus the one-pixel tolerance it applies.
    fn inside(mask: &Mask, x: f64, y: f64) -> bool {
        let m = parry2d_f64::math::Pose::IDENTITY;
        mask.distance_to_point(&m, Vector::new(x, y), true) == 0.0
    }

    fn square() -> Vec<(f64, f64)> {
        vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]
    }

    /// An L: the square with its top-right quadrant bitten out.
    fn l_shape() -> Vec<(f64, f64)> {
        vec![
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 4.0),
            (4.0, 4.0),
            (4.0, 10.0),
            (0.0, 10.0),
        ]
    }

    #[test]
    fn concave_notch_is_excluded() {
        let mask = mask_from_points(&l_shape()).unwrap();
        // In both arms of the L.
        assert!(inside(&mask, 2.0, 2.0));
        assert!(inside(&mask, 8.0, 2.0));
        assert!(inside(&mask, 2.0, 8.0));
        // The bitten-out quadrant is outside, which is the whole point.
        assert!(!inside(&mask, 8.0, 8.0));
        assert!(!inside(&mask, 6.0, 6.0));
    }

    #[test]
    fn winding_direction_does_not_matter() {
        let mut reversed = l_shape();
        reversed.reverse();
        let mask = mask_from_points(&reversed).unwrap();
        assert!(inside(&mask, 2.0, 2.0));
        assert!(!inside(&mask, 8.0, 8.0));
    }

    #[test]
    fn convex_polygon_is_unchanged() {
        // Convex outlines are what worked before, and they must keep working
        // identically: interior in, exterior out.
        let mask = mask_from_points(&square()).unwrap();
        for (x, y) in [(5.0, 5.0), (0.5, 0.5), (9.5, 9.5), (0.0, 0.0), (10.0, 10.0)] {
            assert!(inside(&mask, x, y), "({x}, {y}) should be inside");
        }
        for (x, y) in [(-1.0, 5.0), (11.0, 5.0), (5.0, -1.0), (5.0, 11.0)] {
            assert!(!inside(&mask, x, y), "({x}, {y}) should be outside");
        }
    }

    #[test]
    fn a_closing_duplicate_vertex_is_accepted() {
        let mut closed = l_shape();
        closed.push(closed[0]);
        let mask = mask_from_points(&closed).unwrap();
        assert!(inside(&mask, 2.0, 2.0));
        assert!(!inside(&mask, 8.0, 8.0));
    }

    #[test]
    fn self_crossing_ring_falls_back_to_the_convex_hull() {
        // A bowtie: no well-defined interior. The old convex-hull reading is
        // kept, so anything that used to give a sensible mask still does.
        let bowtie = vec![(0.0, 0.0), (10.0, 10.0), (10.0, 0.0), (0.0, 10.0)];
        let mask = mask_from_points(&bowtie).unwrap();
        // The hull is the full square, including the middle of each edge that
        // the bowtie itself does not enclose.
        assert!(inside(&mask, 5.0, 5.0));
        assert!(inside(&mask, 1.0, 5.0));
        assert!(inside(&mask, 9.0, 5.0));
        assert!(!inside(&mask, 11.0, 5.0));
    }

    #[test]
    fn scrambled_convex_points_still_give_the_hull() {
        // Listing a convex polygon's vertices out of order makes a
        // self-crossing ring, so it keeps the old hull behavior rather than
        // becoming a star.
        let scrambled = vec![(0.0, 0.0), (10.0, 10.0), (0.0, 10.0), (10.0, 0.0)];
        let mask = mask_from_points(&scrambled).unwrap();
        assert!(inside(&mask, 5.0, 5.0));
        assert!(inside(&mask, 1.0, 1.0));
        assert!(inside(&mask, 9.0, 9.0));
    }

    #[test]
    fn a_star_polygon_keeps_its_points_and_valleys() {
        // Five-pointed star: simple (does not cross itself) but strongly
        // concave.
        let mut pts = Vec::new();
        for k in 0..10 {
            let ang = std::f64::consts::PI * 2.0 * (k as f64) / 10.0;
            let r = if k % 2 == 0 { 10.0 } else { 4.0 };
            pts.push((50.0 + r * ang.cos(), 50.0 + r * ang.sin()));
        }
        let mask = mask_from_points(&pts).unwrap();
        // Center is inside; a valley between two points is not.
        assert!(inside(&mask, 50.0, 50.0));
        assert!(inside(&mask, 59.0, 50.0));
        let valley_ang = std::f64::consts::PI * 2.0 / 10.0;
        let (vx, vy) = (50.0 + 8.0 * valley_ang.cos(), 50.0 + 8.0 * valley_ang.sin());
        assert!(!inside(&mask, vx, vy), "({vx}, {vy}) is in a valley");
    }

    #[test]
    fn degenerate_input_is_an_error_not_a_panic() {
        // `Compound::new` panics on an empty shape list, so these used to
        // abort the caller.
        // `Compound` has no `PartialEq`, so compare the error side only.
        assert_eq!(
            mask_from_points(&[]).unwrap_err(),
            MaskError::TooFewPoints(0)
        );
        assert_eq!(
            mask_from_points(&[(1.0, 1.0)]).unwrap_err(),
            MaskError::TooFewPoints(1)
        );
        assert_eq!(
            mask_from_points(&[(1.0, 1.0), (5.0, 5.0)]).unwrap_err(),
            MaskError::TooFewPoints(2)
        );
        // Repeated points collapse to too few.
        assert_eq!(
            mask_from_points(&[(1.0, 1.0), (1.0, 1.0), (1.0, 1.0)]).unwrap_err(),
            MaskError::TooFewPoints(1)
        );
        // Distinct but collinear: three points, no area.
        assert_eq!(
            mask_from_points(&[(1.0, 1.0), (2.0, 2.0), (3.0, 3.0)]).unwrap_err(),
            MaskError::NoArea
        );
    }

    #[test]
    fn simplicity_check_agrees_with_the_obvious_cases() {
        let ring = clean_ring(&square());
        assert!(is_simple(&ring));
        let ring = clean_ring(&l_shape());
        assert!(is_simple(&ring));
        let bowtie = clean_ring(&[(0.0, 0.0), (10.0, 10.0), (10.0, 0.0), (0.0, 10.0)]);
        assert!(!is_simple(&bowtie));
    }
}
