use nalgebra::{Point2, Point3, Isometry3};

use ncollide3d::shape::TriMesh;
use ncollide3d::shape::TrianglePointLocation;
use ncollide3d::query::PointQueryWithLocation;

pub trait FaceIndices<N: nalgebra::RealField> {
    fn indices(&self) -> Vec<Point3<usize>>;
}

impl<N: nalgebra::RealField> FaceIndices<N> for TriMesh<N> {
    fn indices(&self) -> Vec<Point3<usize>> {
        self.faces().iter().map(|face| face.indices.clone()).collect()
    }
}

pub trait UvPosition<N: nalgebra::RealField> {
    fn project_point_to_uv(&self, m: &Isometry3<N>, point: &Point3<N>, solid: bool) -> Option<Point2<N>>;
}

impl<N: nalgebra::RealField> UvPosition<N> for TriMesh<N> {
    fn project_point_to_uv(&self, m: &Isometry3<N>, point: &Point3<N>, _solid: bool) -> Option<Point2<N>> {
        if let Some(ref uvs) = self.uvs() {

            // tri_idx is the number of the triangle
            let (pp, (tri_idx, loc)) = self.project_point_with_location(&m, point, true);
            if !pp.is_inside {
                // TODO sometimes this fails when it shouldn't. I'm not exactly
                // sure what is wrong. Look at implementation of ncollide3d
                // toi_and_normal_and_uv_with_ray to see how this can be
                // avoided.
                return None;
            }

            // tri_indices are the indices of into e.g. uvs for this triangle
            let tri_indices = self.indices()[tri_idx];

            match loc {
                TrianglePointLocation::OnVertex(i) => {
                    let idx = tri_indices[i];
                    Some(uvs[idx])
                },
                TrianglePointLocation::OnEdge(edge_num,bcoords) => {
                    let (idx0, idx1) = match edge_num {
                        0 => unimplemented!(),
                        1 => (tri_indices[1], tri_indices[2]),
                        2 => (tri_indices[0], tri_indices[2]),
                        _ => panic!("triangle with >3 edges?"),
                    };
                    let uv = uvs[idx0].coords*bcoords[0] + uvs[idx1].coords*bcoords[1];
                    Some(uv.into())
                },
                TrianglePointLocation::OnFace(_face_idx,bcoords) => {
                    let uv = uvs[tri_indices[0]].coords*bcoords[0] +
                        uvs[tri_indices[1]].coords*bcoords[1] +
                        uvs[tri_indices[2]].coords*bcoords[2];
                    Some(uv.into())
                },
                TrianglePointLocation::OnSolid => {
                    panic!("impossible: TriMesh OnSolid");
                },
            }

        } else {
            None
        }
    }
}


#[cfg(test)]
mod tests {
    use nalgebra::geometry::{Point2, Point3};

    #[test]
    fn test_trimesh() {
        let trimesh = {
            let coords = vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(0.5, 1.0, 0.0),
                Point3::new(1.5, 1.0, 0.0),
                Point3::new(2.0, 0.0, 0.0),
            ];
            let uvs = vec![
                Point2::new(0.0, 0.0),
                Point2::new(0.5, 0.0),
                Point2::new(0.25, 1.0),
                Point2::new(0.75, 1.0),
                Point2::new(1.0, 0.0),
            ];
            let indices = vec![
                Point3::new(0, 1, 2),
                Point3::new(1, 2, 3),
                Point3::new(1, 3, 4),
            ];
            ncollide3d::shape::TriMesh::<f64>::new( coords, indices, Some(uvs))
        };



        let m = nalgebra::geometry::Isometry3::identity();
        let test_wcs = vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.5, 1.0, 0.0),
            Point3::new(1.5, 1.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),

            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.5, 0.0, 0.0),
            Point3::new(0.5, 0.5, 0.0),
            Point3::new(1.5, 0.0, 0.0),
            Point3::new(1.5, 1.0, 0.0),
        ];
        for wc in test_wcs.iter() {
            use crate::trimesh_ext::UvPosition;
            let _uv = trimesh.project_point_to_uv(&m, wc, true);
        }
    }
}
