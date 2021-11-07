use chrono::{DateTime, Utc};
use ffmpeg_next::util::frame::video::Video;
use machine_vision_formats::{pixel_format::RGB8, ImageBuffer, ImageBufferRef, ImageData, Stride};

use basic_frame::{BasicFrame, DynamicFrame};
use timestamped_frame::ExtraTimeData;

pub enum RawFrameSource {
    /// ffmpeg data
    Ffmpeg(Video),
    /// fmf data
    Fmf(BasicFrame<RGB8>),
}

pub struct Frame {
    /// The presentation time stamp
    pub pts_chrono: DateTime<Utc>,
    /// The frame data
    pub data: RawFrameSource,
}

impl TryFrom<DynamicFrame> for Frame {
    type Error = convert_image::Error;
    fn try_from(orig: DynamicFrame) -> Result<Self, Self::Error> {
        let pts_chrono = orig.extra().host_timestamp();
        let rgb_data = orig.into_pixel_format()?;
        Ok(Self {
            pts_chrono,
            data: RawFrameSource::Fmf(rgb_data),
        })
    }
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        write!(
            f,
            "Frame {{ width: {}, stride: {}, height: {}, num_bytes: {}}}",
            self.width(),
            self.stride(),
            self.height(),
            self.buffer_ref().data.len()
        )
    }
}

impl ImageData<RGB8> for Frame {
    fn width(&self) -> u32 {
        match &self.data {
            RawFrameSource::Ffmpeg(rgb_frame) => rgb_frame.width(),
            RawFrameSource::Fmf(rgb_frame) => rgb_frame.width(),
        }
    }
    fn height(&self) -> u32 {
        match &self.data {
            RawFrameSource::Ffmpeg(rgb_frame) => rgb_frame.height(),
            RawFrameSource::Fmf(rgb_frame) => rgb_frame.height(),
        }
    }
    fn buffer_ref(&self) -> ImageBufferRef<'_, RGB8> {
        match &self.data {
            RawFrameSource::Ffmpeg(rgb_frame) => ImageBufferRef {
                pixel_format: std::marker::PhantomData,
                data: rgb_frame.data(0),
            },
            RawFrameSource::Fmf(rgb_frame) => rgb_frame.buffer_ref(),
        }
    }
    fn buffer(self) -> ImageBuffer<RGB8> {
        self.buffer_ref().to_buffer()
    }
}

impl ImageData<RGB8> for &Frame {
    fn width(&self) -> u32 {
        match &self.data {
            RawFrameSource::Ffmpeg(rgb_frame) => rgb_frame.width(),
            RawFrameSource::Fmf(rgb_frame) => rgb_frame.width(),
        }
    }
    fn height(&self) -> u32 {
        match &self.data {
            RawFrameSource::Ffmpeg(rgb_frame) => rgb_frame.height(),
            RawFrameSource::Fmf(rgb_frame) => rgb_frame.height(),
        }
    }
    fn buffer_ref(&self) -> ImageBufferRef<'_, RGB8> {
        match &self.data {
            RawFrameSource::Ffmpeg(rgb_frame) => ImageBufferRef {
                pixel_format: std::marker::PhantomData,
                data: rgb_frame.data(0),
            },
            RawFrameSource::Fmf(rgb_frame) => rgb_frame.buffer_ref(),
        }
    }
    fn buffer(self) -> ImageBuffer<RGB8> {
        self.buffer_ref().to_buffer()
    }
}

impl Stride for Frame {
    fn stride(&self) -> usize {
        match &self.data {
            RawFrameSource::Ffmpeg(rgb_frame) => rgb_frame.stride(0),
            RawFrameSource::Fmf(rgb_frame) => rgb_frame.stride(),
        }
    }
}

impl Stride for &Frame {
    fn stride(&self) -> usize {
        match &self.data {
            RawFrameSource::Ffmpeg(rgb_frame) => rgb_frame.stride(0),
            RawFrameSource::Fmf(rgb_frame) => rgb_frame.stride(),
        }
    }
}
