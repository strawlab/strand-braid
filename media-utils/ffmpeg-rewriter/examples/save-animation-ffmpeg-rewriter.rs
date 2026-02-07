// Copyright 2022-2024 Andrew D. Straw.
use font_drawing::stamp_frame;
use machine_vision_formats::{
    image_ref::ImageRef,
    owned::OImage,
    pixel_format::{Mono8, RGB8},
    ImageMutStride, Stride,
};

use rusttype::Font;

fn main() -> eyre::Result<()> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "info");
    }
    env_tracing_logger::init();

    // 1970-01-01 01:00:00 UTC
    let start = chrono::DateTime::from_timestamp(60 * 60, 0).unwrap();

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

                let ffmpeg_codec_args = ffmpeg_writer::platform_hardware_encoder()?;

                let rate = None;
                let h264_metadata = Some(strand_cam_remote_control::H264Metadata::new(
                    "ffmpeg-rewriter/save-animation-ffmpeg-rewriter",
                    start.into(),
                ));

                let dt_msec = 5;
                let sample_duration = chrono::Duration::try_milliseconds(dt_msec).unwrap();

                let mut my_ffmpeg_writer = ffmpeg_rewriter::FfmpegReWriter::new(
                    &output_fname,
                    ffmpeg_codec_args,
                    rate,
                    h264_metadata,
                )?;

                // Load the font
                // let font_data = include_bytes!("../Roboto-Regular.ttf");
                let font_data = ttf_firacode::REGULAR;
                // This only succeeds if collection consists of one font
                let font =
                    Font::try_from_bytes(font_data as &[u8]).expect("Error constructing Font");

                let mut count = 0;
                let mut istart = std::time::Instant::now();

                loop {
                    let ts = start.checked_add_signed(sample_duration * count).unwrap();

                    if count % 100 == 0 {
                        let el = istart.elapsed();
                        let elf = el.as_secs() as f64 + 1e-9 * el.subsec_nanos() as f64;
                        println!(
                            "frame {}, duration {} msec, timestamp {}",
                            count,
                            elf * 1000.0,
                            ts
                        );
                        istart = std::time::Instant::now();
                    }
                    if count > 1000 {
                        break;
                    }

                    // The text to render
                    let text = format!("{}", ts);
                    let mut frame = rgb.clone();

                    stamp_frame(&mut frame as &mut dyn ImageMutStride<_>, &font, &text)?;
                    count += 1;

                    match *format_str {
                        "mono8" => {
                            // convert to farget format, keeping full size
                            let mono = convert_image::convert_ref::<_, Mono8>(&frame)?;

                            let out_size_bytes = mono.stride() * final_height as usize;
                            let trimmed = strand_dynamic_frame::DynamicFrameOwned::from_static(
                                OImage::<Mono8>::new(
                                    final_width,
                                    final_height,
                                    mono.stride(),
                                    mono.image_data()[..out_size_bytes].to_vec(),
                                )
                                .unwrap(),
                            );

                            my_ffmpeg_writer.write_dynamic_frame(&trimmed.borrow(), ts)?;
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
                            let dy_trimmed =
                                strand_dynamic_frame::DynamicFrame::from_static_ref(&trimmed);

                            my_ffmpeg_writer.write_dynamic_frame(&dy_trimmed, ts)?;
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
