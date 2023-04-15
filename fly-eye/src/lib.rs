#[macro_use]
extern crate log;

use machine_vision_formats as formats;
use machine_vision_shaders as shaders;

use basic_frame::DynamicFrame;
use formats::Stride;

#[macro_use]
extern crate glium;

#[cfg(feature = "fly-eye")]
mod fly_eye;
#[cfg(feature = "screen-quad")]
mod screen_quad;

use channellib::Receiver;
use glium::{glutin, Surface};

use glutin::window::WindowBuilder;

#[cfg(feature = "fly-eye")]
use fly_eye as coords;
#[cfg(feature = "screen-quad")]
use screen_quad as coords;

struct Inner {
    opengl_texture: glium::texture::Texture2d,
    p_buffer: glium::texture::pixel_buffer::PixelBuffer<u8>,
    program: glium::Program,
    uniform_type: shaders::UniformType,
}

pub fn mainloop(rx: Receiver<DynamicFrame>) -> anyhow::Result<()> {
    let event_loop = glutin::event_loop::EventLoop::new();
    let window = WindowBuilder::new().with_title("Fly Eye");
    let context = glutin::ContextBuilder::new().with_vsync(true);
    let display = glium::Display::new(window, context, &event_loop).expect("open display");

    let vertex_buffer = glium::VertexBuffer::immutable(&display, &coords::VERTEX_DATA).unwrap();
    let indices = glium::IndexBuffer::immutable(
        &display,
        glium::index::PrimitiveType::TrianglesList,
        &coords::INDEX_DATA,
    )
    .unwrap();

    let mut inner = None;

    event_loop.run(move |event, _, control_flow| {
        let result_frame = match inner {
            Some(_) => get_most_recent_frame(&rx), // normal case, get frame if available
            None => Ok(rx.recv().unwrap()),        // ensure we have first frame
        };

        if let Ok(frame) = result_frame {
            let width = frame.width();
            let height = frame.height();
            let stride = frame.stride();
            let pixel_format = frame.pixel_format();
            let imdata: Vec<u8> = frame.into();

            if inner.is_none() {
                // perform initial allocations

                let (uni_ty, vert_src, frag_src, ifmt) =
                    shaders::get_programs(width, height, pixel_format);

                debug!("using internal format {:?}", ifmt);

                let format = match ifmt {
                    shaders::InternalFormat::Rgb8 => {
                        glium::texture::UncompressedFloatFormat::U8U8U8U8
                    }
                    shaders::InternalFormat::Raw8 => glium::texture::UncompressedFloatFormat::U8,
                };

                let opengl_texture = match pixel_format {
                    formats::pixel_format::PixFmt::RGB8 => {
                        let texdata = glium::texture::RawImage2d::from_raw_rgb(
                            imdata.clone(),
                            (width, height),
                        );
                        glium::Texture2d::new(&display, texdata).unwrap()
                    }
                    _ => glium::Texture2d::empty_with_format(
                        &display,
                        format,
                        glium::texture::MipmapsOption::NoMipmap,
                        width,
                        height,
                    )
                    .unwrap(),
                };

                let n_pixels = stride as u32 * height; // make stride width for easy copy
                let p_buffer = glium::texture::pixel_buffer::PixelBuffer::new_empty(
                    &display,
                    n_pixels as usize,
                );

                let program =
                    glium::Program::from_source(&display, vert_src, frag_src, None).unwrap();
                let uniform_type = uni_ty;
                inner = Some(Inner {
                    program,
                    opengl_texture,
                    p_buffer,
                    uniform_type,
                })
            }

            if let Some(ref inner) = inner {
                if pixel_format == formats::pixel_format::PixFmt::RGB8 {
                    unimplemented!("RGB data not coverted to pbuffer");
                }
                inner.p_buffer.write(&imdata);
                inner
                    .opengl_texture
                    .main_level()
                    .raw_upload_from_pixel_buffer(
                        inner.p_buffer.as_slice(),
                        0..width,
                        0..height,
                        0..1,
                    );
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
                    target
                        .draw(
                            &vertex_buffer,
                            &indices,
                            &inner.program,
                            &uniforms,
                            &Default::default(),
                        )
                        .unwrap();
                }
                shaders::UniformType::Bayer(ref di) => {
                    let uniforms = uniform! {
                        source: &inner.opengl_texture,
                        sourceSize: di.source_size,
                        firstRed: di.first_red,
                    };
                    target
                        .draw(
                            &vertex_buffer,
                            &indices,
                            &inner.program,
                            &uniforms,
                            &Default::default(),
                        )
                        .unwrap();
                }
            }
        } else {
            target.finish().unwrap();
            panic!("inner is None");
        }

        target.finish().unwrap();

        match event {
            winit::event::Event::WindowEvent { event, .. } => match event {
                winit::event::WindowEvent::CloseRequested => {
                    *control_flow = winit::event_loop::ControlFlow::Exit
                }
                winit::event::WindowEvent::KeyboardInput { input, .. }
                    if winit::event::ElementState::Pressed == input.state =>
                {
                    *control_flow = winit::event_loop::ControlFlow::Exit
                }
                _ => (),
            },
            _ => (),
        };
    });
}

/// check if a frame is available. if yes, get it and keep getting until most recent.
fn get_most_recent_frame(
    receiver: &Receiver<DynamicFrame>,
) -> Result<DynamicFrame, channellib::TryRecvError> {
    let mut result = Err(crossbeam_channel::TryRecvError::Empty.into());
    loop {
        match receiver.try_recv() {
            Ok(r) => result = Ok(r),
            Err(e) => {
                if e.is_empty() {
                    break;
                } else {
                    return Err(e);
                }
            }
        }
    }
    result
}
