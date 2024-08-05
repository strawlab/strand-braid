pub type Mask = parry2d_f64::shape::Compound;

fn to_na(a: &delaunator::Point) -> parry2d_f64::math::Point<f64> {
    parry2d_f64::math::Point::new(a.x, a.y)
}

pub fn mask_from_points(viewport_points: &[(f64, f64)]) -> Mask {
    use parry2d_f64::shape::{Compound, ConvexPolygon, SharedShape};

    let points: Vec<_> = viewport_points
        .iter()
        .map(|p| delaunator::Point { x: p.0, y: p.1 })
        .collect();

    let delaun = delaunator::triangulate(&points);
    let delta = nalgebra::Isometry2::identity();

    let shapes: Vec<_> = delaun
        .triangles
        .chunks(3)
        .map(|idxs| {
            debug_assert_eq!(idxs.len(), 3);
            let points: Vec<_> = idxs.iter().map(|i| to_na(&points[*i])).collect();
            (
                delta,
                SharedShape::new(ConvexPolygon::from_convex_hull(&points).unwrap()),
            )
        })
        .collect();
    Compound::new(shapes)
}
