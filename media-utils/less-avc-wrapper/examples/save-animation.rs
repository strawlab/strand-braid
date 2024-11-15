// Copyright 2022-2023 Andrew D. Straw.
#[macro_use]
extern crate log;

use machine_vision_formats::{
    image_ref::ImageRef,
    owned::OImage,
    pixel_format::{Mono8, RGB8},
    ImageMutStride, Stride,
};

use font_drawing::stamp_frame;

use rusttype::Font;

fn main() -> eyre::Result<()> {
    env_logger::init();

    let image = image::load_from_memory(&include_bytes!("bee.jpg")[..])?;
    let rgb = convert_image::image_to_rgb8(image)?;
    let rgb = OImage::from_owned(rgb);

    for width_pad in [0, 2] {
        for height_pad in [0, 2] {
            for format_str in &["mono8", "rgb8"] {
                use machine_vision_formats::ImageData;

                let final_width = rgb.width() - width_pad;
                let final_height = rgb.height() - height_pad;
                let output_fname = format!("bee-{final_width}x{final_height}-{format_str}.h264");

                info!("exporting {}", output_fname);

                let out_fd = std::fs::File::create(&output_fname)?;

                let mut my_h264_writer = less_avc_wrapper::H264WriterWrapper::new(out_fd)?;

                // Load the font
                // let font_data = include_bytes!("../Roboto-Regular.ttf");
                let font_data = ttf_firacode::REGULAR;
                // This only succeeds if collection consists of one font
                let font =
                    Font::try_from_bytes(font_data as &[u8]).expect("Error constructing Font");

                let mut count = 0;
                let mut istart = std::time::Instant::now();
                let mut png_buf = None;

                loop {
                    if count % 100 == 0 {
                        let el = istart.elapsed();
                        let elf = el.as_secs() as f64 + 1e-9 * el.subsec_nanos() as f64;
                        println!("frame {}, duration {} msec", count, elf * 1000.0,);
                        istart = std::time::Instant::now();
                    }
                    if count > 10 {
                        break;
                    }

                    // The text to render
                    let text = format!("{}", count);
                    let mut frame = rgb.clone();

                    stamp_frame(&mut frame as &mut dyn ImageMutStride<_>, &font, &text)?;
                    count += 1;

                    let opts = convert_image::EncoderOptions::Png;

                    match *format_str {
                        "mono8" => {
                            // if png_buf.is_none() {
                            //     png_buf = Some(convert_image::frame_to_encoded_buffer(&frame, opts)?);
                            // }
                            // convert to farget format, keeping full size
                            let mono = convert_image::convert_ref::<_, Mono8>(&frame)?;
                            // if png_buf.is_none() {
                            //     png_buf = Some(convert_image::frame_to_encoded_buffer(&mono, opts)?);
                            // }

                            let out_size_bytes = mono.stride() * final_height as usize;
                            let trimmed = OImage::<Mono8>::new(
                                final_width,
                                final_height,
                                mono.stride().try_into().unwrap(),
                                mono.image_data()[..out_size_bytes].to_vec(),
                            )
                            .unwrap();
                            if png_buf.is_none() {
                                png_buf =
                                    Some(convert_image::frame_to_encoded_buffer(&trimmed, opts)?);
                            }

                            my_h264_writer.write(&trimmed)?;
                        }
                        "rgb8" => {
                            let out_size_bytes = frame.stride() * final_height as usize;
                            let trimmed = ImageRef::<RGB8>::new(
                                final_width,
                                final_height,
                                frame.stride(),
                                &frame.image_data()[..out_size_bytes],
                            )
                            .unwrap();
                            if png_buf.is_none() {
                                png_buf =
                                    Some(convert_image::frame_to_encoded_buffer(&trimmed, opts)?);
                            }

                            my_h264_writer.write(&trimmed)?;
                        }
                        _ => {
                            panic!("unknown format");
                        }
                    }
                }

                if let Some(png_buf) = png_buf {
                    // Save .png to verify input image is OK.
                    let mut png_fname = std::path::PathBuf::from(&output_fname);
                    png_fname.set_extension("png");

                    let mut fd = std::fs::File::create(png_fname)?;
                    use std::io::Write;
                    fd.write_all(&png_buf)?;
                }
            }
        }
    }

    Ok(())
}
