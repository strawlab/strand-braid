extern crate machine_vision_shaders as shaders;

extern crate glium;

use glium::{glutin, DisplayBuild, Surface};
use glium::vertex::VertexBufferAny;
use glium::index::IndexBufferAny;
use glium::backend::Facade;

use reactive_cam::Frame;
use super::support::poll_for_quit;
use super::observation::Observation;

pub struct ExtractedFrame {
    pub frame: Frame,
    pub draw_features: Observation,
}

pub struct CameraView {
    camera_view_window: glium::backend::glutin_backend::GlutinFacade,
    opengl_texture: Option<glium::texture::Texture2d>,
    p_buffer: Option<glium::texture::pixel_buffer::PixelBuffer<u8>>,
    program: Option<glium::Program>,
    uniform_type: Option<shaders::UniformType>,
    camera_window_verts: VertexBufferAny,
    camera_window_indices: IndexBufferAny,
    feature_program: glium::Program,
}

fn tracked_points_vertex_buffer<F>(display: &F,
                                   xp: f32,
                                   yp: f32,
                                   wp: u32,
                                   hp: u32,
                                   sz: u32)
                                   -> (VertexBufferAny, IndexBufferAny)
    where F: Facade
{
    let x = 2.0 * xp / wp as f32 - 1.0;
    let y = 2.0 * (hp as f32 - yp) / hp as f32 - 1.0;
    let w = sz as f32 / wp as f32;
    let h = sz as f32 / hp as f32;
    get_quad_bufs(display, x - w, x + w, y - h, y + w)
}

impl CameraView {
    pub fn new() -> CameraView {
        let camera_view_window = glutin::WindowBuilder::new()
            .with_title(format!("Camera View"))
            .with_dimensions(640, 480)
            .build_glium()
            .unwrap();

        let (camera_window_verts, camera_window_indices) =
            load_simple_display_verts(&camera_view_window);

        let feature_vert_src = "#version 140

in vec2 position;
in vec2 tex_coords;
out vec2 v_tex_coords;

void main() {
    v_tex_coords = tex_coords;
    gl_Position = vec4(position, 0.0, 1.0);
}";
        let feature_frag_src = "#version 140

in vec2 v_tex_coords;
out vec4 color;

// uniform sampler2D tex;

void main() {
    // float c = texture(tex, v_tex_coords).x;
    color = vec4(0.4, 1.0, 0.4, 1.0);
}";
        let feature_program = glium::Program::from_source(&camera_view_window,
                                                          feature_vert_src,
                                                          feature_frag_src,
                                                          None)
            .unwrap();

        CameraView {
            camera_view_window: camera_view_window,
            opengl_texture: None,
            p_buffer: None,
            program: None,
            uniform_type: None,
            camera_window_verts: camera_window_verts,
            camera_window_indices: camera_window_indices,
            feature_program: feature_program,
        }
    }
    fn update_texture_data(&mut self, frame: &Frame) {
        if let Some(ref opengl_texture) = self.opengl_texture {
            if let Some(ref p_buffer) = self.p_buffer {
                p_buffer.write(&frame.image_data);
                opengl_texture.main_level()
                    .raw_upload_from_pixel_buffer(p_buffer.as_slice(),
                                                  0..frame.roi.width,
                                                  0..frame.roi.height,
                                                  0..1);
            } else {
                unreachable!();
            }
        } else {
            unreachable!();
        }
    }
    fn initialize_if_needed(&mut self, frame: &Frame) {
        let data_h = frame.roi.height;

        if self.opengl_texture.is_none() {
            // perform initial allocations

            self.opengl_texture = Some(glium::Texture2d::empty_with_format(&self.camera_view_window,
                glium::texture::UncompressedFloatFormat::U8,
                glium::texture::MipmapsOption::NoMipmap,
                frame.roi.width, data_h).unwrap());

            let n_pixels = frame.stride * data_h; // make stride width for easy copy
            self.p_buffer =
                Some(glium::texture::pixel_buffer::PixelBuffer::new_empty(&self.camera_view_window,
                                                                          n_pixels as usize));

            let (uni_ty, vert_src, frag_src) =
                shaders::get_programs(frame.roi.width, frame.roi.height, frame.pixel_format);

            self.program = Some(glium::Program::from_source(&self.camera_view_window,
                                                            vert_src,
                                                            frag_src,
                                                            None)
                .unwrap());
            self.uniform_type = Some(uni_ty);
        }
    }
    fn draw(&mut self, draw_features: &Observation) -> bool {

        let mut obs_data = None;

        if let &Some(ref feature) = draw_features.feature() {
            if let Some(ref tex) = self.opengl_texture {
                let (vbuf, ibuf) = tracked_points_vertex_buffer(&self.camera_view_window,
                                                                feature.x(),
                                                                feature.y(),
                                                                tex.get_width(),
                                                                tex.get_height()
                                                                    .expect("tex height"),
                                                                10);
                obs_data = Some((vbuf, ibuf));
            }
        }


        // drawing a frame
        let mut target = self.camera_view_window.draw();
        target.clear_color(1.0, 1.0, 1.0, 1.0);

        let mut all_some = false;
        if let Some(ref uni_ty) = self.uniform_type {
            if let Some(ref program) = self.program {
                if let Some(ref tex) = self.opengl_texture {
                    match uni_ty {
                        &shaders::UniformType::Mono8 => {
                            let uniforms = uniform! {
                                tex: tex,
                            };
                            target.draw(&self.camera_window_verts,
                                      &self.camera_window_indices,
                                      program,
                                      &uniforms,
                                      &Default::default())
                                .unwrap();
                        }
                        &shaders::UniformType::Bayer(ref di) => {
                            let uniforms = uniform! {
                               source: tex,
                               sourceSize: di.source_size,
                               firstRed: di.first_red,
                           };
                            target.draw(&self.camera_window_verts,
                                      &self.camera_window_indices,
                                      program,
                                      &uniforms,
                                      &Default::default())
                                .unwrap();
                        }
                    }
                    all_some = true;
                }
            }
        }
        if !all_some {
            // Some of the variables were not assigned from their initial value of None.
            unreachable!();
        }

        if let Some((vbuf, ibuf)) = obs_data {
            let uniforms = uniform!{};
            target.draw(&vbuf,
                      &ibuf,
                      &self.feature_program,
                      &uniforms,
                      &Default::default())
                .unwrap();
        }

        target.finish().unwrap();

        poll_for_quit(&self.camera_view_window)
    }
    pub fn display_step(&mut self, maybe_frame: Option<ExtractedFrame>) -> bool {
        match maybe_frame {
            None => true, // There was no new frame. Therefore no point in drawing anything.
            Some(extracted) => {
                self.initialize_if_needed(&extracted.frame);
                self.update_texture_data(&extracted.frame);
                self.draw(&extracted.draw_features)
            }
        }
    }
}

fn load_simple_display_verts<F>(display: &F) -> (VertexBufferAny, IndexBufferAny)
    where F: Facade
{
    get_quad_bufs(display, -1.0, 1.0, 1.0, -1.0)
}

fn get_quad_bufs<F>(display: &F,
                    xmin: f32,
                    xmax: f32,
                    ymin: f32,
                    ymax: f32)
                    -> (VertexBufferAny, IndexBufferAny)
    where F: Facade
{
    #[derive(Copy, Clone)]
    struct Vertex {
        position: [f32; 2],
        tex_coords: [f32; 2],
    }

    implement_vertex!(Vertex, position, tex_coords);

    let vertex1 = Vertex {
        position: [xmin, ymin],
        tex_coords: [0.0, 0.0],
    };
    let vertex2 = Vertex {
        position: [xmin, ymax],
        tex_coords: [0.0, 1.0],
    };
    let vertex3 = Vertex {
        position: [xmax, ymax],
        tex_coords: [1.0, 1.0],
    };
    let vertex4 = Vertex {
        position: [xmax, ymin],
        tex_coords: [1.0, 0.0],
    };
    let shape = vec![vertex1, vertex2, vertex3, vertex1, vertex3, vertex4];
    let vertex_buffer1 = glium::VertexBuffer::immutable(display, &shape).unwrap();

    let vertex_buffer = vertex_buffer1.into_vertex_buffer_any();

    let mut index_data: Vec<u16> = Vec::new();
    for i in 0..6 {
        index_data.push(i);
    }
    let indices = glium::IndexBuffer::immutable(display,
                                                glium::index::PrimitiveType::TrianglesList,
                                                &&index_data)
        .expect("creating index buffer");

    let index_buffer: IndexBufferAny = From::from(indices);

    (vertex_buffer, index_buffer)
}
