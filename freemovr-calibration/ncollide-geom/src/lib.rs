pub type Mask = ncollide2d::shape::Compound<f64>;

fn to_na(a: &delaunator::Point) -> ncollide2d::math::Point<f64> {
    ncollide2d::math::Point::new(a.x, a.y)
}

pub fn mask_from_points(viewport_points: &[(f64, f64)]) -> Mask {
    use ncollide2d::shape::{Compound, ConvexPolygon, ShapeHandle};

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
            let a = &points[idxs[0]];
            let b = &points[idxs[1]];
            let c = &points[idxs[2]];
            let a = to_na(a);
            let b = to_na(b);
            let c = to_na(c);
            let tri = ConvexPolygon::try_from_points(&[a, b, c])
                .expect("Convex hull computation failed.");
            (delta, ShapeHandle::new(tri))
        })
        .collect();
    Compound::new(shapes)
}
