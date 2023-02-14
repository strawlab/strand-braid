// Copyright 2022-2023 Andrew D. Straw.

#[macro_use]
extern crate log;

use chrono::Utc;
use clap::{Parser, ValueEnum};

use machine_vision_formats::{
    pixel_format::{Mono8, RGB8},
    Stride,
};

use ci2_remote_control::Mp4RecordingConfig;
use simple_frame::SimpleFrame;

use rusttype::{point, Font, Scale};

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

struct Rgba(pub [u8; 4]);

fn put_pixel(self_: &mut SimpleFrame<RGB8>, x: u32, y: u32, incoming: Rgba) {
    use machine_vision_formats::{ImageData, ImageMutData};

    let row_start = self_.stride as usize * y as usize;
    let pix_start = row_start + x as usize * 3;

    let alpha = incoming.0[3] as f64 / 255.0;
    let p = 1.0 - alpha;
    let q = alpha;

    let old: [u8; 3] = self_.image_data()[pix_start..pix_start + 3]
        .try_into()
        .unwrap();
    let new: [u8; 3] = [
        (old[0] as f64 * p + incoming.0[0] as f64 * q).round() as u8,
        (old[1] as f64 * p + incoming.0[1] as f64 * q).round() as u8,
        (old[2] as f64 * p + incoming.0[2] as f64 * q).round() as u8,
    ];

    self_.buffer_mut_ref().data[pix_start] = new[0];
    self_.buffer_mut_ref().data[pix_start + 1] = new[1];
    self_.buffer_mut_ref().data[pix_start + 2] = new[2];
}

fn stamp_frame<'a>(
    image: &mut SimpleFrame<RGB8>,
    font: &rusttype::Font<'a>,
    text: &str,
) -> Result<(), anyhow::Error> {
    // from https://gitlab.redox-os.org/redox-os/rusttype/blob/master/dev/examples/image.rs

    // The font size to use
    let scale = Scale::uniform(32.0);

    // Use a dark red colour
    let colour = (150, 0, 0);

    let v_metrics = font.v_metrics(scale);

    let x0 = 20.0;
    let y0 = 20.0;

    // layout the glyphs in a line with 20 pixels padding
    let glyphs: Vec<_> = font
        .layout(text, scale, point(x0, y0 + v_metrics.ascent))
        .collect();

    // Find the most visually pleasing width to display
    let width = glyphs
        .iter()
        .rev()
        .map(|g| g.position().x as f32 + g.unpositioned().h_metrics().advance_width)
        .next()
        .unwrap_or(0.0)
        .ceil() as usize;

    let x_start = x0.floor() as usize;
    let x_end = x_start + width;

    let y_start = y0.floor() as usize;
    let y_end = y_start + v_metrics.ascent.ceil() as usize;

    for x in x_start..x_end {
        for y in y_start..y_end {
            put_pixel(
                image,
                // Offset the position by the glyph bounding box
                x as u32,
                y as u32,
                // Turn the coverage into an alpha value
                Rgba([255, 255, 255, 255]),
            )
        }
    }

    // TODO: clear background

    for glyph in glyphs {
        if let Some(bounding_box) = glyph.pixel_bounding_box() {
            // Draw the glyph into the image per-pixel by using the draw closure
            glyph.draw(|x, y, v| {
                put_pixel(
                    image,
                    // Offset the position by the glyph bounding box
                    x + bounding_box.min.x as u32,
                    y + bounding_box.min.y as u32,
                    // Turn the coverage into an alpha value
                    Rgba([colour.0, colour.1, colour.2, (v * 255.0) as u8]),
                )
            });
        }
    }

    Ok(())
}

fn main() -> Result<(), anyhow::Error> {
    env_logger::init();
    let cli = Cli::parse();

    // let start = Utc::now();
    let start = chrono::DateTime::from_utc(
        chrono::NaiveDateTime::from_timestamp_opt(1 * 60 * 60, 0).unwrap(),
        Utc,
    );

    let image = image::load_from_memory(&include_bytes!("bee.jpg")[..])?;
    let rgb = convert_image::piston_to_frame(image)?;

    for width_pad in [0, 2] {
        for height_pad in [0, 2] {
            for format_str in &["mono8", "rgb8"] {
                use machine_vision_formats::ImageData;

                let final_width = rgb.width() - width_pad;
                let final_height = rgb.height() - height_pad;
                let output_fname = format!("bee-{final_width}x{final_height}-{format_str}.mp4");

                info!("exporting {}", output_fname);

                let out_fd = std::fs::File::create(&output_fname)?;

                #[allow(unused_assignments)]
                let mut nvenc_libs = None;

                let h264_bitrate = None;

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
                        (codec, None)
                    }
                    Encoder::NvEnc => {
                        nvenc_libs = Some(nvenc::Dynlibs::new()?);
                        let codec = ci2_remote_control::Mp4Codec::H264NvEnc(Default::default());
                        (
                            codec,
                            Some(nvenc::NvEnc::new(nvenc_libs.as_ref().unwrap())?),
                        )
                    }
                    Encoder::LessAvc => (ci2_remote_control::Mp4Codec::H264LessAvc, None),
                };

                let dt_msec = 5;
                let sample_duration = chrono::Duration::milliseconds(dt_msec);

                let cfg = Mp4RecordingConfig {
                    codec,
                    max_framerate: Default::default(),
                    sample_duration: sample_duration.to_std().unwrap(),
                    h264_metadata: None,
                };

                let mut my_mp4_writer = mp4_writer::Mp4Writer::new(out_fd, cfg, libs_and_nv_enc)?;

                // Load the font
                // let font_data = include_bytes!("../Roboto-Regular.ttf");
                let font_data = ttf_firacode::REGULAR;
                // This only succeeds if collection consists of one font
                let font = Font::from_bytes(font_data as &[u8]).expect("Error constructing Font");

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

                    stamp_frame(&mut frame, &font, &text)?;
                    count += 1;

                    match *format_str {
                        "mono8" => {
                            let mono = convert_image::convert::<_, Mono8>(&frame)?;

                            let out_size_bytes = mono.stride() * final_height as usize;
                            let trimmed = SimpleFrame::<Mono8>::new(
                                final_width,
                                final_height,
                                mono.stride().try_into().unwrap(),
                                mono.image_data()[..out_size_bytes].to_vec(),
                            )
                            .unwrap();

                            my_mp4_writer.write(&trimmed, ts)?;
                        }
                        "rgb8" => {
                            let out_size_bytes = frame.stride() * final_height as usize;
                            let trimmed = SimpleFrame::<RGB8>::new(
                                final_width,
                                final_height,
                                frame.stride().try_into().unwrap(),
                                frame.image_data()[..out_size_bytes].to_vec(),
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
