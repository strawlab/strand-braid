extern crate glium;
extern crate tobj;
extern crate image;

use std::f32;
use std::path::Path;

use glium::vertex::VertexBufferAny;
use glium::index::IndexBufferAny;
use glium::backend::Facade;
use glium::texture::Texture2d;

use obj_load::image::GenericImage;

macro_rules! warn_unsupported {
    ($mat:expr, $name:ident) => {{
        if $mat.$name.len() > 0 {error!("unsupported: {}", stringify!($name));}
    }}
}


pub fn obj_load<F>(display: &F,
                   path: &::std::path::Path)
                   -> Result<(VertexBufferAny, IndexBufferAny, Option<Texture2d>), ()>
    where F: Facade
{

    #[derive(Copy, Clone)]
    struct Vertex {
        position: [f32; 3],
        tex_coords: [f32; 2],
        normal: [f32; 3],
        color_diffuse: [f32; 3],
        color_specular: [f32; 4],
    }

    implement_vertex!(Vertex,
                      position,
                      tex_coords,
                      normal,
                      color_diffuse,
                      color_specular);

    let base_path = path.parent();

    let mut min_pos = [f32::INFINITY; 3];
    let mut max_pos = [f32::NEG_INFINITY; 3];
    let mut vertex_data = Vec::new();
    let mut index_data = Vec::new();
    let mut vert_count: u16 = 0;
    let mut texture = None;
    match tobj::load_obj(path) {
        Ok((models, mats)) => {
            for mat in &mats {
                warn_unsupported!(mat, ambient_texture);
                warn_unsupported!(mat, specular_texture);
                warn_unsupported!(mat, normal_texture);
                warn_unsupported!(mat, dissolve_texture);

                let tname: String = mat.diffuse_texture.to_string();
                if tname.len() > 0 {
                    let tex_path = match base_path {
                        Some(bp) => bp.join(&tname),
                        None => Path::new(&tname).to_path_buf(),
                    };
                    let image = image::open(&tex_path).expect("opened texture");
                    let image_dimensions = image.dimensions();
                    // TODO: check if we should open this as sRGB texture.

                    let img = image.to_rgb();
                    let ri2 = glium::texture::RawImage2d::from_raw_rgb_reversed(img.into_raw(),
                                                                                image_dimensions);
                    texture = Some(glium::texture::Texture2d::new(display, ri2)
                        .expect("converted RawImage2D to Texture2d"));

                }
            }
            for model in &models {
                let mesh = &model.mesh;
                for idx in &mesh.indices {
                    let i = *idx as usize;
                    index_data.push(vert_count); // We are removing any ordering in this loop.
                    vert_count += 1;
                    let pos = [mesh.positions[3 * i],
                               mesh.positions[3 * i + 1],
                               mesh.positions[3 * i + 2]];
                    let tex_coords = [mesh.texcoords[2 * i], mesh.texcoords[2 * i + 1]];
                    let normal = if !mesh.normals.is_empty() {
                        [mesh.normals[3 * i], mesh.normals[3 * i + 1], mesh.normals[3 * i + 2]]
                    } else {
                        [0.0, 0.0, 0.0]
                    };
                    let (color_diffuse, color_specular) = match mesh.material_id {
                        Some(i) => {
                            (mats[i].diffuse,
                             [mats[i].specular[0],
                              mats[i].specular[1],
                              mats[i].specular[2],
                              mats[i].shininess])
                        }
                        None => ([0.8, 0.8, 0.8], [0.15, 0.15, 0.15, 15.0]),
                    };
                    vertex_data.push(Vertex {
                        position: pos,
                        tex_coords: tex_coords,
                        normal: normal,
                        color_diffuse: color_diffuse,
                        color_specular: color_specular,
                    });
                    // Update our min/max pos so we can figure out the bounding box of the object
                    // to view it
                    for i in 0..3 {
                        min_pos[i] = f32::min(min_pos[i], pos[i]);
                        max_pos[i] = f32::max(max_pos[i], pos[i]);
                    }
                }
            }
        }
        Err(e) => panic!("Loading of {:?} failed due to {:?}", path, e),
    }

    let indices = glium::IndexBuffer::immutable(display,
                                                glium::index::PrimitiveType::TrianglesList,
                                                &&index_data)
        .expect("creating index buffer");

    let vertex_buffer =
        glium::vertex::VertexBuffer::new(display, &vertex_data).unwrap().into_vertex_buffer_any();

    let index_buffer: IndexBufferAny = From::from(indices);
    Ok((vertex_buffer, index_buffer, texture))
}
