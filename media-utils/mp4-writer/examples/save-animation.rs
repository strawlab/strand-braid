// Copyright 2022-2023 Andrew D. Straw.

use clap::{Parser, ValueEnum};
use machine_vision_formats::{
    image_ref::ImageRef,
    pixel_format::{Mono8, RGB8},
    Stride,
};
use tracing::info;

use ci2_remote_control::Mp4RecordingConfig;

use rusttype::Font;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Sets the encoder
    #[arg(value_enum)]
    encoder: Encoder,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
enum Encoder {
    LessAvc,
    OpenH264,
    NvEnc,
}

fn main() -> eyre::Result<()> {
    env_tracing_logger::init();
    let cli = Cli::parse();

    // 1970-01-01 01:00:00 UTC
    let start = chrono::DateTime::from_timestamp(60 * 60, 0).unwrap();

    let image = image::load_from_memory(&include_bytes!("bee.jpg")[..])?;
    let rgb = convert_image::image_to_rgb8(image)?;
    let rgb = machine_vision_formats::owned::OImage::from_owned(rgb);

    for width_pad in [0, 2] {
        for height_pad in [0, 2] {
            for format_str in &["mono8", "rgb8"] {
                use machine_vision_formats::ImageData;

                let final_width = rgb.width() - width_pad;
                let final_height = rgb.height() - height_pad;
                let output_fname = format!("bee-{final_width}x{final_height}-{format_str}.mp4");

                info!("exporting {}", output_fname);

                let out_fd = std::fs::File::create(&output_fname)?;

                #[cfg(feature = "nv-encode")]
                #[allow(unused_assignments)]
                let mut nvenc_libs = None;

                let h264_bitrate = None;

                #[allow(unused_variables)]
                let (codec, libs_and_nv_enc) = match cli.encoder {
                    Encoder::OpenH264 => {
                        let codec = ci2_remote_control::Mp4Codec::H264OpenH264({
                            let preset = if let Some(bitrate) = h264_bitrate {
                                ci2_remote_control::OpenH264Preset::SkipFramesBitrate(bitrate)
                            } else {
                                ci2_remote_control::OpenH264Preset::AllFrames
                            };
                            ci2_remote_control::OpenH264Options {
                                preset,
                                debug: false,
                            }
                        });
                        #[cfg(not(feature = "nv-encode"))]
                        let none = Option::<()>::None;
                        #[cfg(feature = "nv-encode")]
                        let none = None;
                        (codec, none)
                    }
                    #[cfg(feature = "nv-encode")]
                    Encoder::NvEnc => {
                        nvenc_libs = Some(nvenc::Dynlibs::new()?);
                        let codec = ci2_remote_control::Mp4Codec::H264NvEnc(Default::default());
                        (
                            codec,
                            Some(nvenc::NvEnc::new(nvenc_libs.as_ref().unwrap())?),
                        )
                    }
                    #[cfg(not(feature = "nv-encode"))]
                    Encoder::NvEnc => {
                        panic!("NvEnc support not compiled");
                    }
                    Encoder::LessAvc => (ci2_remote_control::Mp4Codec::H264LessAvc, None),
                };

                let dt_msec = 5;
                let sample_duration = chrono::Duration::try_milliseconds(dt_msec).unwrap();

                let cfg = Mp4RecordingConfig {
                    codec,
                    max_framerate: Default::default(),
                    h264_metadata: None,
                };

                #[cfg(feature = "nv-encode")]
                let mut my_mp4_writer = mp4_writer::Mp4Writer::new(out_fd, cfg, libs_and_nv_enc)?;
                #[cfg(not(feature = "nv-encode"))]
                let mut my_mp4_writer = mp4_writer::Mp4Writer::new(out_fd, cfg)?;

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

                    font_drawing::stamp_frame(&mut frame, &font, &text)?;
                    count += 1;

                    match *format_str {
                        "mono8" => {
                            let mono = convert_image::convert_ref::<_, Mono8>(&frame)?;

                            let out_size_bytes = mono.stride() * final_height as usize;
                            let trimmed = ImageRef::<Mono8>::new(
                                final_width,
                                final_height,
                                mono.stride(),
                                &mono.image_data()[..out_size_bytes],
                            )
                            .unwrap();

                            my_mp4_writer.write(&trimmed, ts)?;
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

                            my_mp4_writer.write(&trimmed, ts)?;
                        }
                        _ => {
                            panic!("unknown format");
                        }
                    }
                }

                my_mp4_writer.finish()?;
            }
        }
    }

    Ok(())
}
