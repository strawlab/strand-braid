use eyre::{self as anyhow};

use machine_vision_formats::{pixel_format::RGBA8, ImageBuffer, ImageBufferRef, ImageData, Stride};

pub(crate) struct Frame {
    pixmap: resvg::tiny_skia::Pixmap,
}

impl Frame {
    pub(crate) fn new(mut pixmap: resvg::tiny_skia::Pixmap) -> anyhow::Result<Self> {
        // This pixel conversion is based on that of
        // tiny_skia::Pixmap::encode_png
        for pixel in pixmap.pixels_mut() {
            let c = pixel.demultiply();
            *pixel = resvg::tiny_skia::PremultipliedColorU8::from_rgba(
                c.red(),
                c.green(),
                c.blue(),
                c.alpha(),
            )
            .ok_or_else(|| {
                anyhow::anyhow!("Could not demultiply pixmap. (Hint: draw a background color.)")
            })?;
        }

        Ok(Self { pixmap })
    }
}

impl ImageData<RGBA8> for Frame {
    fn width(&self) -> u32 {
        self.pixmap.width()
    }
    fn height(&self) -> u32 {
        self.pixmap.height()
    }
    fn buffer_ref(&self) -> ImageBufferRef<'_, RGBA8> {
        ImageBufferRef {
            pixel_format: std::marker::PhantomData,
            data: self.pixmap.as_ref().data(),
        }
    }
    fn buffer(self) -> ImageBuffer<RGBA8> {
        self.buffer_ref().to_buffer()
    }
}

impl Stride for Frame {
    fn stride(&self) -> usize {
        self.pixmap.width() as usize * 4
    }
}
