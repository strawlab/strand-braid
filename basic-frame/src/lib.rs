use machine_vision_formats::{Stride, PixelFormat, ImageData};
use timestamped_frame::{HostTimeData, ImageStrideTime, FrameTrait};

#[derive(Clone)]
pub struct BasicFrame {
    /// width in pixels
    pub width: u32,
    /// height in pixels
    pub height: u32,
    /// number of bytes in an image row
    pub stride: u32,
    /// raw image data
    pub image_data: Vec<u8>,
    /// timestamp from host computer
    pub host_timestamp: chrono::DateTime<chrono::Utc>,
    /// framenumber from host computer
    pub host_framenumber: usize,
    /// format of the data
    pub pixel_format: PixelFormat,
}

fn _test_basic_frame_is_send() {
    // Compile-time test to ensure BasicFrame implements Send trait.
    fn implements<T: Send>() {}
    implements::<BasicFrame>();
}

fn _test_basic_frame_is_frame_trait() {
    // Compile-time test to ensure BasicFrame implements FrameTrait trait.
    fn implements<T: FrameTrait>() {}
    implements::<BasicFrame>();
}

fn _test_basic_frame_0() {
    fn implements<T: Into<Vec<u8>>>() {}
    implements::<BasicFrame>();
}

fn _test_basic_frame_1() {
    fn implements<T: ImageStrideTime>() {}
    implements::<BasicFrame>();
}

impl std::fmt::Debug for BasicFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "BasicFrame {{ {}x{} }}", self.width, self.height)
    }
}

impl BasicFrame {
    pub fn copy_from(frame: &dyn ImageStrideTime) -> BasicFrame {
        let width = frame.width();
        let height = frame.height();
        let stride = frame.stride() as u32;
        let host_timestamp = frame.host_timestamp();
        let host_framenumber = frame.host_framenumber();
        let pixel_format = frame.pixel_format();
        let image_data = frame.image_data().to_vec(); // copy data

        Self {
            width,
            height,
            stride,
            image_data,
            host_timestamp,
            host_framenumber,
            pixel_format,
        }
    }
}

impl HostTimeData for BasicFrame {
    fn host_timestamp(&self) -> chrono::DateTime<chrono::Utc> {
        self.host_timestamp
    }
    fn host_framenumber(&self) -> usize {
        self.host_framenumber
    }
}

impl ImageData for BasicFrame {
    fn image_data(&self) -> &[u8] {
        &self.image_data
    }
    fn width(&self) -> u32 {
        self.width
    }
    fn height(&self) -> u32 {
        self.height
    }
    fn pixel_format(&self) -> PixelFormat {
        self.pixel_format
    }
}

impl Stride for BasicFrame {
    fn stride(&self) -> usize {
        self.stride as usize
    }
}

impl From<BasicFrame> for Vec<u8> {
    fn from(orig: BasicFrame) -> Vec<u8> {
        orig.image_data
    }
}

impl From<Box<BasicFrame>> for Vec<u8> {
    fn from(orig: Box<BasicFrame>) -> Vec<u8> {
        orig.image_data
    }
}

impl<F> From<Box<F>> for BasicFrame
    where
        F: FrameTrait,
        Vec<u8>: From<Box<F>>
{
    fn from(frame: Box<F>) -> BasicFrame {

        let width = frame.width();
        let height = frame.height();
        let stride = frame.stride() as u32;
        let host_timestamp = frame.host_timestamp();
        let host_framenumber = frame.host_framenumber();
        let pixel_format = frame.pixel_format();

        BasicFrame {
            width,
            height,
            stride,
            image_data: frame.into(),
            host_timestamp,
            host_framenumber,
            pixel_format,
        }
    }
}
