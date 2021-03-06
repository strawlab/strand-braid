#[macro_use]
extern crate log;
extern crate env_logger;
extern crate crossbeam_channel;
extern crate failure;
extern crate time;
extern crate machine_vision_formats as formats;
extern crate machine_vision_shaders as shaders;
extern crate convert_image;

#[macro_use]
extern crate glium;

#[cfg(feature="fly-eye")]
mod fly_eye;
#[cfg(feature="screen-quad")]
mod screen_quad;

use crossbeam_channel::Receiver;
use glium::{glutin, Surface};

#[cfg(feature="fly-eye")]
use fly_eye as coords;
#[cfg(feature="screen-quad")]
use screen_quad as coords;

pub struct App<F>
    where
        F: formats::ImageStride,
        Vec<u8>: From<Box<F>>
{
    pub rx: Receiver<Box<F>>,
}

struct Inner {
    opengl_texture: glium::texture::Texture2d,
    p_buffer: glium::texture::pixel_buffer::PixelBuffer<u8>,
    program: glium::Program,
    uniform_type: shaders::UniformType,
}

impl<F> App<F>
    where
        F: formats::ImageStride,
        Vec<u8>: From<Box<F>>
{
    pub fn mainloop(&mut self) -> Result<(), failure::Error> {

        let mut events_loop = glutin::EventsLoop::new();
        let window = glutin::WindowBuilder::new().with_title("Fly Eye");
        let context = glutin::ContextBuilder::new().with_vsync(true);
        let display = glium::Display::new(window, context, &events_loop).expect("open display");

        let vertex_buffer = glium::VertexBuffer::immutable(&display, &coords::VERTEX_DATA).unwrap();
        let indices = glium::IndexBuffer::immutable(&display, glium::index::PrimitiveType::TrianglesList,
              &coords::INDEX_DATA).unwrap();

        let mut inner = None;

        let mut running = true;
        while running {

            let result_frame = match inner {
                Some(_) => get_most_recent_frame(&self.rx), // normal case, get frame if available
                None => Ok(self.rx.recv()?), // ensure we have first frame
            };

            if let Ok(frame) = result_frame {
                let width = frame.width();
                let height = frame.height();
                let stride = frame.stride();
                let pixel_format = frame.pixel_format();
                let imdata: Vec<u8> = frame.into();

                if inner.is_none() {

                    // perform initial allocations

                    let (uni_ty, vert_src, frag_src, ifmt) = shaders::get_programs(
                        width, height, pixel_format);

                    debug!("using internal format {:?}", ifmt);

                    let format = match ifmt {
                        shaders::InternalFormat::Rgb8 => glium::texture::UncompressedFloatFormat::U8U8U8U8,
                        shaders::InternalFormat::Raw8 => glium::texture::UncompressedFloatFormat::U8,
                    };

                    let opengl_texture = match pixel_format {
                        formats::PixelFormat::RGB8 => {
                            let texdata = glium::texture::RawImage2d::from_raw_rgb(imdata.clone(),
                                (width, height));
                            glium::Texture2d::new(&display, texdata).unwrap()
                        },
                        _ => {
                            glium::Texture2d::empty_with_format(&display,
                                format,
                                glium::texture::MipmapsOption::NoMipmap,
                                width, height).unwrap()
                        },
                    };

                    let n_pixels = stride as u32 * height; // make stride width for easy copy
                    let p_buffer =
                        glium::texture::pixel_buffer::PixelBuffer::new_empty(&display,
                                                                                  n_pixels as usize);

                    let program =
                        glium::Program::from_source(&display, vert_src, frag_src, None)?;
                    let uniform_type = uni_ty;
                    inner = Some(Inner {
                        program,
                        opengl_texture,
                        p_buffer,
                        uniform_type,
                    })
                }

                if let Some(ref inner) = inner {
                    if pixel_format == formats::PixelFormat::RGB8 {
                        unimplemented!("RGB data not coverted to pbuffer");
                    }
                    inner.p_buffer.write(&imdata);
                    inner.opengl_texture.main_level()
                        .raw_upload_from_pixel_buffer(inner.p_buffer.as_slice(),
                                                        0..width,
                                                        0..height,
                                                        0..1);
                } else {
                    panic!("reached unreachable state");
                }
            } else {
                error!("ignoring error ({}:{})", file!(), line!());
            }

            // drawing a frame
            let mut target = display.draw();
            target.clear_color(1.0, 1.0, 1.0, 1.0);

            if let Some(ref inner) = inner {
                match inner.uniform_type {
                    shaders::UniformType::Mono8 | shaders::UniformType::Rgb8 => {
                        let uniforms = uniform! {
                            tex: &inner.opengl_texture,
                        };
                        target.draw(&vertex_buffer,
                                    &indices,
                                    &inner.program,
                                    &uniforms,
                                    &Default::default())?;
                    }
                    shaders::UniformType::Bayer(ref di) => {
                        let uniforms = uniform! {
                            source: &inner.opengl_texture,
                            sourceSize: di.source_size,
                            firstRed: di.first_red,
                        };
                        target.draw(&vertex_buffer,
                                    &indices,
                                    &inner.program,
                                    &uniforms,
                                    &Default::default())?;
                    }
                }
            } else {
                target.finish()?;
                panic!("inner is None");
            }

            target.finish()?;

            events_loop.poll_events(|ev| match ev {
                glutin::Event::WindowEvent { event, .. } => {
                    match event {
                        glutin::WindowEvent::CloseRequested => running = false,
                        glutin::WindowEvent::KeyboardInput {
                            input, ..
                        } if glutin::ElementState::Pressed == input.state => {
                            // if let glutin::VirtualKeyCode::Escape = key {
                            running = false
                        }
                        _ => (),
                    }
                }
                _ => (),
            });

        }
        Ok(())
    }
}

/// check if a frame is available. if yes, get it and keep getting until most recent.
fn get_most_recent_frame<F>(receiver: &Receiver<Box<F>>)
                         -> Result<Box<F>,crossbeam_channel::TryRecvError>
{
    let mut result = Err(crossbeam_channel::TryRecvError::Empty);
    loop {
        match receiver.try_recv() {
            Ok(r) => result = Ok(r),
            Err(crossbeam_channel::TryRecvError::Empty) => break,
            Err(crossbeam_channel::TryRecvError::Disconnected) => {
                return Err(crossbeam_channel::TryRecvError::Disconnected);
            },
        }
    }
    result
}

/// run a function returning Result<()> and handle errors.
// see https://github.com/withoutboats/failure/issues/76#issuecomment-347402383
pub fn run_func<F: FnOnce() -> Result<(), failure::Error>>(real_func: F) {
    // Decide which command to run, and run it, and print any errors.
    if let Err(err) = real_func() {
        use std::io::Write;

        let mut stderr = std::io::stderr();
        writeln!(stderr, "Error: {}", err)
            .expect("unable to write error to stderr");
        for cause in err.iter_causes() {
            writeln!(stderr, "Caused by: {}", cause)
                .expect("unable to write error to stderr");
        }
        std::process::exit(1);
    }
}
