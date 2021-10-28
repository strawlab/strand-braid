use chrono::{DateTime, Utc};
use ffmpeg_next::util::frame::video::Video;
use machine_vision_formats::{pixel_format::RGB8, ImageBuffer, ImageBufferRef, ImageData, Stride};

pub struct Frame {
    /// The presentation time stamp (in ffmpeg units)
    pub pts: i64,
    /// The presentation time stamp
    pub pts_chrono: DateTime<Utc>,
    /// The ffmpeg data
    pub rgb_frame: Video,
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        write!(
            f,
            "Frame {{ pts: {}, width: {}, stride: {}, height: {}, num_bytes: {}}}",
            self.pts,
            self.width(),
            self.stride(),
            self.height(),
            self.buffer_ref().data.len()
        )
    }
}

impl ImageData<RGB8> for Frame {
    fn width(&self) -> u32 {
        self.rgb_frame.width()
    }
    fn height(&self) -> u32 {
        self.rgb_frame.height()
    }
    fn buffer_ref(&self) -> ImageBufferRef<'_, RGB8> {
        ImageBufferRef {
            pixel_format: std::marker::PhantomData,
            data: self.rgb_frame.data(0),
        }
    }
    fn buffer(self) -> ImageBuffer<RGB8> {
        self.buffer_ref().to_buffer()
    }
}

impl ImageData<RGB8> for &Frame {
    fn width(&self) -> u32 {
        self.rgb_frame.width()
    }
    fn height(&self) -> u32 {
        self.rgb_frame.height()
    }
    fn buffer_ref(&self) -> ImageBufferRef<'_, RGB8> {
        ImageBufferRef {
            pixel_format: std::marker::PhantomData,
            data: self.rgb_frame.data(0),
        }
    }
    fn buffer(self) -> ImageBuffer<RGB8> {
        self.buffer_ref().to_buffer()
    }
}

impl Stride for Frame {
    fn stride(&self) -> usize {
        self.rgb_frame.stride(0)
    }
}

impl Stride for &Frame {
    fn stride(&self) -> usize {
        self.rgb_frame.stride(0)
    }
}
