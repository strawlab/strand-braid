use formats::{ImageData, ImageStride, PixFmt, PixelFormat, Stride};
use machine_vision_formats as formats;

use crate::{convert_to_dynamic, match_all_dynamic_fmts, new_basic_frame, BasicFrame};

macro_rules! new_basic_frame_copy {
    ($x:expr) => {{
        BasicFrame {
            width: $x.width(),
            height: $x.height(),
            stride: $x.stride() as u32,
            image_data: $x.image_data().to_vec(),
            pixel_format: std::marker::PhantomData,
        }
    }};
}

macro_rules! new_basic_frame_move {
    ($x:expr) => {{
        let width = $x.width();
        let height = $x.height();
        let stride = $x.stride() as u32;
        let image_data: Vec<u8> = $x.into();
        BasicFrame {
            width,
            height,
            stride,
            image_data,
            pixel_format: std::marker::PhantomData,
        }
    }};
}

macro_rules! convert_to_dynamic2 {
    ($format_type:ty, $x:expr) => {{
        let pixfmt = formats::pixel_format::pixfmt::<$format_type>().unwrap();
        match pixfmt {
            PixFmt::Mono8 => DynamicFrame::Mono8($x),
            PixFmt::Mono32f => DynamicFrame::Mono32f($x),
            PixFmt::RGB8 => DynamicFrame::RGB8($x),
            PixFmt::BayerRG8 => DynamicFrame::BayerRG8($x),
            PixFmt::BayerRG32f => DynamicFrame::BayerRG32f($x),
            PixFmt::BayerGB8 => DynamicFrame::BayerGB8($x),
            PixFmt::BayerGB32f => DynamicFrame::BayerGB32f($x),
            PixFmt::BayerGR8 => DynamicFrame::BayerGR8($x),
            PixFmt::BayerGR32f => DynamicFrame::BayerGR32f($x),
            PixFmt::BayerBG8 => DynamicFrame::BayerBG8($x),
            PixFmt::BayerBG32f => DynamicFrame::BayerBG32f($x),
            PixFmt::YUV422 => DynamicFrame::YUV422($x),
            _ => {
                panic!("unsupported type {}", pixfmt);
            }
        }
    }};
}

/// An image whose pixel format is known only at runtime.
///
/// When reading an image from disk, for example, its pixel format is not known
/// in advance. This enum represents the different possible formats as different
/// variants.
///
/// Note that we do not implement `ImageData<FMT>` trait because the pixel
/// format (parameterized by FMT) is not known at compile-time for DynamicFrame.
#[allow(non_camel_case_types)]
#[derive(Clone, PartialEq)]
pub enum DynamicFrame {
    Mono8(BasicFrame<formats::pixel_format::Mono8>),
    Mono32f(BasicFrame<formats::pixel_format::Mono32f>),
    RGB8(BasicFrame<formats::pixel_format::RGB8>),
    BayerRG8(BasicFrame<formats::pixel_format::BayerRG8>),
    BayerRG32f(BasicFrame<formats::pixel_format::BayerRG32f>),
    BayerGB8(BasicFrame<formats::pixel_format::BayerGB8>),
    BayerGB32f(BasicFrame<formats::pixel_format::BayerGB32f>),
    BayerGR8(BasicFrame<formats::pixel_format::BayerGR8>),
    BayerGR32f(BasicFrame<formats::pixel_format::BayerGR32f>),
    BayerBG8(BasicFrame<formats::pixel_format::BayerBG8>),
    BayerBG32f(BasicFrame<formats::pixel_format::BayerBG32f>),
    YUV444(BasicFrame<formats::pixel_format::YUV444>),
    YUV422(BasicFrame<formats::pixel_format::YUV422>),
    NV12(BasicFrame<formats::pixel_format::NV12>),
}

fn _test_dynamic_frame_is_send() {
    // Compile-time test to ensure DynamicFrame implements Send trait.
    fn implements<T: Send>() {}
    implements::<DynamicFrame>();
}

impl std::fmt::Debug for DynamicFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        write!(f, "DynamicFrame{{{}, ..}}", self.pixel_format())
    }
}

impl DynamicFrame {
    pub fn new(
        width: u32,
        height: u32,
        stride: u32,
        image_data: Vec<u8>,
        pixel_format: PixFmt,
    ) -> DynamicFrame {
        // First create a variant with likely the wrong type...
        let wrong_type = DynamicFrame::Mono8(BasicFrame {
            width,
            height,
            stride,
            image_data,
            pixel_format: std::marker::PhantomData,
        });
        // ...then convert it to the right type.
        wrong_type.force_pixel_format(pixel_format)
    }
    pub fn copy_from<FMT: PixelFormat>(frame: &dyn ImageStride<FMT>) -> Self {
        convert_to_dynamic2!(FMT, new_basic_frame_copy!(frame))
    }

    // TODO: actually implement the From trait. However, this is more difficult
    // than it may initially sound because of trait generic stuff.
    pub fn from<FRAME, FMT>(frame: FRAME) -> Self
    where
        FRAME: ImageStride<FMT> + Into<Vec<u8>>,
        FMT: PixelFormat,
    {
        convert_to_dynamic2!(FMT, new_basic_frame_move!(frame))
    }
}

impl DynamicFrame {
    /// Return the image as a [BasicFrame] of the given pixel format.
    ///
    /// This is done by moving the data. No copy is made.
    ///
    /// If the image is a different pixel format than requested, None will be
    /// returned.
    ///
    /// To convert the image data if necessary, use [Self::into_pixel_format].
    pub fn as_basic<FMT>(self) -> Option<BasicFrame<FMT>>
    where
        FMT: PixelFormat,
    {
        let pixfmt = formats::pixel_format::pixfmt::<FMT>().unwrap();
        if pixfmt == self.pixel_format() {
            let width = self.width();
            let height = self.height();
            let stride = self.stride() as u32;
            let image_data = self.into();
            Some(BasicFrame {
                width,
                height,
                stride,
                image_data,
                pixel_format: std::marker::PhantomData,
            })
        } else {
            None
        }
    }

    #[cfg(feature = "convert-image")]
    /// Return the image as a [BasicFrame] converting the data to the requested
    /// pixel format as necessary.
    ///
    /// If the requested pixel format is the current pixel format, this moves
    /// the data (without reallocation or copying). Otherwise, the data is
    /// converted.
    ///
    /// To avoid converting the data, use [Self::as_basic].
    ///
    /// Consider using [Self::into_pixel_format2], which returns a view of the
    /// original data if no conversion is necessary.
    pub fn into_pixel_format<FMT>(self) -> Result<BasicFrame<FMT>, convert_image::Error>
    where
        FMT: PixelFormat,
    {
        let pixfmt = formats::pixel_format::pixfmt::<FMT>().unwrap();
        if pixfmt == self.pixel_format() {
            // Fast path. Simply return the data.
            let width = self.width();
            let height = self.height();
            let stride = self.stride() as u32;
            let image_data = self.into();
            Ok(BasicFrame {
                width,
                height,
                stride,
                image_data,
                pixel_format: std::marker::PhantomData,
            })
        } else {
            let width = self.width();
            let height = self.height();

            let dest_fmt = machine_vision_formats::pixel_format::pixfmt::<FMT>().unwrap();

            // Allocate buffer for new image.
            let dest_stride = dest_fmt.bits_per_pixel() as usize * width as usize / 8;
            let dest_size = height as usize * dest_stride;
            let mut dest_buf = vec![0u8; dest_size];

            {
                let mut dest = formats::image_ref::ImageRefMut::<FMT>::new(
                    width,
                    height,
                    dest_stride,
                    &mut dest_buf,
                )
                .unwrap();

                match_all_dynamic_fmts!(&self, x, convert_image::convert_into(x, &mut dest)?);
            }

            let image_data = dest_buf;

            Ok(BasicFrame {
                width,
                height,
                stride: dest_stride as u32,
                image_data,
                pixel_format: std::marker::PhantomData,
            })
        }
    }

    #[cfg(feature = "convert-image")]
    /// Return the image as a [CowImage] converting the data to the requested
    /// pixel format as necessary.
    ///
    /// If the requested pixel format is the current pixel format, this borrows
    /// the data (without reallocation or copying). Otherwise, the data is
    /// converted and copied.
    ///
    /// To avoid converting the data, use [Self::as_basic].
    pub fn into_pixel_format2<FMT>(&self) -> Result<CowImage<FMT>, convert_image::Error>
    where
        FMT: PixelFormat,
    {
        let pixfmt = formats::pixel_format::pixfmt::<FMT>().unwrap();
        if pixfmt == self.pixel_format() {
            // Fast path. Simply return the data.
            Ok(CowImage::Borrowed(
                ImageRef::new(
                    self.width(),
                    self.height(),
                    self.stride(),
                    self.image_data_without_format(),
                )
                .unwrap(),
            ))
        } else {
            let width = self.width();
            let height = self.height();

            let dest_fmt = machine_vision_formats::pixel_format::pixfmt::<FMT>().unwrap();

            // Allocate buffer for new image.
            let dest_stride = dest_fmt.bits_per_pixel() as usize * width as usize / 8;
            let dest_size = height as usize * dest_stride;
            let mut dest_buf = vec![0u8; dest_size];

            {
                let mut dest = formats::image_ref::ImageRefMut::<FMT>::new(
                    width,
                    height,
                    dest_stride,
                    &mut dest_buf,
                )
                .unwrap();

                match_all_dynamic_fmts!(&self, x, convert_image::convert_into(x, &mut dest)?);
            }

            Ok(CowImage::Owned(
                OImage::new(self.width(), self.height(), self.stride(), dest_buf).unwrap(),
            ))
        }
    }

    pub fn pixel_format(&self) -> PixFmt {
        use DynamicFrame::*;
        match self {
            Mono8(_) => PixFmt::Mono8,
            Mono32f(_) => PixFmt::Mono32f,
            RGB8(_) => PixFmt::RGB8,
            BayerRG8(_) => PixFmt::BayerRG8,
            BayerRG32f(_) => PixFmt::BayerRG32f,
            BayerGB8(_) => PixFmt::BayerGB8,
            BayerGB32f(_) => PixFmt::BayerGB32f,
            BayerGR8(_) => PixFmt::BayerGR8,
            BayerGR32f(_) => PixFmt::BayerGR32f,
            BayerBG8(_) => PixFmt::BayerBG8,
            BayerBG32f(_) => PixFmt::BayerBG32f,
            YUV444(_) => PixFmt::YUV444,
            YUV422(_) => PixFmt::YUV422,
            NV12(_) => PixFmt::NV12,
        }
    }
    /// Force the frame into a new pixel format without altering the data.
    pub fn force_pixel_format(self, pixfmt: PixFmt) -> DynamicFrame {
        match_all_dynamic_fmts!(self, x, { convert_to_dynamic!(pixfmt, x) })
    }
    pub fn width(&self) -> u32 {
        match_all_dynamic_fmts!(self, x, { x.width() })
    }
    pub fn height(&self) -> u32 {
        match_all_dynamic_fmts!(self, x, { x.height() })
    }
    /// Get a view of the image data.
    ///
    /// Note that this discards any type information about the pixel format.
    pub fn image_data_without_format(&self) -> &[u8] {
        match_all_dynamic_fmts!(self, x, &x.image_data)
    }
}

impl From<DynamicFrame> for Vec<u8> {
    fn from(orig: DynamicFrame) -> Self {
        match_all_dynamic_fmts!(orig, x, { x.image_data })
    }
}

impl Stride for DynamicFrame {
    fn stride(&self) -> usize {
        match_all_dynamic_fmts!(self, x, { x.stride() })
    }
}

// ----------------
use machine_vision_formats::{image_ref::ImageRef, owned::OImage, ImageBuffer, ImageBufferRef};

pub enum CowImage<'a, F: PixelFormat> {
    Borrowed(ImageRef<'a, F>),
    Owned(OImage<F>),
}

impl<'a, F: PixelFormat> From<ImageRef<'a, F>> for CowImage<'a, F> {
    fn from(frame: ImageRef<'a, F>) -> CowImage<'a, F> {
        CowImage::Borrowed(frame)
    }
}

impl<'a, F: PixelFormat> From<OImage<F>> for CowImage<'a, F> {
    fn from(frame: OImage<F>) -> CowImage<'a, F> {
        CowImage::Owned(frame)
    }
}

impl<F: PixelFormat> Stride for CowImage<'_, F> {
    fn stride(&self) -> usize {
        match self {
            CowImage::Borrowed(im) => im.stride(),
            CowImage::Owned(im) => im.stride(),
        }
    }
}

impl<F: PixelFormat> ImageData<F> for CowImage<'_, F> {
    fn width(&self) -> u32 {
        match self {
            CowImage::Borrowed(im) => im.width(),
            CowImage::Owned(im) => im.width(),
        }
    }
    fn height(&self) -> u32 {
        match self {
            CowImage::Borrowed(im) => im.height(),
            CowImage::Owned(im) => im.height(),
        }
    }
    fn buffer_ref(&self) -> ImageBufferRef<'_, F> {
        let image_data = match self {
            CowImage::Borrowed(im) => im.image_data(),
            CowImage::Owned(im) => im.image_data(),
        };
        ImageBufferRef::new(image_data)
    }
    fn buffer(self) -> ImageBuffer<F> {
        match self {
            CowImage::Borrowed(im) => ImageBuffer::new(im.image_data().to_vec()),
            CowImage::Owned(im) => ImageBuffer::new(im.into()),
        }
    }
}
