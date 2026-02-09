use rusttype::{point, Scale};
use machine_vision_formats::{pixel_format, ImageMutStride};
use eyre::Result;

struct Rgba(pub [u8; 4]);

fn put_pixel(image: &mut dyn ImageMutStride<pixel_format::RGB8>, x: u32, y: u32, incoming: Rgba) {
    let row_start = image.stride() * y as usize;
    let pix_start = row_start + x as usize * 3;

    let alpha = incoming.0[3] as f64 / 255.0;
    let p = 1.0 - alpha;
    let q = alpha;

    let old: [u8; 3] = image.image_data()[pix_start..pix_start + 3]
        .try_into()
        .unwrap();
    let new: [u8; 3] = [
        (old[0] as f64 * p + incoming.0[0] as f64 * q).round() as u8,
        (old[1] as f64 * p + incoming.0[1] as f64 * q).round() as u8,
        (old[2] as f64 * p + incoming.0[2] as f64 * q).round() as u8,
    ];

    image.buffer_mut_ref().data[pix_start] = new[0];
    image.buffer_mut_ref().data[pix_start + 1] = new[1];
    image.buffer_mut_ref().data[pix_start + 2] = new[2];
}

pub fn stamp_frame<'a>(image: &mut dyn ImageMutStride<pixel_format::RGB8>, font: &rusttype::Font<'a>, text: &str) -> Result<()> {
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
        .map(|g| g.position().x + g.unpositioned().h_metrics().advance_width)
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
