use chrono::{DateTime, Utc};
#[cfg(feature = "read-mkv")]
use ffmpeg_next::util::frame::video::Video;
use machine_vision_formats::{pixel_format::RGB8, ImageBuffer, ImageBufferRef, ImageData, Stride};

use basic_frame::{BasicFrame, DynamicFrame};
use timestamped_frame::ExtraTimeData;

pub(crate) enum RawFrameSource {
    /// ffmpeg data
    #[cfg(feature = "read-mkv")]
    Ffmpeg(Video),
    /// fmf data
    Fmf(BasicFrame<RGB8>),
}

pub struct Frame {
    /// The presentation time stamp
    pub(crate) pts_chrono: DateTime<Utc>,
    /// The frame data
    pub(crate) data: RawFrameSource,
    /// Extra timestamp data
    pub(crate) extra: basic_frame::BasicExtra,
}

impl TryFrom<DynamicFrame> for Frame {
    type Error = convert_image::Error;
    fn try_from(orig: DynamicFrame) -> Result<Self, Self::Error> {
        let pts_chrono = orig.extra().host_timestamp();
        let host_timestamp = orig.extra().host_timestamp();
        let host_framenumber = orig.extra().host_framenumber();
        let rgb_data = orig.into_pixel_format()?;
        let extra = basic_frame::BasicExtra {
            host_timestamp,
            host_framenumber,
        };
        Ok(Self {
            pts_chrono,
            data: RawFrameSource::Fmf(rgb_data),
            extra,
        })
    }
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        write!(
            f,
            "Frame {{ width: {}, stride: {}, height: {}, num_bytes: {}, pts: \"{}\"}}",
            self.width(),
            self.stride(),
            self.height(),
            self.buffer_ref().data.len(),
            self.pts_chrono,
        )
    }
}

impl ImageData<RGB8> for Frame {
    fn width(&self) -> u32 {
        match &self.data {
            #[cfg(feature = "read-mkv")]
            RawFrameSource::Ffmpeg(rgb_frame) => rgb_frame.width(),
            RawFrameSource::Fmf(rgb_frame) => rgb_frame.width(),
        }
    }
    fn height(&self) -> u32 {
        match &self.data {
            #[cfg(feature = "read-mkv")]
            RawFrameSource::Ffmpeg(rgb_frame) => rgb_frame.height(),
            RawFrameSource::Fmf(rgb_frame) => rgb_frame.height(),
        }
    }
    fn buffer_ref(&self) -> ImageBufferRef<'_, RGB8> {
        match &self.data {
            #[cfg(feature = "read-mkv")]
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
            #[cfg(feature = "read-mkv")]
            RawFrameSource::Ffmpeg(rgb_frame) => rgb_frame.width(),
            RawFrameSource::Fmf(rgb_frame) => rgb_frame.width(),
        }
    }
    fn height(&self) -> u32 {
        match &self.data {
            #[cfg(feature = "read-mkv")]
            RawFrameSource::Ffmpeg(rgb_frame) => rgb_frame.height(),
            RawFrameSource::Fmf(rgb_frame) => rgb_frame.height(),
        }
    }
    fn buffer_ref(&self) -> ImageBufferRef<'_, RGB8> {
        match &self.data {
            #[cfg(feature = "read-mkv")]
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
            #[cfg(feature = "read-mkv")]
            RawFrameSource::Ffmpeg(rgb_frame) => rgb_frame.stride(0),
            RawFrameSource::Fmf(rgb_frame) => rgb_frame.stride(),
        }
    }
}

impl Stride for &Frame {
    fn stride(&self) -> usize {
        match &self.data {
            #[cfg(feature = "read-mkv")]
            RawFrameSource::Ffmpeg(rgb_frame) => rgb_frame.stride(0),
            RawFrameSource::Fmf(rgb_frame) => rgb_frame.stride(),
        }
    }
}

impl timestamped_frame::ExtraTimeData for Frame {
    fn extra(&self) -> &dyn timestamped_frame::HostTimeData {
        &self.extra
    }
}
