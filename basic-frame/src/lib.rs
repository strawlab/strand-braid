use machine_vision_formats::{
    ImageBuffer, ImageBufferMutRef, ImageBufferRef, ImageData, ImageMutData, ImageStride, Stride,
};

mod dynamic_frame;
pub use dynamic_frame::DynamicFrame;

/// Convert a BasicFrame into another BasicFrame with a new pixel_format.
#[macro_export]
macro_rules! new_basic_frame {
    ($x:expr) => {{
        BasicFrame {
            width: $x.width,
            height: $x.height,
            stride: $x.stride,
            image_data: $x.image_data,
            pixel_format: std::marker::PhantomData,
        }
    }};
}

/// Return an DynamicFrame variant according to $pixfmt.
#[macro_export]
macro_rules! convert_to_dynamic {
    ($pixfmt:expr, $x:expr) => {{
        match $pixfmt {
            PixFmt::Mono8 => DynamicFrame::Mono8(new_basic_frame!($x)),
            PixFmt::Mono32f => DynamicFrame::Mono32f(new_basic_frame!($x)),
            PixFmt::RGB8 => DynamicFrame::RGB8(new_basic_frame!($x)),
            PixFmt::BayerRG8 => DynamicFrame::BayerRG8(new_basic_frame!($x)),
            PixFmt::BayerRG32f => DynamicFrame::BayerRG32f(new_basic_frame!($x)),
            PixFmt::BayerGB8 => DynamicFrame::BayerGB8(new_basic_frame!($x)),
            PixFmt::BayerGB32f => DynamicFrame::BayerGB32f(new_basic_frame!($x)),
            PixFmt::BayerGR8 => DynamicFrame::BayerGR8(new_basic_frame!($x)),
            PixFmt::BayerGR32f => DynamicFrame::BayerGR32f(new_basic_frame!($x)),
            PixFmt::BayerBG8 => DynamicFrame::BayerBG8(new_basic_frame!($x)),
            PixFmt::BayerBG32f => DynamicFrame::BayerBG32f(new_basic_frame!($x)),
            PixFmt::YUV422 => DynamicFrame::YUV422(new_basic_frame!($x)),

            PixFmt::NV12 => DynamicFrame::NV12(new_basic_frame!($x)),
            PixFmt::YUV444 => DynamicFrame::YUV444(new_basic_frame!($x)),
            _ => {
                panic!("unsupported pixel format {}", $pixfmt);
            }
        }
    }};
}

#[macro_export]
macro_rules! match_all_dynamic_fmts {
    ($self:expr, $x:ident, $block:expr) => {
        match $self {
            DynamicFrame::Mono8($x) => $block,
            DynamicFrame::Mono32f($x) => $block,
            DynamicFrame::RGB8($x) => $block,
            DynamicFrame::BayerRG8($x) => $block,
            DynamicFrame::BayerRG32f($x) => $block,
            DynamicFrame::BayerGB8($x) => $block,
            DynamicFrame::BayerGB32f($x) => $block,
            DynamicFrame::BayerGR8($x) => $block,
            DynamicFrame::BayerGR32f($x) => $block,
            DynamicFrame::BayerBG8($x) => $block,
            DynamicFrame::BayerBG32f($x) => $block,
            DynamicFrame::YUV444($x) => $block,
            DynamicFrame::YUV422($x) => $block,
            DynamicFrame::NV12($x) => $block,
        }
    };
}

/// Image data with a statically typed, strided pixel format.
#[derive(Clone)]
pub struct BasicFrame<F> {
    /// width in pixels
    pub width: u32,
    /// height in pixels
    pub height: u32,
    /// number of bytes in an image row
    pub stride: u32,
    /// raw image data
    pub image_data: Vec<u8>,
    /// pixel format
    pub pixel_format: std::marker::PhantomData<F>,
}

impl<F> PartialEq for BasicFrame<F>
where
    F: machine_vision_formats::PixelFormat,
{
    fn eq(&self, other: &BasicFrame<F>) -> bool {
        if self.width != other.width {
            return false;
        }
        if self.height != other.height {
            return false;
        }

        // We do enforce that `stride` is equal

        // We know `pixel_format` is the same due to type F.

        // Finally, check the buffers for equality in all regions where the pixels should be equal.
        let valid_size =
            usize::try_from(self.height).unwrap() * usize::try_from(self.stride).unwrap();
        let a_row_iter =
            self.image_data[..valid_size].chunks_exact(self.stride.try_into().unwrap());
        let b_row_iter =
            other.image_data[..valid_size].chunks_exact(other.stride.try_into().unwrap());

        let fmt = machine_vision_formats::pixel_format::pixfmt::<F>().unwrap();
        let valid_stride = fmt.bits_per_pixel() as usize * self.width as usize / 8;

        for (a_row, b_row) in a_row_iter.zip(b_row_iter) {
            if a_row[..valid_stride] != b_row[..valid_stride] {
                return false;
            }
        }
        true
    }
}

fn _test_basic_frame_is_send<F: Send>() {
    // Compile-time test to ensure BasicFrame implements Send trait.
    fn implements<T: Send>() {}
    implements::<BasicFrame<F>>();
}

fn _test_basic_frame_is_image_stride<F>() {
    // Compile-time test to ensure BasicFrame implements ImageStride trait.
    fn implements<T: ImageStride<F>, F>() {}
    implements::<BasicFrame<F>, F>();
}

impl<F> BasicFrame<F> {
    pub fn copy_from(frame: &dyn ImageStride<F>) -> BasicFrame<F> {
        let width = frame.width();
        let height = frame.height();
        let stride = frame.stride() as u32;

        let image_data = frame.image_data().to_vec(); // copy data

        Self {
            width,
            height,
            stride,
            image_data,
            pixel_format: std::marker::PhantomData,
        }
    }
}

impl<F> ImageData<F> for BasicFrame<F> {
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

impl<F> ImageMutData<F> for BasicFrame<F> {
    fn buffer_mut_ref(&mut self) -> ImageBufferMutRef<'_, F> {
        ImageBufferMutRef::new(&mut self.image_data)
    }
}

impl<F> Stride for BasicFrame<F> {
    fn stride(&self) -> usize {
        self.stride as usize
    }
}

impl<F> From<BasicFrame<F>> for Vec<u8> {
    fn from(orig: BasicFrame<F>) -> Vec<u8> {
        orig.image_data
    }
}

impl<F> From<Box<BasicFrame<F>>> for Vec<u8> {
    fn from(orig: Box<BasicFrame<F>>) -> Vec<u8> {
        orig.image_data
    }
}
