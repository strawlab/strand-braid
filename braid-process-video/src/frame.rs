use chrono::{DateTime, Utc};
use ffmpeg_next::util::frame::video::Video;

pub struct Frame {
    /// The presentation time stamp (in ffmpeg units)
    pub pts: i64,
    /// The presentation time stamp
    pub pts_chrono: DateTime<Utc>,
    /// The ffmpeg data
    pub rgb_frame: Video,
}

impl Frame {
    pub fn bytes(&self) -> &[u8] {
        self.rgb_frame.data(0)
    }
    pub fn stride(&self) -> usize {
        self.rgb_frame.stride(0)
    }
    pub fn width(&self) -> u32 {
        self.rgb_frame.width()
    }
    pub fn height(&self) -> u32 {
        self.rgb_frame.height()
    }
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
            self.bytes().len(),
        )
    }
}
