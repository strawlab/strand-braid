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

    let delaun = delaunator::triangulate(&points).expect("No triangulation exists.");
    let delta = nalgebra::Isometry2::identity();

    let shapes: Vec<_> = delaun
        .triangles
        .chunks(3)
        .map(|idxs| {
            let a = &points[idxs[0]];
            let b = &points[idxs[1]];
            let c = &points[idxs[2]];
            let a = to_na(a);
            let b = to_na(b);
            let c = to_na(c);
            let tri = ConvexPolygon::from_convex_hull(&[a, b, c])
                .expect("Convex hull computation failed.");
            (delta, SharedShape::new(tri))
        })
        .collect();
    Compound::new(shapes)
}
