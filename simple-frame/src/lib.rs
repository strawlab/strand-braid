use machine_vision_formats::{Stride, PixelFormat, ImageData, OwnedImageStride};

pub struct SimpleFrame {
    /// width in pixels
    pub width: u32,
    /// height in pixels
    pub height: u32,
    /// number of bytes in an image row
    pub stride: u32,
    /// raw image data
    pub image_data: Vec<u8>,
    /// format of the data
    pub pixel_format: PixelFormat,
}

fn _test_basic_frame_is_send() {
    // Compile-time test to ensure SimpleFrame implements Send trait.
    fn implements<T: Send>() {}
    implements::<SimpleFrame>();
}

fn _test_basic_frame_is_frame_trait() {
    // Compile-time test to ensure SimpleFrame implements OwnedImageStride trait.
    fn implements<T: OwnedImageStride>() {}
    implements::<SimpleFrame>();
}

fn _test_basic_frame_0() {
    fn implements<T: Into<Vec<u8>>>() {}
    implements::<SimpleFrame>();
}

fn _test_basic_frame_1() {
    fn implements<T: OwnedImageStride>() {}
    implements::<SimpleFrame>();
}

impl std::fmt::Debug for SimpleFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "SimpleFrame {{ {}x{} }}", self.width, self.height)
    }
}

impl SimpleFrame {
    pub fn copy_from<F: OwnedImageStride>(frame: &F) -> SimpleFrame {
        let width = frame.width();
        let height = frame.height();
        let stride = frame.stride() as u32;
        let pixel_format = frame.pixel_format();
        let image_data = frame.image_data().to_vec(); // copy data

        Self {
            width,
            height,
            stride,
            image_data,
            pixel_format,
        }
    }
}

impl ImageData for SimpleFrame {
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

impl Stride for SimpleFrame {
    fn stride(&self) -> usize {
        self.stride as usize
    }
}

impl From<SimpleFrame> for Vec<u8> {
    fn from(orig: SimpleFrame) -> Vec<u8> {
        orig.image_data
    }
}

impl From<Box<SimpleFrame>> for Vec<u8> {
    fn from(orig: Box<SimpleFrame>) -> Vec<u8> {
        orig.image_data
    }
}

impl<F> From<Box<F>> for SimpleFrame
    where
        F: OwnedImageStride,
        Vec<u8>: From<Box<F>>
{
    fn from(frame: Box<F>) -> SimpleFrame {

        let width = frame.width();
        let height = frame.height();
        let stride = frame.stride() as u32;
        let pixel_format = frame.pixel_format();

        SimpleFrame {
            width,
            height,
            stride,
            image_data: frame.into(),
            pixel_format,
        }
    }
}
