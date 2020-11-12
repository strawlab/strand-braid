extern crate glium;

use glium::vertex::VertexBufferAny;
use glium::index::IndexBufferAny;
use glium::texture::Texture2d;
use glium::{glutin, Surface};

use std::path::Path;
use std::fs::File;
use std::sync::{Arc, Mutex};
use std::io::Read;

use super::config;
use super::obj_load;
use super::tracker::Tracker;

use cgmath::{self, SquareMatrix};

/// A flat display with size (width, height). The units are the same as returned by the tracker
/// and the same as specified in the .obj model file.
pub struct VRDisplay<'a> {
    display: glium::backend::glutin_backend::GlutinFacade,
    obj_verts: VertexBufferAny,
    obj_indices: IndexBufferAny,
    maybe_texture: Option<Texture2d>,
    program: glium::Program,
    vr_cfg: &'a config::VRDisplayConfig,
    tracker: Arc<Mutex<Tracker>>,
    r: f32,
    screen_half_width: f32,
    screen_half_height: f32,
    events_loop: u8,
}

fn load_path(path: &Path) -> String {
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(e) => {
            panic!("could not open file {:?}: {}", path, e);
        }
    };
    let mut buffer = String::new();
    file.read_to_string(&mut buffer).expect("failed reading file");
    buffer
}

impl<'a> VRDisplay<'a> {
    pub fn new(vr_cfg: &'a config::VRDisplayConfig,
               tracker: Arc<Mutex<Tracker>>,
               base_path: &Path)
               -> VRDisplay<'a> {
        let mut events_loop = glutin::EventsLoop::new();
        let window = glutin::WindowBuilder::new().with_title("VR Display");
        let context = glutin::ContextBuilder::new().with_vsync(false);
        let display = glium::Display::new(window, context, &events_loop).unwrap();
            // .with_dimensions(vr_cfg.window_preferred_width_pixels,
            //                  vr_cfg.window_preferred_height_pixels)

        let model_fname = base_path.join(&vr_cfg.model_fname);

        let (obj_verts, obj_indices, maybe_texture) = obj_load::obj_load(&display, &model_fname)
            .expect("load obj");

        let vert_src = load_path(&base_path.join(&vr_cfg.vert_shader_fname));
        let frag_src = load_path(&base_path.join(&vr_cfg.frag_shader_fname));

        let program = glium::Program::from_source(&display, &vert_src, &frag_src, None).unwrap();

        VRDisplay {
            display: display,
            maybe_texture: maybe_texture,
            obj_verts: obj_verts,
            obj_indices: obj_indices,
            program: program,
            vr_cfg: vr_cfg,
            tracker: tracker,
            r: 0.0,
            screen_half_width: vr_cfg.width_meters / 2.0,
            screen_half_height: vr_cfg.width_meters / 2.0,
            events_loop,
        }
    }
    pub fn display_step(&mut self) -> bool {

        let maybe_obj_state = self.tracker.lock().unwrap().get_state();
        let point = match maybe_obj_state {
            Some(point) => point,
            None => {
                let mut running = true;
                self.events_loop.poll_events(|ev| match ev {
                    glutin::Event::WindowEvent { event, .. } => {
                        match event {
                            glutin::WindowEvent::Closed => running = false,
                            glutin::WindowEvent::KeyboardInput {
                                input, ..
                            } if glutin::ElementState::Pressed == input.state => {
                                if let glutin::VirtualKeyCode::Escape = key {
                                    running = false
                                }
                            }
                            _ => (),
                        }
                    }
                    _ => (),
                });
                return running;
            }
        };

        // -----------------
        // drawing a frame
        let mut target = self.display.draw();
        let (width, height) = target.get_dimensions();

        target.clear_color(self.r, 0.8, 0.6, 1.0);

        // Flicker the background just so we know we have updates doing on. TODO: remove from
        // production code.
        self.r += 0.01;
        if self.r > 1.0 {
            self.r = 0.0;
        }

        // draw VR world -------------------------------------------------------------------------
        {
            // World coordinates of screen are that (0,0,0) is in center of screen.
            let display_min_x = -self.screen_half_width;
            let display_max_x = self.screen_half_width;
            let display_min_y = -self.screen_half_height;
            let display_max_y = self.screen_half_height;
            let left = display_min_x - point.x;
            let right = display_max_x - point.x;
            let bottom = display_min_y - point.y;
            let top = display_max_y - point.y;

            // MAKE THESE TWO CONFIGURABLE!!!
            let near = self.vr_cfg.distance_to_screen_meters;
            let far = self.vr_cfg.far_clip_meters;

            let projection_matrix: cgmath::Matrix4<f32> =
                cgmath::frustum(left, right, bottom, top, near, far);

            let view_eye: cgmath::Point3<f32> = cgmath::Point3::new(point.x, point.y, 0.0);
            let (center_z, up_y) = if self.vr_cfg.screen_above_observer {
                (1.0, 1.0)
            } else {
                (-1.0, -1.0)
            };
            let view_center: cgmath::Point3<f32> = cgmath::Point3::new(point.x, point.y, center_z);
            let view_up: cgmath::Vector3<f32> = cgmath::Vector3::new(0.0, up_y, 0.0);
            let view_matrix: cgmath::Matrix4<f32> =
                cgmath::Matrix4::look_at(view_eye, view_center, view_up);
            let model_matrix: cgmath::Matrix4<f32> = cgmath::Matrix4::identity();

            let viewport = match self.vr_cfg.show_overview {
                true => {
                    // show VR world in bottom half of viewport
                    Some(glium::Rect {
                        left: 0,
                        bottom: 0,
                        width: width,
                        height: height / 2,
                    })
                }
                false => None, // fullscreen
            };
            let params = glium::DrawParameters { viewport: viewport, ..Default::default() };

            match self.maybe_texture {
                Some(ref tex) => {
                    // The model has a texture.
                    let uniforms = uniform! {
                       tex: tex,
                       projection_matrix: Into::<[[f32; 4]; 4]>::into(projection_matrix),
                       view_matrix: Into::<[[f32; 4]; 4]>::into(view_matrix),
                       model_matrix: Into::<[[f32; 4]; 4]>::into(model_matrix),
                    };
                    target.draw(&self.obj_verts,
                              &self.obj_indices,
                              &self.program,
                              &uniforms,
                              &params)
                        .unwrap();
                }
                None => {
                    // The model has no texture.
                    let uniforms = uniform! {
                        projection_matrix: Into::<[[f32; 4]; 4]>::into(projection_matrix),
                        view_matrix: Into::<[[f32; 4]; 4]>::into(view_matrix),
                        model_matrix: Into::<[[f32; 4]; 4]>::into(model_matrix),
                    };
                    target.draw(&self.obj_verts,
                              &self.obj_indices,
                              &self.program,
                              &uniforms,
                              &params)
                        .unwrap();
                }
            }
        }

        // draw overview -------------------------------------------------------------------------
        if self.vr_cfg.show_overview {
            let viewport = match self.vr_cfg.show_overview {
                true => {
                    // show VR world in bottom half of viewport
                    Some(glium::Rect {
                        left: 0,
                        bottom: height / 2,
                        width: width,
                        height: height / 2,
                    })
                }
                false => None, // fullscreen
            };
            let params = glium::DrawParameters { viewport: viewport, ..Default::default() };

            let aspect_ratio = width as f32 / height as f32;
            let projection_matrix: cgmath::Matrix4<f32> =
                cgmath::perspective(cgmath::deg(45.0), aspect_ratio, 0.0001, 100.0);

            let view_eye: cgmath::Point3<f32> = cgmath::Point3::new(self.vr_cfg.overview_cam_x,
                                                                    self.vr_cfg.overview_cam_y,
                                                                    self.vr_cfg.overview_cam_z);
            let view_center: cgmath::Point3<f32> = cgmath::Point3::new(0.0, 0.0, 0.0);
            let view_up: cgmath::Vector3<f32> = cgmath::Vector3::new(0.0, 0.0, 1.0); // up is +Z
            let view_matrix: cgmath::Matrix4<f32> =
                cgmath::Matrix4::look_at(view_eye, view_center, view_up);
            let model_matrix: cgmath::Matrix4<f32> = cgmath::Matrix4::identity();

            // TODO: draw location of observer

            match self.maybe_texture {
                Some(ref tex) => {
                    // The model has a texture.
                    let uniforms = uniform! {
                       tex: tex,
                       projection_matrix: Into::<[[f32; 4]; 4]>::into(projection_matrix),
                       view_matrix: Into::<[[f32; 4]; 4]>::into(view_matrix),
                       model_matrix: Into::<[[f32; 4]; 4]>::into(model_matrix),
                    };
                    target.draw(&self.obj_verts,
                              &self.obj_indices,
                              &self.program,
                              &uniforms,
                              &params)
                        .unwrap();
                }
                None => {
                    // The model has no texture.
                    let uniforms = uniform! {
                        projection_matrix: Into::<[[f32; 4]; 4]>::into(projection_matrix),
                        view_matrix: Into::<[[f32; 4]; 4]>::into(view_matrix),
                        model_matrix: Into::<[[f32; 4]; 4]>::into(model_matrix),
                    };
                    target.draw(&self.obj_verts,
                              &self.obj_indices,
                              &self.program,
                              &uniforms,
                              &params)
                        .unwrap();
                }
            }
        }

        target.finish().unwrap();
        poll_for_quit(&self.display)
    }
}
