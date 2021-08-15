#[macro_use]
extern crate log;

use chrono::{DateTime, Utc};

use basic_frame::{BasicExtra, BasicFrame};
use ci2_remote_control::MkvRecordingConfig;

use machine_vision_formats::{pixel_format::RGB8, ImageData};
use rusttype::{point, Font, Scale};

struct Rgba(pub [u8; 4]);

fn put_pixel(self_: &mut BasicFrame<RGB8>, x: u32, y: u32, incoming: Rgba) {
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

fn stamp_frame<'a>(
    rgb: &simple_frame::SimpleFrame<RGB8>,
    font: &rusttype::Font<'a>,
    count: usize,
    start: &DateTime<Utc>,
) -> Result<(basic_frame::BasicFrame<RGB8>, DateTime<Utc>), anyhow::Error> {
    let dt_msec = 5;
    let dt = chrono::Duration::milliseconds(count as i64 * dt_msec);

    let ts = start.checked_add_signed(dt).unwrap();

    let width = rgb.width();
    let height = rgb.height();
    let image_data = rgb.image_data().to_vec(); // copy data

    let extra = Box::new(BasicExtra {
        host_timestamp: ts,
        host_framenumber: count,
    });
    let mut image: BasicFrame<RGB8> = BasicFrame {
        width: width as u32,
        height: height as u32,
        stride: (width * 3) as u32,
        image_data,
        extra,
        pixel_format: std::marker::PhantomData,
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

fn usage_exit() -> Result<(), anyhow::Error> {
    println!(
        "Usage:

    save-animation nv-h264|vp8"
    );
    Err(anyhow::format_err!("invalid usage"))
}

fn main() -> Result<(), anyhow::Error> {
    let start = Utc::now();
    let output_fname = "animation.mkv";

    info!("exporting {}", output_fname);

    let out_fd = std::fs::File::create(&output_fname)?;

    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        usage_exit()?;
    }

    #[allow(unused_assignments)]
    let mut nvenc_libs = None;

    let (codec, libs_and_nv_enc) = match args[1].as_str() {
        "nv-h264" => {
            nvenc_libs = Some(nvenc::Dynlibs::new()?);
            let codec =
                ci2_remote_control::MkvCodec::H264(ci2_remote_control::H264Options::default());
            (
                codec,
                Some(nvenc::NvEnc::new(&nvenc_libs.as_ref().unwrap())?),
            )
        }
        "vp8" => {
            let mut opts = ci2_remote_control::VP8Options::default();
            opts.bitrate = 1000;
            let codec = ci2_remote_control::MkvCodec::VP8(opts);
            (codec, None)
        }
        _ => {
            return usage_exit();
        }
    };

    let cfg = MkvRecordingConfig {
        codec,
        max_framerate: ci2_remote_control::RecordingFrameRate::Unlimited,
        writing_application: None,
    };

    let mut my_mkv_writer = mkv_writer::MkvWriter::new(out_fd, cfg, libs_and_nv_enc)?;

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
        my_mkv_writer.write(&frame, ts)?;
    }

    my_mkv_writer.finish()?;
    Ok(())
}
