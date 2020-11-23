use machine_vision_formats::{ImageData, PixelFormat, Stride};

/// A view of a source image in which the rightmost pixels may be clipped
pub(crate) struct ClippedFrame<'a> {
    src: &'a basic_frame::BasicFrame,
    width: u32,
}

impl<'a> ImageData for ClippedFrame<'a> {
    fn width(&self) -> u32 {
        self.width
    }
    fn height(&self) -> u32 {
        self.src.height()
    }
    fn image_data(&self) -> &[u8] {
        self.src.image_data()
    }
    fn pixel_format(&self) -> PixelFormat {
        self.src.pixel_format()
    }
}

impl<'a> Stride for ClippedFrame<'a> {
    fn stride(&self) -> usize {
        self.src.stride()
    }
}

pub(crate) trait ClipFrame {
    fn clip_to_power_of_2(&self, val: u8) -> ClippedFrame;
}

impl ClipFrame for basic_frame::BasicFrame {
    fn clip_to_power_of_2(&self, val: u8) -> ClippedFrame {
        let width = (self.width() / val as u32) * val as u32;
        debug!("clipping image of width {} to {}", self.width(), width);
        ClippedFrame { src: &self, width }
    }
}
