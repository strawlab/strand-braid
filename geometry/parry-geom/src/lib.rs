// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

pub type Mask = parry2d_f64::shape::Compound;

fn to_parry_vector(a: &delaunator::Point) -> parry2d_f64::math::Vector {
    parry2d_f64::math::Vector::new(a.x, a.y)
}

pub fn mask_from_points(viewport_points: &[(f64, f64)]) -> Mask {
    use parry2d_f64::shape::{Compound, ConvexPolygon, SharedShape};

    let points: Vec<_> = viewport_points
        .iter()
        .map(|p| delaunator::Point { x: p.0, y: p.1 })
        .collect();

    let delaun = delaunator::triangulate(&points);
    let delta = parry2d_f64::math::Pose::IDENTITY;

    let shapes: Vec<_> = delaun
        .triangles
        .chunks(3)
        .map(|idxs| {
            debug_assert_eq!(idxs.len(), 3);
            let points: Vec<_> = idxs.iter().map(|i| to_parry_vector(&points[*i])).collect();
            (
                delta,
                SharedShape::new(ConvexPolygon::from_convex_hull(&points).unwrap()),
            )
        })
        .collect();
    Compound::new(shapes)
}
