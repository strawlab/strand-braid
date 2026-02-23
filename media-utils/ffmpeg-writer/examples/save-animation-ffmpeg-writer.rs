// Copyright 2022-2023 Andrew D. Straw.
use font_drawing::stamp_frame;
use machine_vision_formats::{
    image_ref::ImageRef,
    owned::OImage,
    pixel_format::{Mono8, RGB8},
    ImageMutStride, Stride,
};
use tracing::info;

use rusttype::Font;

fn main() -> eyre::Result<()> {
    if std::env::var_os("RUST_LOG").is_none() {
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("RUST_LOG", "info") };
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
                let output_fname = format!("bee-{final_width}x{final_height}-{format_str}.mp4");

                let ffmpeg_codec_args = ffmpeg_writer::platform_hardware_encoder()?;

                info!("exporting {output_fname} with {ffmpeg_codec_args:?}");

                let mut my_ffmpeg_writer =
                    ffmpeg_writer::FfmpegWriter::new(&output_fname, ffmpeg_codec_args, None)?;

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
                            // convert to target format, keeping full size
                            let mono = convert_image::convert_ref::<_, Mono8>(&frame)?;

                            let trimmed = strand_dynamic_frame::DynamicFrameOwned::from_static(
                                OImage::<Mono8>::new(
                                    final_width,
                                    final_height,
                                    mono.stride(),
                                    mono.image_data().to_vec(),
                                )
                                .unwrap(),
                            );

                            my_ffmpeg_writer
                                .write_dynamic_frame(&trimmed.borrow())
                                .map_err(better_error)?;
                        }
                        "rgb8" => {
                            let trimmed = ImageRef::<RGB8>::new(
                                final_width,
                                final_height,
                                frame.stride(),
                                frame.image_data(),
                            )
                            .unwrap();
                            let dy_trimmed =
                                strand_dynamic_frame::DynamicFrame::from_static_ref(&trimmed);

                            my_ffmpeg_writer
                                .write_dynamic_frame(&dy_trimmed)
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
