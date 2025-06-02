#![cfg_attr(not(doctest), doc = include_str!("../README.md"))]

use machine_vision_formats as formats;

use formats::{
    image_ref::ImageRef, owned::OImage, ImageBuffer, ImageBufferRef, ImageData, ImageStride,
    PixFmt, PixelFormat, Stride,
};

fn convert_to_dynamic<FRAME, FMT>(frame: FRAME) -> DynamicFrame
where
    FRAME: ImageStride<FMT> + Into<Vec<u8>>,
    FMT: PixelFormat,
{
    let pixfmt = formats::pixel_format::pixfmt::<FMT>().unwrap();
    let w = frame.width();
    let h = frame.height();
    let s = frame.stride();
    let buf = frame.into();
    match pixfmt {
        PixFmt::Mono8 => DynamicFrame::Mono8(OImage::new(w, h, s, buf).unwrap()),
        PixFmt::Mono32f => DynamicFrame::Mono32f(OImage::new(w, h, s, buf).unwrap()),
        PixFmt::RGB8 => DynamicFrame::RGB8(OImage::new(w, h, s, buf).unwrap()),
        PixFmt::BayerRG8 => DynamicFrame::BayerRG8(OImage::new(w, h, s, buf).unwrap()),
        PixFmt::BayerRG32f => DynamicFrame::BayerRG32f(OImage::new(w, h, s, buf).unwrap()),
        PixFmt::BayerGB8 => DynamicFrame::BayerGB8(OImage::new(w, h, s, buf).unwrap()),
        PixFmt::BayerGB32f => DynamicFrame::BayerGB32f(OImage::new(w, h, s, buf).unwrap()),
        PixFmt::BayerGR8 => DynamicFrame::BayerGR8(OImage::new(w, h, s, buf).unwrap()),
        PixFmt::BayerGR32f => DynamicFrame::BayerGR32f(OImage::new(w, h, s, buf).unwrap()),
        PixFmt::BayerBG8 => DynamicFrame::BayerBG8(OImage::new(w, h, s, buf).unwrap()),
        PixFmt::BayerBG32f => DynamicFrame::BayerBG32f(OImage::new(w, h, s, buf).unwrap()),
        PixFmt::YUV422 => DynamicFrame::YUV422(OImage::new(w, h, s, buf).unwrap()),

        PixFmt::NV12 => DynamicFrame::NV12(OImage::new(w, h, s, buf).unwrap()),
        PixFmt::YUV444 => DynamicFrame::YUV444(OImage::new(w, h, s, buf).unwrap()),
        _ => {
            panic!("unsupported pixel format {}", pixfmt);
        }
    }
}

/// Match all [DynamicFrame] variants and execute an expression.
///
/// `$self` is the [DynamicFrame] and `$x` is the identifier of the [OImage]
/// used in the `$block`.
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

/// An image whose pixel format is known only at runtime.
///
/// When reading an image from disk, for example, its pixel format is not known
/// in advance. This enum represents the different possible formats as different
/// variants.
///
/// Note that we do not implement `ImageData<FMT>` trait because the pixel
/// format (parameterized by FMT) is not known at compile-time for DynamicFrame.
#[allow(non_camel_case_types)]
#[derive(Clone)]
pub enum DynamicFrame {
    Mono8(OImage<formats::pixel_format::Mono8>),
    Mono32f(OImage<formats::pixel_format::Mono32f>),
    RGB8(OImage<formats::pixel_format::RGB8>),
    BayerRG8(OImage<formats::pixel_format::BayerRG8>),
    BayerRG32f(OImage<formats::pixel_format::BayerRG32f>),
    BayerGB8(OImage<formats::pixel_format::BayerGB8>),
    BayerGB32f(OImage<formats::pixel_format::BayerGB32f>),
    BayerGR8(OImage<formats::pixel_format::BayerGR8>),
    BayerGR32f(OImage<formats::pixel_format::BayerGR32f>),
    BayerBG8(OImage<formats::pixel_format::BayerBG8>),
    BayerBG32f(OImage<formats::pixel_format::BayerBG32f>),
    YUV444(OImage<formats::pixel_format::YUV444>),
    YUV422(OImage<formats::pixel_format::YUV422>),
    NV12(OImage<formats::pixel_format::NV12>),
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
    /// Move raw data (without copy) into a new [DynamicFrame].
    pub fn new(w: u32, h: u32, s: usize, buf: Vec<u8>, pixfmt: PixFmt) -> Option<DynamicFrame> {
        match pixfmt {
            PixFmt::Mono8 => OImage::new(w, h, s, buf).map(DynamicFrame::Mono8),
            PixFmt::Mono32f => OImage::new(w, h, s, buf).map(DynamicFrame::Mono32f),
            PixFmt::RGB8 => OImage::new(w, h, s, buf).map(DynamicFrame::RGB8),
            PixFmt::BayerRG8 => OImage::new(w, h, s, buf).map(DynamicFrame::BayerRG8),
            PixFmt::BayerRG32f => OImage::new(w, h, s, buf).map(DynamicFrame::BayerRG32f),
            PixFmt::BayerGB8 => OImage::new(w, h, s, buf).map(DynamicFrame::BayerGB8),
            PixFmt::BayerGB32f => OImage::new(w, h, s, buf).map(DynamicFrame::BayerGB32f),
            PixFmt::BayerGR8 => OImage::new(w, h, s, buf).map(DynamicFrame::BayerGR8),
            PixFmt::BayerGR32f => OImage::new(w, h, s, buf).map(DynamicFrame::BayerGR32f),
            PixFmt::BayerBG8 => OImage::new(w, h, s, buf).map(DynamicFrame::BayerBG8),
            PixFmt::BayerBG32f => OImage::new(w, h, s, buf).map(DynamicFrame::BayerBG32f),
            PixFmt::YUV422 => OImage::new(w, h, s, buf).map(DynamicFrame::YUV422),

            PixFmt::NV12 => OImage::new(w, h, s, buf).map(DynamicFrame::NV12),
            PixFmt::YUV444 => OImage::new(w, h, s, buf).map(DynamicFrame::YUV444),
            _ => {
                panic!("unsupported pixel format {}", pixfmt);
            }
        }
    }
    pub fn copy_from<FMT: PixelFormat>(frame: &dyn ImageStride<FMT>) -> Self {
        let buf = frame.image_data().to_vec(); // copy data
        convert_to_dynamic(
            OImage::<FMT>::new(frame.width(), frame.height(), frame.stride(), buf).unwrap(),
        )
    }

    // TODO: actually implement the From trait. However, this is more difficult
    // than it may initially sound because of trait generic stuff.
    pub fn from<FRAME, FMT>(frame: FRAME) -> Self
    where
        FRAME: ImageStride<FMT> + Into<Vec<u8>>,
        FMT: PixelFormat,
    {
        let w = frame.width();
        let h = frame.height();
        let s = frame.stride();
        let buf = frame.into(); // move data
        convert_to_dynamic(OImage::<FMT>::new(w, h, s, buf).unwrap())
    }
}

impl DynamicFrame {
    /// Return the image as a [OImage] of the given pixel format.
    ///
    /// This is done by moving the data. No copy is made.
    ///
    /// If the image is a different pixel format than requested, None will be
    /// returned.
    ///
    /// To convert the image data if necessary, use [Self::into_pixel_format]
    /// (which requires the `convert-image` feature).
    pub fn as_basic<FMT>(self) -> Option<OImage<FMT>>
    where
        FMT: PixelFormat,
    {
        let pixfmt = formats::pixel_format::pixfmt::<FMT>().unwrap();
        if pixfmt == self.pixel_format() {
            let width = self.width();
            let height = self.height();
            let stride = self.stride();
            let image_data = self.into();
            OImage::new(width, height, stride, image_data)
        } else {
            None
        }
    }

    #[cfg(feature = "convert-image")]
    /// Return the image as a [OImage] converting the data to the requested
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
    pub fn into_pixel_format<FMT>(self) -> Result<OImage<FMT>, convert_image::Error>
    where
        FMT: PixelFormat,
    {
        let pixfmt = formats::pixel_format::pixfmt::<FMT>().unwrap();
        if pixfmt == self.pixel_format() {
            // Fast path. Simply return the data.
            let width = self.width();
            let height = self.height();
            let stride = self.stride();
            let image_data = self.into();
            Ok(OImage::new(width, height, stride, image_data).unwrap())
        } else {
            let width = self.width();
            let height = self.height();

            let dest_fmt = formats::pixel_format::pixfmt::<FMT>().unwrap();

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

            Ok(OImage::new(width, height, dest_stride, image_data).unwrap())
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

            let dest_fmt = formats::pixel_format::pixfmt::<FMT>().unwrap();

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
    ///
    /// Returns None if the buffer is not large enough for the desired new format.
    pub fn force_pixel_format(self, pixfmt: PixFmt) -> Option<DynamicFrame> {
        let x = self;
        let w = x.width();
        let h = x.height();
        let s = x.stride();
        let buf = x.into();

        match pixfmt {
            PixFmt::Mono8 => OImage::new(w, h, s, buf).map(DynamicFrame::Mono8),
            PixFmt::Mono32f => OImage::new(w, h, s, buf).map(DynamicFrame::Mono32f),
            PixFmt::RGB8 => OImage::new(w, h, s, buf).map(DynamicFrame::RGB8),
            PixFmt::BayerRG8 => OImage::new(w, h, s, buf).map(DynamicFrame::BayerRG8),
            PixFmt::BayerRG32f => OImage::new(w, h, s, buf).map(DynamicFrame::BayerRG32f),
            PixFmt::BayerGB8 => OImage::new(w, h, s, buf).map(DynamicFrame::BayerGB8),
            PixFmt::BayerGB32f => OImage::new(w, h, s, buf).map(DynamicFrame::BayerGB32f),
            PixFmt::BayerGR8 => OImage::new(w, h, s, buf).map(DynamicFrame::BayerGR8),
            PixFmt::BayerGR32f => OImage::new(w, h, s, buf).map(DynamicFrame::BayerGR32f),
            PixFmt::BayerBG8 => OImage::new(w, h, s, buf).map(DynamicFrame::BayerBG8),
            PixFmt::BayerBG32f => OImage::new(w, h, s, buf).map(DynamicFrame::BayerBG32f),
            PixFmt::YUV422 => OImage::new(w, h, s, buf).map(DynamicFrame::YUV422),

            PixFmt::NV12 => OImage::new(w, h, s, buf).map(DynamicFrame::NV12),
            PixFmt::YUV444 => OImage::new(w, h, s, buf).map(DynamicFrame::YUV444),
            _ => {
                panic!("unsupported pixel format {}", pixfmt);
            }
        }
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
        match_all_dynamic_fmts!(self, x, &x.image_data())
    }
}

impl From<DynamicFrame> for Vec<u8> {
    fn from(orig: DynamicFrame) -> Self {
        match_all_dynamic_fmts!(orig, x, { x.into() })
    }
}

impl Stride for DynamicFrame {
    fn stride(&self) -> usize {
        match_all_dynamic_fmts!(self, x, { x.stride() })
    }
}

/// An owned or borrowed image. Implements [ImageData] and [Stride].
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
