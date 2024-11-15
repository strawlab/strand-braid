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
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "info");
    }
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
                // let output_fname = format!("bee-{final_width}x{final_height}-{format_str}.y4m");
                let output_fname = format!("bee-{final_width}x{final_height}-{format_str}.mp4");

                let encoder_opts = Some(ffmpeg_writer::platform_hardware_encoder()?);
                // let encoder_opts = Some(ffmpeg_writer::FfmpegEncoderOptions::Y4mNoFfmpeg);

                info!("exporting {output_fname} with {encoder_opts:?}");

                let mut my_ffmpeg_writer =
                    ffmpeg_writer::FfmpegWriter::new(&output_fname, encoder_opts, None)?;

                // Load the font
                // let font_data = include_bytes!("../Roboto-Regular.ttf");
                let font_data = ttf_firacode::REGULAR;
                // This only succeeds if collection consists of one font
                let font =
                    Font::try_from_bytes(font_data as &[u8]).expect("Error constructing Font");

                let mut count = 0;
                let mut istart = std::time::Instant::now();

                loop {
                    if count % 100 == 0 {
                        let el = istart.elapsed();
                        let elf = el.as_secs() as f64 + 1e-9 * el.subsec_nanos() as f64;
                        info!("frame {}, duration {} msec", count, elf * 1000.0,);
                        istart = std::time::Instant::now();
                    }
                    if count > 1000 {
                        break;
                    }

                    // The text to render
                    let text = format!("{}", count);
                    let mut frame = rgb.clone();

                    stamp_frame(&mut frame as &mut dyn ImageMutStride<_>, &font, &text)?;
                    count += 1;

                    match *format_str {
                        "mono8" => {
                            // convert to farget format, keeping full size
                            let mono = convert_image::convert_ref::<_, Mono8>(&frame)?;

                            let out_size_bytes = mono.stride() * final_height as usize;
                            let trimmed = OImage::<Mono8>::new(
                                final_width,
                                final_height,
                                mono.stride().try_into().unwrap(),
                                mono.image_data()[..out_size_bytes].to_vec(),
                            )
                            .unwrap();

                            my_ffmpeg_writer
                                .write_frame(&trimmed)
                                .map_err(better_error)?;
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

                            my_ffmpeg_writer
                                .write_frame(&trimmed)
                                .map_err(better_error)?;
                        }
                        _ => {
                            panic!("unknown format");
                        }
                    }
                }

                my_ffmpeg_writer.close()?;
            }
        }
    }

    Ok(())
}

fn better_error(e: ffmpeg_writer::Error) -> eyre::Report {
    match e {
        ffmpeg_writer::Error::FfmpegError { output } => {
            eyre::eyre!(
                "ffmpeg writer error. Exit code {}.\nStdout:\n{}\nStderror:\n{}\n",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        }
        e => e.into(),
    }
}
