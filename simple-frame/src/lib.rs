use machine_vision_formats::{
    ImageBuffer, ImageBufferMutRef, ImageBufferRef, ImageData, ImageMutData, ImageStride,
    OwnedImageStride, PixelFormat, Stride,
};

#[derive(Clone)]
pub struct SimpleFrame<F> {
    /// width in pixels
    pub width: u32,
    /// height in pixels
    pub height: u32,
    /// number of bytes in an image row
    pub stride: u32,
    /// raw image data
    image_data: Vec<u8>,
    /// format of the data
    pub fmt: std::marker::PhantomData<F>,
}

impl<F> SimpleFrame<F>
where
    F: PixelFormat,
{
    /// Move a `Vec<u8>` buffer as the backing store for a SimpleFrame for
    /// image.
    ///
    /// Returns None if the buffer is not large enough to store an image of the
    /// desired properties.
    pub fn new(width: u32, height: u32, stride: u32, image_data: Vec<u8>) -> Option<Self> {
        let fmt = machine_vision_formats::pixel_format::pixfmt::<F>().unwrap();
        let valid_stride = fmt.bits_per_pixel() as usize * width as usize / 8;

        let sz = stride as usize * (height as usize - 1) + valid_stride;

        if image_data.len() < sz {
            return None;
        }
        Some(Self {
            width,
            height,
            stride,
            image_data,
            fmt: std::marker::PhantomData,
        })
    }
    /// Allocate minimium size buffer for image and fill with zeros
    pub fn zeros(width: u32, height: u32, stride: u32) -> Option<Self> {
        let fmt = machine_vision_formats::pixel_format::pixfmt::<F>().unwrap();
        let valid_stride = fmt.bits_per_pixel() as usize * width as usize / 8;

        let sz = stride as usize * (height as usize - 1) + valid_stride;
        let image_data = vec![0u8; sz];
        Some(Self {
            width,
            height,
            stride,
            image_data,
            fmt: std::marker::PhantomData,
        })
    }
}

fn _test_basic_frame_is_send<F: Send>() {
    // Compile-time test to ensure SimpleFrame implements Send trait.
    fn implements<T: Send>() {}
    implements::<SimpleFrame<F>>();
}

fn _test_basic_frame_is_frame_trait<F>() {
    // Compile-time test to ensure SimpleFrame implements OwnedImageStride trait.
    fn implements<T: OwnedImageStride<F>, F>() {}
    implements::<SimpleFrame<F>, F>();
}

fn _test_basic_frame_0<F>() {
    fn implements<T: Into<Vec<u8>>>() {}
    implements::<SimpleFrame<F>>();
}

fn _test_basic_frame_1<F>() {
    fn implements<T: OwnedImageStride<F>, F>() {}
    implements::<SimpleFrame<F>, F>();
}

impl<F> std::fmt::Debug for SimpleFrame<F> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "SimpleFrame {{ {}x{} }}", self.width, self.height)
    }
}

impl<F> SimpleFrame<F> {
    pub fn copy_from<FRAME: ImageStride<F>>(frame: &FRAME) -> SimpleFrame<F> {
        let width = frame.width();
        let height = frame.height();
        let stride = frame.stride() as u32;
        let image_data = frame.image_data().to_vec(); // copy data

        Self {
            width,
            height,
            stride,
            image_data,
            fmt: std::marker::PhantomData,
        }
    }
}

impl<F> ImageData<F> for SimpleFrame<F> {
    fn width(&self) -> u32 {
        self.width
    }
    fn height(&self) -> u32 {
        self.height
    }
    fn buffer_ref(&self) -> ImageBufferRef<'_, F> {
        ImageBufferRef::new(&self.image_data)
    }
    fn buffer(self) -> ImageBuffer<F> {
        ImageBuffer::new(self.image_data)
    }
}

impl<F> ImageMutData<F> for SimpleFrame<F> {
    fn buffer_mut_ref(&mut self) -> ImageBufferMutRef<'_, F> {
        ImageBufferMutRef::new(&mut self.image_data)
    }
}

impl<F> Stride for SimpleFrame<F> {
    fn stride(&self) -> usize {
        self.stride as usize
    }
}

impl<F> From<SimpleFrame<F>> for Vec<u8> {
    fn from(orig: SimpleFrame<F>) -> Vec<u8> {
        orig.image_data
    }
}

impl<F> From<Box<SimpleFrame<F>>> for Vec<u8> {
    fn from(orig: Box<SimpleFrame<F>>) -> Vec<u8> {
        orig.image_data
    }
}

impl<FRAME, FMT> From<Box<FRAME>> for SimpleFrame<FMT>
where
    FRAME: OwnedImageStride<FMT>,
    Vec<u8>: From<Box<FRAME>>,
{
    fn from(frame: Box<FRAME>) -> SimpleFrame<FMT> {
        let width = frame.width();
        let height = frame.height();
        let stride = frame.stride() as u32;

        SimpleFrame {
            width,
            height,
            stride,
            image_data: frame.into(),
            fmt: std::marker::PhantomData,
        }
    }
}
