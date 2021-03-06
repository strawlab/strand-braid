#[macro_use]
extern crate log;

use chrono::{DateTime, Utc};

use basic_frame::BasicFrame;
use ci2_remote_control::MkvRecordingConfig;

use machine_vision_formats::ImageData;
use rusttype::{point, Font, Scale};

struct Rgba(pub [u8; 4]);

fn put_pixel(self_: &mut BasicFrame, x: u32, y: u32, incoming: Rgba) {
    match self_.pixel_format {
        machine_vision_formats::PixelFormat::RGB8 => {
            let row_start = self_.stride as usize * y as usize;
            let pix_start = row_start + x as usize * 3;

            let alpha = incoming.0[3] as f64 / 255.0;
            let p = 1.0 - alpha;
            let q = alpha;

            use std::convert::TryInto;
            let old: [u8; 3] = self_.image_data[pix_start..pix_start + 3]
                .try_into()
                .unwrap();
            let new: [u8; 3] = [
                (old[0] as f64 * p + incoming.0[0] as f64 * q).round() as u8,
                (old[1] as f64 * p + incoming.0[1] as f64 * q).round() as u8,
                (old[2] as f64 * p + incoming.0[2] as f64 * q).round() as u8,
            ];

            self_.image_data[pix_start] = new[0];
            self_.image_data[pix_start + 1] = new[1];
            self_.image_data[pix_start + 2] = new[2];
        }
        _ => {
            panic!("unsupported image format: {}", self_.pixel_format);
        }
    }
}

fn stamp_frame<'a>(
    rgb: &convert_image::ConvertImageFrame,
    font: &rusttype::Font<'a>,
    count: usize,
    start: &DateTime<Utc>,
) -> Result<(basic_frame::BasicFrame, DateTime<Utc>), failure::Error> {
    let dt_msec = 5;
    let dt = chrono::Duration::milliseconds(count as i64 * dt_msec);

    let ts = start.checked_add_signed(dt).unwrap();

    let width = rgb.width();
    let height = rgb.height();
    let image_data = rgb.image_data().to_vec(); // copy data

    let mut image = BasicFrame {
        width: width as u32,
        height: height as u32,
        stride: (width * 3) as u32,
        image_data,
        host_timestamp: ts,
        host_framenumber: count,
        pixel_format: machine_vision_formats::PixelFormat::RGB8,
    };

    // from https://gitlab.redox-os.org/redox-os/rusttype/blob/master/dev/examples/image.rs

    // The font size to use
    let scale = Scale::uniform(32.0);

    // The text to render
    let text = format!("{}", ts);

    // Use a dark red colour
    let colour = (150, 0, 0);

    let v_metrics = font.v_metrics(scale);

    // layout the glyphs in a line with 20 pixels padding
    let glyphs: Vec<_> = font
        .layout(&text, scale, point(20.0, 20.0 + v_metrics.ascent))
        .collect();

    for glyph in glyphs {
        if let Some(bounding_box) = glyph.pixel_bounding_box() {
            // Draw the glyph into the image per-pixel by using the draw closure
            glyph.draw(|x, y, v| {
                put_pixel(
                    &mut image,
                    // Offset the position by the glyph bounding box
                    x + bounding_box.min.x as u32,
                    y + bounding_box.min.y as u32,
                    // Turn the coverage into an alpha value
                    Rgba([colour.0, colour.1, colour.2, (v * 255.0) as u8]),
                )
            });
        }
    }

    Ok((image, ts))
}

fn main() -> Result<(), failure::Error> {
    let start = Utc::now();
    let output_fname = "animation.mkv";

    info!("exporting {}", output_fname);

    let out_fd = std::fs::File::create(&output_fname)?;

    #[cfg(feature = "example-nv-h264")]
    let libs = nvenc::Dynlibs::new()?;

    #[cfg(feature = "example-nv-h264")]
    let (codec, libs_and_nv_enc) = {
        let codec = ci2_remote_control::MkvCodec::H264(ci2_remote_control::H264Options::default());
        (codec, Some(nvenc::NvEnc::new(&libs)?))
    };

    #[cfg(feature = "example-vp8")]
    let (codec, libs_and_nv_enc) = {
        let mut opts = ci2_remote_control::VP8Options::default();
        opts.bitrate = 1000;
        let codec = ci2_remote_control::MkvCodec::VP8(opts);
        (codec, None)
    };

    let cfg = MkvRecordingConfig {
        codec,
        max_framerate: ci2_remote_control::RecordingFrameRate::Unlimited,
    };

    let mut mkv_writer = webm_writer::WebmWriter::new(out_fd, cfg, libs_and_nv_enc)?;

    let image = image::load_from_memory(&include_bytes!("bee.jpg")[..])?;
    let rgb = convert_image::piston_to_frame(image)?;

    // Load the font
    // let font_data = include_bytes!("../Roboto-Regular.ttf");
    let font_data = ttf_firacode::REGULAR;
    // This only succeeds if collection consists of one font
    let font = Font::from_bytes(font_data as &[u8]).expect("Error constructing Font");

    let mut count = 0;
    let mut istart = std::time::Instant::now();
    loop {
        if count % 100 == 0 {
            let el = istart.elapsed();
            let elf = el.as_secs() as f64 + 1e-9 * el.subsec_nanos() as f64;
            println!("frame {}, duration {} msec", count, elf * 1000.0);
            istart = std::time::Instant::now();
        }
        if count > 1000 {
            break;
        }
        let (frame, ts) = stamp_frame(&rgb, &font, count, &start)?;
        count += 1;
        mkv_writer.write(&frame, ts)?;
    }

    mkv_writer.finish()?;
    Ok(())
}
