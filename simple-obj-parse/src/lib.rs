use genmesh::EmitTriangles;
use nalgebra::geometry::{Point2, Point3};
use std::convert::TryInto;

mod error;

pub use crate::error::Error;

fn to_point2(x: [f32; 2]) -> Point2<f64> {
    Point2::new(x[0] as f64, x[1] as f64)
}

fn to_point3(x: [f32; 3]) -> Point3<f64> {
    Point3::new(x[0] as f64, x[1] as f64, x[2] as f64)
}

/// Load an .obj file and convert into `ncollide3d::shape::TriMesh<f64>`
///
/// Limitations: This converts very inefficiently, by simply duplicating
/// positions and texture coordinates for each triangle to make indexing easy.
/// Does not load normal data. Does not load materials.
pub fn obj_parse(
    buf: &[u8],
) -> Result<Vec<(String, ncollide3d::shape::TriMesh<f64>)>, crate::Error> {
    let mut reader = std::io::BufReader::new(buf);

    let obj = obj::ObjData::load_buf(&mut reader)?;

    let mut results = Vec::new();
    for o in &obj.objects {
        let mut coords: Vec<Point3<f64>> = Vec::new();
        let mut uvs: Vec<Point2<f64>> = Vec::new();
        let mut indices: Vec<Point3<usize>> = Vec::new();
        let mut has_uv = true;

        for g in &o.groups {
            for poly in g.polys.iter() {
                let mesh: genmesh::Polygon<_> = poly.clone().try_into()?;
                mesh.emit_triangles(|tri| {
                    let genmesh::Triangle {
                        ref x,
                        ref y,
                        ref z,
                    } = tri;

                    // iterate over each vert in the triangle
                    for vert in &[x, y, z] {
                        let pos_idx = vert.0;
                        if let Some(ref uv_idx) = vert.1 {
                            coords.push(to_point3(obj.position[pos_idx]));
                            uvs.push(to_point2(obj.texture[*uv_idx]));
                        } else {
                            has_uv = false;
                        }
                    }

                    if has_uv {
                        let last_idx = coords.len();
                        indices.push(Point3::new(last_idx - 3, last_idx - 2, last_idx - 1));
                    }
                });
            }
        }

        if !has_uv {
            use crate::error::ErrorKind::ObjHasNoTextureCoords;
            return Err(crate::Error::new(ObjHasNoTextureCoords));
        }

        let trimesh_f64 = ncollide3d::shape::TriMesh::<f64>::new(coords, indices, Some(uvs));

        results.push((o.name.clone(), trimesh_f64))
    }
    Ok(results)
}
