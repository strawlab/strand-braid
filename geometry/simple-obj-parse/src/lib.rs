use genmesh::EmitTriangles;

mod error;

pub use crate::error::Error;

fn to_point2(x: [f32; 2]) -> [f64; 2] {
    [x[0] as f64, x[1] as f64]
}

fn to_point3(x: [f32; 3]) -> [f64; 3] {
    [x[0] as f64, x[1] as f64, x[2] as f64]
}

/// Load an .obj file and convert into [textured_tri_mesh::TriMesh]
///
/// Limitations: This converts very inefficiently, by simply duplicating
/// positions and texture coordinates for each triangle to make indexing easy.
/// Does not load normal data. Does not load materials.
pub fn obj_parse(buf: &[u8]) -> Result<Vec<(String, textured_tri_mesh::TriMesh)>, crate::Error> {
    let mut reader = std::io::BufReader::new(buf);

    let obj = obj::ObjData::load_buf(&mut reader)?;

    let mut results = Vec::new();
    for o in &obj.objects {
        let mut coords = Vec::new();
        let mut uvs = Vec::new();
        let mut indices = Vec::new();
        let mut has_uv = true;
        let mut too_many_verts = false;

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
                        match <usize as TryInto<u32>>::try_into(coords.len()) {
                            Ok(last_idx) => {
                                indices.push([last_idx - 3, last_idx - 2, last_idx - 1]);
                            }
                            Err(_) => {
                                too_many_verts = true;
                            }
                        }
                    }
                });
            }
        }

        if !has_uv {
            return Err(crate::Error::new(
                crate::error::ErrorKind::ObjHasNoTextureCoords,
            ));
        }

        if too_many_verts {
            return Err(crate::Error::new(
                crate::error::ErrorKind::ObjHasTooManyVertices,
            ));
        }

        let trimesh_f64 = textured_tri_mesh::TriMesh {
            coords,
            indices,
            uvs,
        };

        results.push((o.name.clone(), trimesh_f64))
    }
    Ok(results)
}
