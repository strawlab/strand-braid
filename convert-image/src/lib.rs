#![recursion_limit = "512"]
#![cfg_attr(feature = "backtrace", feature(error_generic_member_access))]

// TODO: Add support for Reversible Color Transform (RCT) YUV types

use bayer as wang_debayer;
use machine_vision_formats as formats;

use formats::{
    pixel_format::{self, Mono8, NV12, RGB8},
    ImageBuffer, ImageBufferMutRef, ImageBufferRef, ImageData, ImageMutData, OwnedImageStride,
    PixFmt, PixelFormat, Stride,
};
use image_iter::{ImageStride, ImageStrideMut};
use simple_frame::SimpleFrame;

type Result<T> = std::result::Result<T, Error>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("unimplemented pixel_format: {0:?}")]
    UnimplementedPixelFormat(PixFmt),
    #[error("unimplemented ROI width conversion")]
    UnimplementedRoiWidthConversion,
    #[error("ROI size exceeds original image")]
    RoiExceedsOriginal,
    #[error("invalid allocated buffer size")]
    InvalidAllocatedBufferSize {
        #[cfg(feature = "backtrace")]
        backtrace: std::backtrace::Backtrace,
    },
    #[error("invalid allocated buffer stride")]
    InvalidAllocatedBufferStride,
    #[error("{source}")]
    Bayer {
        #[from]
        source: wang_debayer::BayerError,
        #[cfg(feature = "backtrace")]
        backtrace: std::backtrace::Backtrace,
    },
    #[error("io error: {source}")]
    Io {
        #[from]
        source: std::io::Error,
        #[cfg(feature = "backtrace")]
        backtrace: std::backtrace::Backtrace,
    },
    #[error("{source}")]
    Image {
        #[from]
        source: image::ImageError,
        #[cfg(feature = "backtrace")]
        backtrace: std::backtrace::Backtrace,
    },
    #[error("unimplemented conversion {0} -> {1}")]
    UnimplementedConversion(PixFmt, PixFmt),
}

const EMPTY_BYTE: u8 = 128;

#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
#[derive(PartialEq, Eq, Debug)]
pub struct RGB888 {
    pub R: u8,
    pub G: u8,
    pub B: u8,
}

#[cfg(test)]
impl RGB888 {
    fn max_channel_distance(&self, other: &RGB888) -> i32 {
        let dr = (self.R as i32 - other.R as i32).abs();
        let dg = (self.G as i32 - other.G as i32).abs();
        let db = (self.B as i32 - other.B as i32).abs();

        let m1 = if dr > dg { dr } else { dg };

        if m1 > db {
            m1
        } else {
            db
        }
    }

    fn distance(&self, other: &RGB888) -> i32 {
        let dr = (self.R as i32 - other.R as i32).abs();
        let dg = (self.G as i32 - other.G as i32).abs();
        let db = (self.B as i32 - other.B as i32).abs();
        dr + dg + db
    }
}

#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
#[derive(PartialEq, Eq, Debug)]
pub struct YUV444 {
    pub Y: u8,
    pub U: u8,
    pub V: u8,
}

#[test]
fn test_f32_to_u8() {
    // Validate some conversion assumptions.
    assert_eq!(-1.0f32 as u8, 0u8);
    assert_eq!(-2.0f32 as u8, 0u8);
    assert_eq!(-100.0f32 as u8, 0u8);
    assert_eq!(255.0f32 as u8, 255u8);
    assert_eq!(255.1f32 as u8, 255u8);
    assert_eq!(255.8f32 as u8, 255u8);
    assert_eq!(5000.0f32 as u8, 255u8);
}

#[allow(non_snake_case)]
fn YUV444_bt601_toRGB(Y: u8, U: u8, V: u8) -> RGB888 {
    // See https://en.wikipedia.org/wiki/YCbCr
    let Y = Y as f32;
    let U = U as f32 - 128.0;
    let V = V as f32 - 128.0;

    let R = 1.0 * Y + 1.402 * V;
    let G = 1.0 * Y + -0.344136 * U + -0.714136 * V;
    let B = 1.0 * Y + 1.772 * U;

    RGB888 {
        R: R as u8,
        G: G as u8,
        B: B as u8,
    }
}

#[allow(non_snake_case)]
#[inline]
fn RGB888toYUV444_bt601_full_swing(R: u8, G: u8, B: u8) -> YUV444 {
    // See http://en.wikipedia.org/wiki/YUV and
    // https://en.wikipedia.org/wiki/YCbCr. Should we consider converting to f32
    // representation as in YUV444_bt601_toRGB above?
    let Y = RGB888toY4_bt601_full_swing(R, G, B);
    let R = R as i32;
    let G = G as i32;
    let B = B as i32;
    let U = ((-43 * R - 84 * G + 127 * B + 128) >> 8) + 128;
    let V = ((127 * R - 106 * G - 21 * B + 128) >> 8) + 128;
    YUV444 {
        Y,
        U: U as u8,
        V: V as u8,
    }
}

#[allow(non_snake_case)]
#[inline]
fn RGB888toY4_bt601_full_swing(R: u8, G: u8, B: u8) -> u8 {
    // See http://en.wikipedia.org/wiki/YUV and
    // https://en.wikipedia.org/wiki/YCbCr. Should we consider converting to f32
    // representation as in YUV444_bt601_toRGB above?
    let R = R as i32;
    let G = G as i32;
    let B = B as i32;
    let Y = (77 * R + 150 * G + 29 * B + 128) >> 8;
    Y as u8
}

/// Convert an input image to an RGB8 image.
pub fn piston_to_frame(
    piston_image: image::DynamicImage,
) -> Result<SimpleFrame<formats::pixel_format::RGB8>> {
    let rgb = piston_image.to_rgb8();
    let (width, height) = rgb.dimensions();
    let stride = width * 3;
    let data = rgb.into_vec();

    Ok(SimpleFrame::new(width, height, stride, data).unwrap())
}

/// Copy an YUV422 input image to a pre-allocated RGB8 buffer.
fn yuv422_into_rgb(
    src_yuv422: &dyn ImageStride<formats::pixel_format::YUV422>,
    dest_rgb: &mut dyn ImageStrideMut<RGB8>,
) -> Result<()> {
    // The destination must be at least this large per row.
    let min_stride = src_yuv422.width() as usize * PixFmt::RGB8.bits_per_pixel() as usize / 8;
    if dest_rgb.stride() < min_stride {
        return Err(Error::InvalidAllocatedBufferStride);
    }

    let expected_size = dest_rgb.stride() * src_yuv422.height() as usize;
    if dest_rgb.buffer_mut_ref().data.len() != expected_size {
        return Err(invalid_buf_size_err());
    }

    let w = src_yuv422.width() as usize;
    for (src_row, dest_row) in src_yuv422
        .rowchunks_exact()
        .zip(dest_rgb.rowchunks_exact_mut())
    {
        for (result_chunk, yuv422_pixpair) in dest_row[..(w * 3)]
            .chunks_exact_mut(6)
            .zip(src_row[..w * 2].chunks_exact(4))
        {
            let u = yuv422_pixpair[0];
            let y1 = yuv422_pixpair[1];
            let v = yuv422_pixpair[2];
            let y2 = yuv422_pixpair[3];

            let tmp_rgb1 = YUV444_bt601_toRGB(y1, u, v);
            let tmp_rgb2 = YUV444_bt601_toRGB(y2, u, v);

            result_chunk[0] = tmp_rgb1.R;
            result_chunk[1] = tmp_rgb1.G;
            result_chunk[2] = tmp_rgb1.B;

            result_chunk[3] = tmp_rgb2.R;
            result_chunk[4] = tmp_rgb2.G;
            result_chunk[5] = tmp_rgb2.B;
        }
    }
    Ok(())
}

fn into_yuv444<FMT>(
    frame: &dyn ImageStride<FMT>,
    dest: &mut ImageBufferMutRef<YUV444>,
    dest_stride: usize,
) -> Result<()>
where
    FMT: PixelFormat,
{
    let w = frame.width() as usize;
    // The destination must be at least this large per row.
    let min_stride = w * 3;
    if dest_stride < min_stride {
        return Err(Error::InvalidAllocatedBufferStride);
    }

    let expected_size = dest_stride * frame.height() as usize;
    if dest.data.len() != expected_size {
        return Err(invalid_buf_size_err());
    }

    // Convert to Mono8 or RGB8 TODO: if input encoding is YUV already, do
    // something better than this. We can assume that it won't be YUV444 because
    // we check for such no-op calls in `convert_into()`. But other YUV
    // encodings won't be caught there and we should avoid a round-trip through
    // RGB.
    let frame = to_rgb8_or_mono8(frame)?;

    match &frame {
        SupportedEncoding::Mono(mono) => {
            for (dest_row, src_row) in dest
                .data
                .chunks_exact_mut(dest_stride)
                .zip(mono.image_data().chunks_exact(mono.stride()))
            {
                for (dest_pixel, src_pixel) in
                    dest_row[..(w * 3)].chunks_exact_mut(3).zip(&src_row[..w])
                {
                    let yuv = YUV444 {
                        Y: *src_pixel,
                        U: 128,
                        V: 128,
                    };
                    dest_pixel[0] = yuv.Y;
                    dest_pixel[1] = yuv.U;
                    dest_pixel[2] = yuv.V;
                }
            }
        }
        SupportedEncoding::Rgb(rgb) => {
            for (dest_row, src_row) in dest
                .data
                .chunks_exact_mut(dest_stride)
                .zip(rgb.image_data().chunks_exact(rgb.stride()))
            {
                for (dest_pixel, src_pixel) in dest_row[..(w * 3)]
                    .chunks_exact_mut(3)
                    .zip(src_row[..(w * 3)].chunks_exact(3))
                {
                    let yuv =
                        RGB888toYUV444_bt601_full_swing(src_pixel[0], src_pixel[1], src_pixel[2]);
                    dest_pixel[0] = yuv.Y;
                    dest_pixel[1] = yuv.U;
                    dest_pixel[2] = yuv.V;
                }
            }
        }
    };
    Ok(())
}

/// Copy an input bayer image to a pre-allocated RGB8 buffer.
fn bayer_into_rgb<FMT>(
    frame: &dyn ImageStride<FMT>,
    dest_rgb: &mut dyn ImageStrideMut<RGB8>,
) -> Result<()>
where
    FMT: formats::PixelFormat,
{
    let dest_stride = dest_rgb.stride();

    if frame.stride() != frame.width() as usize {
        return Err(Error::UnimplementedRoiWidthConversion);
    }

    // The debayer code expects exactly this stride.
    let expected_stride = frame.width() as usize * PixFmt::RGB8.bits_per_pixel() as usize / 8;
    if dest_stride != expected_stride {
        return Err(Error::InvalidAllocatedBufferStride);
    }

    let src_fmt = machine_vision_formats::pixel_format::pixfmt::<FMT>().unwrap();

    let cfa = match src_fmt {
        formats::pixel_format::PixFmt::BayerRG8 => wang_debayer::CFA::RGGB,
        formats::pixel_format::PixFmt::BayerGB8 => wang_debayer::CFA::GBRG,
        formats::pixel_format::PixFmt::BayerGR8 => wang_debayer::CFA::GRBG,
        formats::pixel_format::PixFmt::BayerBG8 => wang_debayer::CFA::BGGR,
        _ => {
            return Err(Error::UnimplementedPixelFormat(src_fmt));
        }
    };

    use std::io::Cursor;

    {
        let mut dst = wang_debayer::RasterMut::new(
            frame.width() as usize,
            frame.height() as usize,
            wang_debayer::RasterDepth::Depth8,
            dest_rgb.buffer_mut_ref().data,
        );

        wang_debayer::run_demosaic(
            &mut Cursor::new(&frame.image_data()),
            wang_debayer::BayerDepth::Depth8,
            cfa,
            wang_debayer::Demosaic::Cubic,
            &mut dst,
        )?;
    }
    Ok(())
}

/// Copy an input mono8 image to a pre-allocated RGB8 buffer.
///
/// This copies the mono channel to each of the R, G and B channels.
fn mono8_into_rgb8(
    src: &dyn ImageStride<formats::pixel_format::Mono8>,
    dest_rgb: &mut dyn ImageStrideMut<RGB8>,
) -> Result<()> {
    let dest_stride = dest_rgb.stride();
    // The destination must be at least this large per row.
    let min_stride = src.width() as usize * PixFmt::RGB8.bits_per_pixel() as usize / 8;
    if dest_stride < min_stride {
        return Err(Error::InvalidAllocatedBufferStride);
    }

    let w = src.width() as usize;
    for (src_row, dest_row) in src.rowchunks_exact().zip(dest_rgb.rowchunks_exact_mut()) {
        for (dest_pix, src_pix) in dest_row[..(w * 3)].chunks_exact_mut(3).zip(&src_row[..w]) {
            dest_pix[0] = *src_pix;
            dest_pix[1] = *src_pix;
            dest_pix[2] = *src_pix;
        }
    }
    Ok(())
}

/// Copy an input rgba8 image to a pre-allocated RGB8 buffer.
fn rgba_into_rgb(
    frame: &dyn ImageStride<formats::pixel_format::RGBA8>,
    dest: &mut dyn ImageStrideMut<RGB8>,
) -> Result<()> {
    let dest_stride = dest.stride();

    // The destination must be at least this large per row.
    let min_stride = frame.width() as usize * PixFmt::RGB8.bits_per_pixel() as usize / 8;
    if dest_stride < min_stride {
        return Err(Error::InvalidAllocatedBufferStride);
    }

    let w = frame.width() as usize;
    for (src_row, dest_row) in frame.rowchunks_exact().zip(dest.rowchunks_exact_mut()) {
        for (dest_pix, src_pix) in dest_row[..(w * 3)]
            .chunks_exact_mut(3)
            .zip(src_row[..(w * 4)].chunks_exact(4))
        {
            dest_pix[0] = src_pix[0];
            dest_pix[1] = src_pix[1];
            dest_pix[2] = src_pix[2];
            // src_pix[3] is not used.
        }
    }
    Ok(())
}

/// Convert RGB8 image data into pre-allocated Mono8 buffer.
fn rgb8_into_mono8(
    frame: &dyn ImageStride<formats::pixel_format::RGB8>,
    dest: &mut dyn ImageStrideMut<Mono8>,
) -> Result<()> {
    if !(dest.height() == frame.height() && dest.width() == frame.width()) {
        return Err(invalid_buf_size_err());
    }

    let w = frame.width() as usize;
    for (src_row, dest_row) in frame.rowchunks_exact().zip(dest.rowchunks_exact_mut()) {
        let y_iter = src_row[..w * 3]
            .chunks_exact(3)
            .map(|rgb| RGB888toY4_bt601_full_swing(rgb[0], rgb[1], rgb[2]));

        let dest_iter = dest_row[0..w].iter_mut();

        for (ydest, y) in dest_iter.zip(y_iter) {
            *ydest = y;
        }
    }

    Ok(())
}

/// Convert YUV444 image data into pre-allocated Mono8 buffer.
fn yuv444_into_mono8(
    frame: &dyn ImageStride<formats::pixel_format::YUV444>,
    dest: &mut dyn ImageStrideMut<Mono8>,
) -> Result<()> {
    if !(dest.height() == frame.height() && dest.width() == frame.width()) {
        return Err(invalid_buf_size_err());
    }

    let w = frame.width() as usize;
    for (src_row, dest_row) in frame.rowchunks_exact().zip(dest.rowchunks_exact_mut()) {
        let y_iter = src_row[..w * 3].chunks_exact(3).map(|yuv444| yuv444[0]);

        let dest_iter = dest_row[0..w].iter_mut();

        for (ydest, y) in dest_iter.zip(y_iter) {
            *ydest = y;
        }
    }

    Ok(())
}

/// Convert NV12 image data into pre-allocated Mono8 buffer.
fn nv12_into_mono8(
    frame: &dyn ImageStride<formats::pixel_format::NV12>,
    dest: &mut dyn ImageStrideMut<Mono8>,
) -> Result<()> {
    if !(dest.height() == frame.height() && dest.width() == frame.width()) {
        return Err(invalid_buf_size_err());
    }

    for (src_row, dest_row) in frame.rowchunks_exact().zip(dest.rowchunks_exact_mut()) {
        dest_row[..frame.width() as usize].copy_from_slice(&src_row[..frame.width() as usize]);
    }

    Ok(())
}

/// A view of image to have pixel format `FMT2`.
pub struct ReinterpretedImage<'a, FMT1, FMT2> {
    orig: &'a dyn ImageStride<FMT1>,
    fmt: std::marker::PhantomData<FMT2>,
}

impl<'a, FMT1, FMT2> ImageData<FMT2> for ReinterpretedImage<'a, FMT1, FMT2> {
    fn width(&self) -> u32 {
        self.orig.width()
    }
    fn height(&self) -> u32 {
        self.orig.height()
    }
    fn buffer_ref(&self) -> ImageBufferRef<'_, FMT2> {
        ImageBufferRef::new(self.orig.image_data())
    }
    fn buffer(self) -> ImageBuffer<FMT2> {
        // copy the data
        self.buffer_ref().to_buffer()
    }
}

impl<'a, FMT1, FMT2> Stride for ReinterpretedImage<'a, FMT1, FMT2> {
    fn stride(&self) -> usize {
        self.orig.stride()
    }
}

/// A view of mutable image to have pixel format `FMT2`.
struct ReinterpretedImageMut<'a, FMT1, FMT2> {
    orig: &'a mut dyn ImageStrideMut<FMT1>,
    fmt: std::marker::PhantomData<FMT2>,
}

impl<'a, FMT1, FMT2> ImageData<FMT2> for ReinterpretedImageMut<'a, FMT1, FMT2> {
    fn width(&self) -> u32 {
        self.orig.width()
    }
    fn height(&self) -> u32 {
        self.orig.height()
    }
    fn buffer_ref(&self) -> ImageBufferRef<'_, FMT2> {
        ImageBufferRef::new(self.orig.image_data())
    }
    fn buffer(self) -> ImageBuffer<FMT2> {
        // copy the data
        self.buffer_ref().to_buffer()
    }
}

impl<'a, FMT1, FMT2> ImageMutData<FMT2> for ReinterpretedImageMut<'a, FMT1, FMT2> {
    fn buffer_mut_ref(&mut self) -> ImageBufferMutRef<'_, FMT2> {
        ImageBufferMutRef::new(self.orig.buffer_mut_ref().data)
    }
}

impl<'a, FMT1, FMT2> Stride for ReinterpretedImageMut<'a, FMT1, FMT2> {
    fn stride(&self) -> usize {
        self.orig.stride()
    }
}

/// If needed, copy original image data to remove stride.
fn remove_padding<FMT>(frame: &dyn ImageStride<FMT>) -> Result<CowImage<'_, FMT, FMT>>
where
    FMT: PixelFormat,
{
    let fmt = machine_vision_formats::pixel_format::pixfmt::<FMT>().unwrap();
    let bytes_per_pixel = fmt.bits_per_pixel() as usize / 8;
    let dest_stride = frame.width() as usize * bytes_per_pixel;
    if dest_stride == frame.stride() {
        Ok(CowImage::Reinterpreted(force_pixel_format_ref(frame)))
    } else {
        if frame.stride() < dest_stride {
            return Err(Error::InvalidAllocatedBufferStride);
        }
        // allocate output
        let mut dest_buf = vec![0u8; frame.height() as usize * dest_stride];
        // trim input slice to height
        frame
            .rowchunks_exact()
            .zip(dest_buf.chunks_exact_mut(dest_stride))
            .for_each(|(src_row_full, dest_row)| {
                dest_row[..dest_stride].copy_from_slice(&src_row_full[..dest_stride]);
            });
        // Return the new buffer as a new image.
        Ok(CowImage::Owned(
            SimpleFrame::new(frame.width(), frame.height(), dest_stride as u32, dest_buf).unwrap(),
        ))
    }
}

/// An RoiImage maintains a reference to the original image but views a
/// subregion of the original data.
pub struct RoiImage<'a, F> {
    data: &'a [u8],
    w: u32,
    h: u32,
    stride: usize,
    fmt: std::marker::PhantomData<F>,
}

impl<'a, F> RoiImage<'a, F>
where
    F: PixelFormat,
{
    /// Create a new `RoiImage` referencing the original `frame`.
    pub fn new(
        frame: &'a dyn ImageStride<F>,
        w: u32,
        h: u32,
        x: u32,
        y: u32,
    ) -> Result<RoiImage<'a, F>> {
        let stride = frame.stride();
        let fmt = machine_vision_formats::pixel_format::pixfmt::<F>().unwrap();
        let col_offset = x as usize * fmt.bits_per_pixel() as usize / 8;
        if col_offset >= stride {
            return Err(Error::RoiExceedsOriginal);
        }
        let offset = y as usize * stride + col_offset;
        Ok(RoiImage {
            data: &frame.image_data()[offset..],
            w,
            h,
            stride,
            fmt: std::marker::PhantomData,
        })
    }
}

impl<'a, F> Stride for RoiImage<'a, F> {
    fn stride(&self) -> usize {
        self.stride
    }
}

impl<'a, F> ImageData<F> for RoiImage<'a, F> {
    fn width(&self) -> u32 {
        self.w
    }
    fn height(&self) -> u32 {
        self.h
    }
    fn buffer_ref(&self) -> ImageBufferRef<'_, F> {
        let image_data = self.data;
        ImageBufferRef::new(image_data)
    }
    fn buffer(self) -> ImageBuffer<F> {
        let copied = self.data.to_vec();
        ImageBuffer::new(copied)
    }
}

enum CowImage<'a, F, FORIG> {
    Reinterpreted(ReinterpretedImage<'a, FORIG, F>),
    Owned(SimpleFrame<F>),
}

impl<'a, F, FORIG> From<ReinterpretedImage<'a, FORIG, F>> for CowImage<'a, F, FORIG> {
    fn from(frame: ReinterpretedImage<'a, FORIG, F>) -> CowImage<'a, F, FORIG> {
        CowImage::Reinterpreted(frame)
    }
}

impl<'a, F, FORIG> From<SimpleFrame<F>> for CowImage<'a, F, FORIG> {
    fn from(frame: SimpleFrame<F>) -> CowImage<'a, F, FORIG> {
        CowImage::Owned(frame)
    }
}

impl<'a, F, FORIG> Stride for CowImage<'a, F, FORIG> {
    fn stride(&self) -> usize {
        match self {
            CowImage::Reinterpreted(im) => im.stride(),
            CowImage::Owned(im) => im.stride(),
        }
    }
}

impl<'a, F, FORIG> ImageData<F> for CowImage<'a, F, FORIG> {
    fn width(&self) -> u32 {
        match self {
            CowImage::Reinterpreted(im) => im.width(),
            CowImage::Owned(im) => im.width(),
        }
    }
    fn height(&self) -> u32 {
        match self {
            CowImage::Reinterpreted(im) => im.height(),
            CowImage::Owned(im) => im.height(),
        }
    }
    fn buffer_ref(&self) -> ImageBufferRef<'_, F> {
        let image_data = match self {
            CowImage::Reinterpreted(im) => im.image_data(),
            CowImage::Owned(im) => im.image_data(),
        };
        ImageBufferRef::new(image_data)
    }
    fn buffer(self) -> ImageBuffer<F> {
        match self {
            CowImage::Reinterpreted(im) => ImageBuffer::new(im.image_data().to_vec()),
            CowImage::Owned(im) => ImageBuffer::new(im.into()),
        }
    }
}

/// Force interpretation of data from frame into another pixel_format.
///
/// This moves the data. See `force_pixel_format_ref()` for a function which
/// makes a view of the original data.
pub fn force_pixel_format<FRAME, FMT1, FMT2>(frame: FRAME) -> SimpleFrame<FMT2>
where
    FRAME: OwnedImageStride<FMT1>,
    FMT2: PixelFormat,
{
    let width = frame.width();
    let height = frame.height();
    let stride = frame.stride() as u32;
    let image_data = frame.into(); // Move the original data.

    SimpleFrame::new(width, height, stride, image_data).unwrap()
}

/// Force interpretation of data from frame into another pixel_format.
///
/// This makes a view of the original data. See `force_pixel_format()` for a
/// function which consumes the original data and moves it into the output.
fn force_pixel_format_ref<'a, FMT1, FMT2>(
    frame: &'a dyn ImageStride<FMT1>,
) -> ReinterpretedImage<'a, FMT1, FMT2>
where
    FMT1: 'a,
    FMT2: 'a,
{
    ReinterpretedImage {
        orig: frame,
        fmt: std::marker::PhantomData,
    }
}

// /// Force interpretation of data from frame into another pixel_format.
// fn force_buffer_pixel_format_ref<'a, 'b, FMT1, FMT2>(
//     orig: &'b mut ImageBufferMutRef<'a, FMT1>,
// ) -> ImageBufferMutRef<'b, FMT2>
// where
//     FMT1: 'a,
//     FMT2: 'b,
// {
//     ImageBufferMutRef::new(orig.data)
// }

/// Force interpretation of data from frame into another pixel_format.
fn force_buffer_pixel_format_ref<FMT1, FMT2>(
    orig: ImageBufferMutRef<'_, FMT1>,
) -> ImageBufferMutRef<'_, FMT2> {
    ImageBufferMutRef::new(orig.data)
}

/// Convert input frame with pixel_format `SRC` into pixel_format `DEST`
///
/// This is a general purpose function which should be able to convert between
/// many types as efficiently as possible. In case no data needs to be copied,
/// no data is copied.
///
/// For a version which converts into a pre-allocated buffer, use `convert_into`
/// (which will copy the image even if the format remains unchanged).
pub fn convert_owned<OWNED, SRC, DEST>(frame: OWNED) -> Result<impl ImageStride<DEST>>
where
    OWNED: OwnedImageStride<SRC>,
    SRC: PixelFormat,
    DEST: PixelFormat,
{
    let src_fmt = machine_vision_formats::pixel_format::pixfmt::<SRC>().unwrap();
    let dest_fmt = machine_vision_formats::pixel_format::pixfmt::<DEST>().unwrap();

    // If format does not change, move original data without copy.
    if src_fmt == dest_fmt {
        let width = frame.width();
        let height = frame.height();
        let stride = frame.stride();
        let image_data: Vec<u8> = frame.into();
        let dest = SimpleFrame::new(width, height, stride as u32, image_data).unwrap();
        return Ok(CowImage::Owned::<_, SRC>(dest));
    }

    // Allocate minimal size buffer for new image.
    let dest_min_stride = dest_fmt.bits_per_pixel() as usize * frame.width() as usize / 8;
    let dest_size = frame.height() as usize * dest_min_stride;
    let image_data = vec![0u8; dest_size];
    let mut dest = SimpleFrame::new(
        frame.width(),
        frame.height(),
        dest_min_stride as u32,
        image_data,
    )
    .unwrap();

    // Fill the new buffer.
    convert_into(&frame, &mut dest)?;

    // Return the new buffer as a new image.
    Ok(CowImage::Owned(dest))
}

/// Convert input frame with pixel_format `SRC` into pixel_format `DEST`
///
/// This is a general purpose function which should be able to convert between
/// many types as efficiently as possible. In case no data needs to be copied,
/// no data is copied.
///
/// For a version which converts into a pre-allocated buffer, use `convert_into`
/// (which will copy the image even if the format remains unchanged).
pub fn convert<SRC, DEST>(frame: &dyn ImageStride<SRC>) -> Result<impl ImageStride<DEST> + '_>
where
    SRC: PixelFormat,
    DEST: PixelFormat,
{
    let src_fmt = machine_vision_formats::pixel_format::pixfmt::<SRC>().unwrap();
    let dest_fmt = machine_vision_formats::pixel_format::pixfmt::<DEST>().unwrap();

    // If format does not change, return reference to original image without copy.
    if src_fmt == dest_fmt {
        return Ok(CowImage::Reinterpreted(force_pixel_format_ref(frame)));
    }

    // Allocate minimal size buffer for new image.
    let dest_min_stride = dest_fmt.bits_per_pixel() as usize * frame.width() as usize / 8;
    let dest_size = frame.height() as usize * dest_min_stride;
    let image_data = vec![0u8; dest_size];
    let mut dest = SimpleFrame::new(
        frame.width(),
        frame.height(),
        dest_min_stride as u32,
        image_data,
    )
    .unwrap();

    // Fill the new buffer.
    convert_into(frame, &mut dest)?;

    // Return the new buffer as a new image.
    Ok(CowImage::Owned(dest))
}

/// Convert input frame with pixel_format `SRC` into pixel_format `DEST`
///
/// This is a general purpose function which should be able to convert between
/// many types as efficiently as possible.
pub fn convert_into<SRC, DEST>(
    frame: &dyn ImageStride<SRC>,
    dest: &mut dyn ImageStrideMut<DEST>,
) -> Result<()>
where
    SRC: PixelFormat,
    DEST: PixelFormat,
{
    let src_fmt = machine_vision_formats::pixel_format::pixfmt::<SRC>().unwrap();
    let dest_fmt = machine_vision_formats::pixel_format::pixfmt::<DEST>().unwrap();

    let dest_stride = dest.stride();

    // If format does not change, copy the data row-by-row to respect strides.
    if src_fmt == dest_fmt {
        let dest_size = frame.height() as usize * dest_stride;
        if dest.buffer_mut_ref().data.len() != dest_size {
            return Err(invalid_buf_size_err());
        }

        use itertools::izip;
        let w = frame.width() as usize;
        let nbytes = dest_fmt.bits_per_pixel() as usize * w / 8;
        for (src_row, dest_row) in izip![
            frame.image_data().chunks_exact(frame.stride()),
            dest.buffer_mut_ref().data.chunks_exact_mut(dest_stride),
        ] {
            dest_row[..nbytes].copy_from_slice(&src_row[..nbytes]);
        }
    }

    match dest_fmt {
        formats::pixel_format::PixFmt::RGB8 => {
            let mut dest_rgb = ReinterpretedImageMut {
                orig: dest,
                fmt: std::marker::PhantomData,
            };
            // Convert to RGB8..
            match src_fmt {
                formats::pixel_format::PixFmt::BayerRG8
                | formats::pixel_format::PixFmt::BayerGB8
                | formats::pixel_format::PixFmt::BayerGR8
                | formats::pixel_format::PixFmt::BayerBG8 => {
                    // .. from bayer.
                    // The bayer code requires no padding in the input image.
                    let exact_stride = remove_padding(frame)?;
                    bayer_into_rgb(&exact_stride, &mut dest_rgb)?;
                    Ok(())
                }
                formats::pixel_format::PixFmt::Mono8 => {
                    // .. from mono8.
                    let mono8 = force_pixel_format_ref(frame);
                    mono8_into_rgb8(&mono8, &mut dest_rgb)?;
                    Ok(())
                }
                formats::pixel_format::PixFmt::RGBA8 => {
                    // .. from rgba8.
                    let rgba8 = force_pixel_format_ref(frame);
                    rgba_into_rgb(&rgba8, &mut dest_rgb)?;
                    Ok(())
                }
                formats::pixel_format::PixFmt::YUV422 => {
                    // .. from YUV422.
                    let yuv422 = force_pixel_format_ref(frame);
                    yuv422_into_rgb(&yuv422, &mut dest_rgb)?;
                    Ok(())
                }
                _ => Err(Error::UnimplementedConversion(src_fmt, dest_fmt)),
            }
        }
        formats::pixel_format::PixFmt::Mono8 => {
            let mut dest_mono8 = ReinterpretedImageMut {
                orig: dest,
                fmt: std::marker::PhantomData,
            };
            // Convert to Mono8..
            match src_fmt {
                formats::pixel_format::PixFmt::RGB8 => {
                    // .. from RGB8.
                    let tmp = force_pixel_format_ref(frame);
                    {
                        rgb8_into_mono8(&tmp, &mut dest_mono8)?;
                    }
                    Ok(())
                }
                formats::pixel_format::PixFmt::YUV444 => {
                    // .. from YUV444.
                    let yuv444 = force_pixel_format_ref(frame);
                    // let mut mono8 = force_buffer_pixel_format_ref(&mut dest.buffer_mut_ref());
                    yuv444_into_mono8(&yuv444, &mut dest_mono8)?;
                    Ok(())
                }
                formats::pixel_format::PixFmt::NV12 => {
                    // .. from NV12.
                    let nv12 = force_pixel_format_ref(frame);
                    nv12_into_mono8(&nv12, &mut dest_mono8)?;
                    Ok(())
                }
                _ => Err(Error::UnimplementedConversion(src_fmt, dest_fmt)),
            }
        }
        formats::pixel_format::PixFmt::YUV444 => {
            // Convert to YUV444.
            // let mut dest2 = force_buffer_pixel_format_ref(&mut dest.buffer_mut_ref());
            let mut dest2 = force_buffer_pixel_format_ref(dest.buffer_mut_ref());
            into_yuv444(frame, &mut dest2, dest_stride)?;
            Ok(())
        }
        formats::pixel_format::PixFmt::NV12 => {
            // Convert to NV12.
            let mut dest2 = force_buffer_pixel_format_ref(dest.buffer_mut_ref());
            encode_into_nv12_inner(frame, &mut dest2, dest_stride)?;
            Ok(())
        }
        _ => Err(Error::UnimplementedConversion(src_fmt, dest_fmt)),
    }
}

/// An image which can be directly encoded as RGB8 or Mono8
///
/// This nearly supports the ImageStride trait, but we avoid it because it has a
/// type parameter specifying the pixel format, whereas we don't use that here
/// and instead explicitly represent an image with one of two possible pixel
/// formats.
enum SupportedEncoding<'a> {
    Rgb(Box<dyn ImageStride<formats::pixel_format::RGB8> + 'a>),
    Mono(Box<dyn ImageStride<formats::pixel_format::Mono8> + 'a>),
}

impl<'a> SupportedEncoding<'a> {
    #[inline]
    fn width(&self) -> u32 {
        match self {
            SupportedEncoding::Rgb(m) => m.width(),
            SupportedEncoding::Mono(m) => m.width(),
        }
    }
    #[inline]
    fn height(&self) -> u32 {
        match self {
            SupportedEncoding::Rgb(m) => m.height(),
            SupportedEncoding::Mono(m) => m.height(),
        }
    }
    #[inline]
    fn stride(&self) -> usize {
        match self {
            SupportedEncoding::Rgb(m) => m.stride(),
            SupportedEncoding::Mono(m) => m.stride(),
        }
    }
    #[inline]
    fn image_data(&self) -> &[u8] {
        match self {
            SupportedEncoding::Rgb(m) => m.image_data(),
            SupportedEncoding::Mono(m) => m.image_data(),
        }
    }
}

/// If the input is Mono8, return as Mono8, otherwise, return as RGB8.
fn to_rgb8_or_mono8<FMT>(frame: &dyn ImageStride<FMT>) -> Result<SupportedEncoding<'_>>
where
    FMT: PixelFormat,
{
    if machine_vision_formats::pixel_format::pixfmt::<FMT>().unwrap()
        == formats::pixel_format::PixFmt::Mono8
    {
        let im = convert::<_, formats::pixel_format::Mono8>(frame)?;
        Ok(SupportedEncoding::Mono(Box::new(im)))
    } else {
        let im = convert::<_, formats::pixel_format::RGB8>(frame)?;
        Ok(SupportedEncoding::Rgb(Box::new(im)))
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ImageOptions {
    Jpeg(u8),
    Png,
}

/// Convert any type implementing `ImageStride<FMT>` to a Jpeg or Png buffer.
pub fn frame_to_image<FMT>(frame: &dyn ImageStride<FMT>, opts: ImageOptions) -> Result<Vec<u8>>
where
    FMT: PixelFormat,
{
    let mut result = Vec::new();

    let frame = to_rgb8_or_mono8(frame)?;

    let (coding, bytes_per_pixel) = match &frame {
        SupportedEncoding::Mono(_) => (image::ColorType::L8, 1),
        SupportedEncoding::Rgb(_) => (image::ColorType::Rgb8, 3),
    };

    // The encoders in the `image` crate only handle packed inputs. We check if
    // our data is packed and if not, make a packed copy.

    let mut packed = None;
    let packed_stride = frame.width() as usize * bytes_per_pixel as usize;
    if frame.stride() != packed_stride {
        let mut dest = Vec::with_capacity(packed_stride * frame.height() as usize);
        let src = frame.image_data();
        let chunk_iter = src.chunks_exact(frame.stride());
        if !chunk_iter.remainder().is_empty() {
            return Err(invalid_buf_size_err());
        }
        for src_row in chunk_iter {
            dest.extend_from_slice(&src_row[..packed_stride]);
        }
        packed = Some(dest);
    }

    let use_frame = match &packed {
        None => frame.image_data(),
        Some(p) => p.as_slice(),
    };

    match opts {
        ImageOptions::Jpeg(quality) => {
            let mut encoder =
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut result, quality);
            encoder.encode(use_frame, frame.width(), frame.height(), coding)?;
        }
        ImageOptions::Png => {
            use image::ImageEncoder;
            let encoder = image::codecs::png::PngEncoder::new(&mut result);
            encoder.write_image(use_frame, frame.width(), frame.height(), coding)?;
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use crate::*;

    fn imstr<F>(frame: &dyn ImageStride<F>) -> String
    where
        F: PixelFormat,
    {
        let fmt = machine_vision_formats::pixel_format::pixfmt::<F>().unwrap();
        let bytes_per_pixel = fmt.bits_per_pixel() as usize / 8;

        frame
            .rowchunks_exact()
            .map(|row| {
                let image_row = &row[..frame.width() as usize * bytes_per_pixel];
                image_row
                    .chunks_exact(fmt.bits_per_pixel() as usize / 8)
                    .map(|x| format!("{:?}", x))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn check_roi_mono8() {
        // Create an image where stride is larger than width.
        const STRIDE: usize = 10;
        const W: u32 = 8;
        const H: u32 = 6;
        // Create buffer with value of `255` everywhere.
        let mut image_data = vec![255; H as usize * STRIDE];
        // Set the image part of the buffer to `col*H + row`.
        for row in 0..H as usize {
            let start_idx = row * STRIDE;
            for col in 0..W as usize {
                image_data[start_idx + col] = (row * W as usize + col) as u8;
            }
        }
        let frame: SimpleFrame<formats::pixel_format::Mono8> =
            SimpleFrame::new(W, H, STRIDE as u32, image_data).unwrap();
        println!("frame: {:?}", frame.image_data());
        println!("frame: \n{}", imstr(&frame));
        let roi = RoiImage::new(&frame, 6, 2, 1, 1).unwrap();
        println!("roi: {:?}", roi.image_data());
        println!("roi: \n{}", imstr(&roi));
        let small = super::remove_padding(&roi).unwrap();
        println!("small: {:?}", small.image_data());
        println!("small: \n{}", imstr(&small));
        assert_eq!(
            small.image_data(),
            &[9, 10, 11, 12, 13, 14, 17, 18, 19, 20, 21, 22]
        );
    }

    #[test]
    fn check_roi_rgb8() {
        // Create an image where stride is larger than width.
        const STRIDE: usize = 30;
        const W: u32 = 8;
        const H: u32 = 6;
        // Create buffer with value of `255` everywhere.
        let mut image_data = vec![255; H as usize * STRIDE];
        for row in 0..H as usize {
            let start_idx = row * STRIDE;
            for col in 0..W as usize {
                let col_offset = col * 3;
                image_data[start_idx + col_offset] = ((row * W as usize + col) * 3) as u8;
                image_data[start_idx + col_offset + 1] = ((row * W as usize + col) * 3) as u8 + 1;
                image_data[start_idx + col_offset + 2] = ((row * W as usize + col) * 3) as u8 + 2;
            }
        }
        let frame: SimpleFrame<formats::pixel_format::RGB8> =
            SimpleFrame::new(W, H, STRIDE as u32, image_data).unwrap();
        println!("frame: {:?}", frame.image_data());
        println!("frame: \n{}", imstr(&frame));
        let roi = RoiImage::new(&frame, 6, 2, 1, 1).unwrap();
        println!("roi: {:?}", roi.image_data());
        println!("roi: \n{}", imstr(&roi));
        let small = super::remove_padding(&roi).unwrap();
        println!("small: {:?}", small.image_data());
        println!("small: \n{}", imstr(&small));
        assert_eq!(
            small.image_data(),
            &[
                27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 51, 52, 53,
                54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68
            ]
        );
    }
    #[test]
    fn check_stride_conversion_to_image() {
        // Create an image where stride is larger than width.
        const STRIDE: usize = 6;
        const W: u32 = 4;
        const H: u32 = 2;
        // Create buffer with value of `42` everywhere.
        let mut image_data = vec![42; H as usize * STRIDE];
        // Set the image part of the buffer to `0`.
        for row in 0..H as usize {
            let start_idx = row * STRIDE;
            for col in 0..W as usize {
                image_data[start_idx + col] = 0;
            }
        }
        let frame: SimpleFrame<formats::pixel_format::Mono8> =
            SimpleFrame::new(W, H, STRIDE as u32, image_data).unwrap();
        let buf = frame_to_image(&frame, ImageOptions::Png).unwrap();

        // Decode the BMP data into an image.
        let im2 = image::load_from_memory_with_format(&buf, image::ImageFormat::Png).unwrap();

        let im2gray = im2.into_luma8();
        assert_eq!(im2gray.width(), W);
        assert_eq!(im2gray.height(), H);
        for row in 0..H {
            for col in 0..W {
                let pixel = im2gray.get_pixel(col, row);
                assert_eq!(pixel.0[0], 0, "at pixel {},{}", col, row);
            }
        }
    }

    #[test]
    fn check_bayer_conversion_to_jpg() {
        // Create an image where stride is larger than width.
        const STRIDE: usize = 6;
        const W: u32 = 4;
        const H: u32 = 4;
        // Create buffer with value of `42` everywhere.
        let mut image_data = vec![42; H as usize * STRIDE];
        // Set the image part of the buffer to `0`.
        for row in 0..H as usize {
            let start_idx = row * STRIDE;
            for col in 0..W as usize {
                image_data[start_idx + col] = 0;
            }
        }
        let frame: SimpleFrame<formats::pixel_format::BayerRG8> =
            SimpleFrame::new(W, H, STRIDE as u32, image_data).unwrap();
        frame_to_image(&frame, ImageOptions::Jpeg(240)).unwrap();
    }

    #[test]
    fn prevent_unnecessary_copy_mono8() {
        let frame: SimpleFrame<formats::pixel_format::Mono8> =
            SimpleFrame::new(10, 10, 10, vec![42; 100]).unwrap();
        // `im2` has only a reference to original data.
        let im2 = convert::<_, formats::pixel_format::Mono8>(&frame).unwrap();
        // Confirm the data are correct.
        assert_eq!(im2.image_data(), frame.image_data());

        // Now get a pointer to the original data.
        let const_ptr = frame.image_data().as_ptr();
        // Make it mutable.
        let data_ptr = const_ptr as *mut u8;
        // Now edit the original data.
        unsafe {
            *data_ptr = 2;
        }
        // And confirm that `im2` now will also have the new value;
        assert_eq!(im2.image_data()[0], 2);
    }

    #[test]
    fn prevent_unnecessary_copy_rgb8() {
        let frame: SimpleFrame<formats::pixel_format::RGB8> =
            SimpleFrame::new(10, 10, 30, vec![42; 300]).unwrap();
        // `im2` has only a reference to original data.
        let im2 = convert::<_, formats::pixel_format::RGB8>(&frame).unwrap();
        // Confirm the data are correct.
        assert_eq!(im2.image_data(), frame.image_data());

        // Now get a pointer to the original data.
        let const_ptr = frame.image_data().as_ptr();
        // Make it mutable.
        let data_ptr = const_ptr as *mut u8;
        // Now edit the original data.
        unsafe {
            *data_ptr = 2;
        }
        // And confirm that `im2` now will also have the new value;
        assert_eq!(im2.image_data()[0], 2);
    }

    #[test]
    fn test_rgb_yuv_roundtrip() {
        // Related: reversible color transforms (e.g. YCoCg):
        // https://stackoverflow.com/questions/10566668/lossless-rgb-to-ycbcr-transformation
        let black_rgb = RGB888 { R: 0, G: 0, B: 0 };
        let black_yuv = RGB888toYUV444_bt601_full_swing(black_rgb.R, black_rgb.G, black_rgb.B);
        let black_rgb2 = YUV444_bt601_toRGB(black_yuv.Y, black_yuv.U, black_yuv.V);
        assert_eq!(black_rgb, black_rgb2);

        let white_rgb = RGB888 {
            R: 255,
            G: 255,
            B: 255,
        };
        let white_yuv = RGB888toYUV444_bt601_full_swing(white_rgb.R, white_rgb.G, white_rgb.B);
        let white_rgb2 = YUV444_bt601_toRGB(white_yuv.Y, white_yuv.U, white_yuv.V);
        assert_eq!(white_rgb, white_rgb2);

        for r in 0..255 {
            for g in 0..255 {
                for b in 0..255 {
                    let expected = RGB888 { R: r, G: g, B: b };
                    let yuv = RGB888toYUV444_bt601_full_swing(expected.R, expected.G, expected.B);
                    let actual = YUV444_bt601_toRGB(yuv.Y, yuv.U, yuv.V);
                    assert!(
                        actual.distance(&expected) <= 7,
                        "expected: {:?}, actual: {:?}",
                        expected,
                        actual
                    );
                    assert!(
                        actual.max_channel_distance(&expected) <= 4,
                        "expected: {:?}, actual: {:?}",
                        expected,
                        actual
                    );
                }
            }
        }
    }

    #[test]
    // Test MONO8->RGB8
    fn test_mono8_rgb8() -> Result<()> {
        let orig: SimpleFrame<formats::pixel_format::Mono8> =
            SimpleFrame::new(256, 1, 256, (0u8..=255u8).collect()).unwrap();
        let rgb = convert::<_, formats::pixel_format::RGB8>(&orig)?;
        for (i, rgb_pix) in rgb.image_data().chunks_exact(3).enumerate() {
            assert_eq!(i, rgb_pix[0] as usize);
            assert_eq!(i, rgb_pix[1] as usize);
            assert_eq!(i, rgb_pix[2] as usize);
        }
        Ok(())
    }

    #[test]
    fn test_mono8_rgb_roundtrip() -> Result<()> {
        let orig: SimpleFrame<formats::pixel_format::Mono8> =
            SimpleFrame::new(256, 1, 256, (0u8..=255u8).collect()).unwrap();
        let rgb = convert::<_, formats::pixel_format::RGB8>(&orig)?;
        let actual = convert::<_, formats::pixel_format::Mono8>(&rgb)?;
        assert_eq!(orig.image_data(), actual.image_data());
        Ok(())
    }

    #[test]
    fn test_mono8_nv12_roundtrip() -> Result<()> {
        let orig: SimpleFrame<formats::pixel_format::Mono8> =
            SimpleFrame::new(256, 1, 256, (0u8..=255u8).collect()).unwrap();
        let nv12 = convert::<_, formats::pixel_format::NV12>(&orig)?;
        let actual = convert::<_, formats::pixel_format::Mono8>(&nv12)?;
        for i in 0..256 {
            assert_eq!(orig.image_data()[i], actual.image_data()[i]);
        }
        assert_eq!(orig.image_data(), actual.image_data());
        Ok(())
    }

    #[test]
    // Test MONO8->YUV444->MONO8.
    fn test_mono8_yuv_roundtrip() -> Result<()> {
        let orig: SimpleFrame<formats::pixel_format::Mono8> =
            SimpleFrame::new(256, 1, 256, (0u8..=255u8).collect()).unwrap();
        let yuv = convert::<_, formats::pixel_format::YUV444>(&orig)?;
        let actual = convert::<_, formats::pixel_format::Mono8>(&yuv)?;
        assert_eq!(orig.image_data(), actual.image_data());
        Ok(())
    }
}

/// Defines the colorspace used by the [encode_y4m_frame] function.
#[derive(Debug, Clone, Copy)]
pub enum Y4MColorspace {
    /// luminance
    ///
    /// WARNING: Not compatible with much software, not in spec.
    CMono,
    /// 4:2:0 with vertically-displaced chroma planes
    C420paldv,
    // /// 4:4:4
    // C444,
}

impl std::str::FromStr for Y4MColorspace {
    type Err = &'static str;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "Mono" | "mono" => Ok(Y4MColorspace::CMono),
            "C420paldv" | "420paldv" => Ok(Y4MColorspace::C420paldv),
            // "C444" | "444" => Ok(Y4MColorspace::C444),
            _ => Err("unknown colorspace"),
        }
    }
}

impl std::fmt::Display for Y4MColorspace {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::result::Result<(), std::fmt::Error> {
        match self {
            Y4MColorspace::CMono => write!(f, "mono"),
            Y4MColorspace::C420paldv => write!(f, "420paldv"),
            // &Y4MColorspace::C444 => write!(f, "444"),
        }
    }
}

/// YUV420 planar data
pub struct Y4MFrame {
    pub data: Vec<u8>,
    pub width: i32,
    pub height: i32,
    pub y_stride: i32,
    colorspace: Y4MColorspace,
    chroma_stride: usize,
    alloc_rows: i32,
    alloc_chroma_rows: i32,
    /// True if the U and V planes are known to contain no data.
    is_known_mono_only: bool,
    forced_block_size: Option<u32>,
}

impl std::fmt::Debug for Y4MFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Y4MFrame{{width: {}, height: {}, y_stride: {}, chroma_stride: {}, data.len(): {}, alloc_rows: {}, alloc_chroma_rows: {}, is_known_mono_only: {}, forced_block_size: {:?}}}",
            self.width, self.height, self.y_stride, self.chroma_stride, self.data.len(), self.alloc_rows, self.alloc_chroma_rows, self.is_known_mono_only, self.forced_block_size)
    }
}

impl Y4MFrame {
    #[allow(clippy::too_many_arguments)]
    fn new(
        data: Vec<u8>,
        width: u32,
        height: u32,
        stride: i32,
        chroma_stride: usize,
        alloc_rows: i32,
        alloc_chroma_rows: i32,
        is_known_mono_only: bool,
        forced_block_size: Option<u32>,
        colorspace: Y4MColorspace,
    ) -> Self {
        let width: i32 = width.try_into().unwrap();
        let height: i32 = height.try_into().unwrap();
        let y_stride = stride;

        if let Some(sz) = forced_block_size {
            debug_assert_eq!(y_stride % sz as i32, 0);
            debug_assert_eq!(chroma_stride % sz as usize, 0);
        }

        Self {
            data,
            width,
            height,
            y_stride,
            colorspace,
            chroma_stride,
            alloc_rows,
            alloc_chroma_rows,
            is_known_mono_only,
            forced_block_size,
        }
    }

    pub fn convert<DEST>(&self) -> Result<impl ImageStride<DEST>>
    where
        DEST: PixelFormat,
    {
        let y_data = self.y_plane_data();

        match &self.colorspace {
            Y4MColorspace::C420paldv => {
                // // Convert from color data RGB8.

                // // TODO: implement shortcut when DEST is Mono8.

                // // Instead of iterating smartly, just fill to 444.
                // fn expand_plane(small: &[u8], small_stride: usize) -> Vec<u8> {
                //     let small_rows = small.len() / small_stride;
                //     let full_rows = small_rows * 2;
                //     let full_stride = small_stride * 2;
                //     let full_len = full_rows * full_stride;
                //     let mut result = vec![0u8; full_len];

                //     for (small_row_num, small_row) in small.chunks_exact(small_stride).enumerate() {
                //         let big_row_num = small_row_num * 2;

                //         for (small_col, val) in small_row.iter().enumerate() {
                //             let big_col = small_col * 2;
                //             result[big_row_num * full_stride + big_col] = *val;
                //             result[big_row_num * full_stride + big_col + 1] = *val;

                //             result[(big_row_num + 1) * full_stride + big_col] = *val;
                //             result[(big_row_num + 1) * full_stride + big_col + 1] = *val;
                //         }
                //     }

                //     result
                // }
                // let ufull_data = expand_plane(self.u_plane_data(), self.u_stride());
                // let vfull_data = expand_plane(self.v_plane_data(), self.v_stride());

                // let mut image_data = vec![0u8; vfull_data.len() * 3];
                // let y_stride = self.y_stride();

                // for (dest_row, (y_row, (u_row, v_row))) in
                //     image_data.chunks_exact_mut(y_stride * 3).zip(
                //         y_data.chunks_exact(y_stride).zip(
                //             ufull_data
                //                 .chunks_exact(y_stride)
                //                 .zip(vfull_data.chunks_exact(y_stride)),
                //         ),
                //     )
                // {
                //     for (col, (y, (u, v))) in
                //         y_row.iter().zip(u_row.iter().zip(v_row.iter())).enumerate()
                //     {
                //         let rgb = YUV444_bt601_toRGB(*y, *u, *v);
                //         dest_row[col * 3] = rgb.R;
                //         dest_row[col * 3 + 1] = rgb.G;
                //         dest_row[col * 3 + 2] = rgb.B;
                //     }
                // }

                // let rgb8 = SimpleFrame::<RGB8>::new(
                //     self.width.try_into().unwrap(),
                //     self.height.try_into().unwrap(),
                //     (self.width * 3).try_into().unwrap(),
                //     image_data,
                // )
                // .unwrap();

                // // Then convert to final target output
                // let out = convert_owned::<_, RGB8, DEST>(rgb8)?;
                // Ok(out)
                todo!();
            }
            Y4MColorspace::CMono => {
                let mono8 = SimpleFrame::<Mono8>::new(
                    self.width.try_into().unwrap(),
                    self.height.try_into().unwrap(),
                    self.width.try_into().unwrap(),
                    y_data.to_vec(),
                )
                .unwrap();

                // Then convert to final target output
                let out = convert_owned::<_, Mono8, DEST>(mono8)?;
                Ok(out)
            }
        }
    }

    pub fn forced_block_size(&self) -> Option<u32> {
        self.forced_block_size
    }
    /// get the size of the luminance plane
    fn y_size(&self) -> usize {
        if self.forced_block_size.is_some() {
            self.y_stride as usize * self.alloc_rows as usize
        } else {
            self.y_stride as usize * self.height as usize
        }
    }
    /// get the size of each chrominance plane
    ///
    /// The U plane will have this size of data. The V plane will also. If
    /// requested with `forced_block_size`, this includes potentially invalid
    /// rows allocated for macroblocks.
    fn uv_size(&self) -> usize {
        self.u_stride() * TryInto::<usize>::try_into(self.alloc_chroma_rows).unwrap()
    }
    pub fn new_mono8(data: Vec<u8>, width: u32, height: u32) -> Result<Self> {
        let width: i32 = width.try_into().unwrap();
        let height: i32 = height.try_into().unwrap();
        let y_stride = width;
        let chroma_stride = 0;
        let expected_size = width as usize * height as usize;
        if data.len() != expected_size {
            return Err(invalid_buf_size_err());
        }
        let alloc_chroma_rows = 0;

        Ok(Self {
            data,
            width,
            height,
            y_stride,
            colorspace: Y4MColorspace::CMono,
            chroma_stride,
            alloc_rows: height,
            alloc_chroma_rows,
            is_known_mono_only: true,
            forced_block_size: None,
        })
    }
    pub fn is_known_mono_only(&self) -> bool {
        self.is_known_mono_only
    }
    pub fn data(&self) -> &[u8] {
        &self.data[..]
    }
    pub fn into_data(self) -> Vec<u8> {
        self.data
    }
    pub fn y_plane_data(&self) -> &[u8] {
        let ysize = self.y_size();
        &self.data[..ysize]
    }
    pub fn u_plane_data(&self) -> &[u8] {
        let ysize = self.y_size();
        &self.data[ysize..ysize + self.uv_size()]
    }
    pub fn v_plane_data(&self) -> &[u8] {
        let ysize = self.y_size();
        &self.data[(ysize + self.uv_size())..]
    }

    pub fn width(&self) -> u32 {
        self.width.try_into().unwrap()
    }
    pub fn height(&self) -> u32 {
        self.height.try_into().unwrap()
    }
    pub fn y_stride(&self) -> usize {
        self.y_stride.try_into().unwrap()
    }
    pub fn u_stride(&self) -> usize {
        self.chroma_stride
    }
    pub fn v_stride(&self) -> usize {
        self.chroma_stride
    }
    pub fn colorspace(&self) -> Y4MColorspace {
        self.colorspace
    }
}

fn generic_to_c420paldv_macroblocks<FMT>(
    frame: &dyn ImageStride<FMT>,
    block_size: u32,
) -> Result<Y4MFrame>
where
    FMT: PixelFormat,
{
    // Convert to planar data with macroblock size

    // TODO: convert directly to YUV420 instead of YUV444 for efficiency.
    // Currently we convert to YUV444 first and then downsample later.
    let frame_yuv444 = convert::<_, pixel_format::YUV444>(frame)?;

    let width: usize = frame.width().try_into().unwrap();

    // full width (i.e. Y plane)
    let fullstride: usize = next_multiple(frame.width(), block_size).try_into().unwrap();

    // full height (i.e. Y plane)
    let num_dest_alloc_rows_luma: usize = next_multiple(frame.height(), block_size)
        .try_into()
        .unwrap();

    let half_width = div_ceil(frame.width(), 2);

    // Calculate stride for downsampled chroma planes. It may not really be
    // "half" size because it needs to be a multiple of the block_size
    let halfstride: usize = next_multiple(half_width, block_size).try_into().unwrap();
    let half_height = div_ceil(frame.height(), 2);
    let valid_chroma_size: usize = halfstride * TryInto::<usize>::try_into(half_height).unwrap();
    let num_dest_allow_rows_chroma: usize =
        next_multiple(half_height, block_size).try_into().unwrap();

    // Allocate space for Y U and V planes. We already allocate one big
    // contiguous chunk with the fullsize Y and quarter size U and V planes.
    let y_size = fullstride * num_dest_alloc_rows_luma;
    let full_chroma_size = halfstride * num_dest_allow_rows_chroma;
    let mut data = vec![EMPTY_BYTE; y_size + 2 * full_chroma_size];

    let (y_plane_dest, uv_data) = data.split_at_mut(y_size);
    debug_assert_eq!(2 * full_chroma_size, uv_data.len());

    let (u_plane_dest, v_plane_dest) = uv_data.split_at_mut(full_chroma_size);

    // Here we allocate separate buffers for the fullsize U and V plane.
    let mut fullsize_u_plane = vec![EMPTY_BYTE; fullstride * num_dest_alloc_rows_luma];
    let mut fullsize_v_plane = vec![EMPTY_BYTE; fullstride * num_dest_alloc_rows_luma];

    // First, fill fullsize Y, U, and V planes. This would be full YUV444 resolution.
    for (
        y_plane_dest_row,
        (fullsize_u_plane_dest_row, (fullsize_v_plane_dest_row, src_yuv444_row)),
    ) in y_plane_dest.chunks_exact_mut(fullstride).zip(
        fullsize_u_plane.chunks_exact_mut(fullstride).zip(
            fullsize_v_plane.chunks_exact_mut(fullstride).zip(
                frame_yuv444
                    .image_data()
                    .chunks_exact(frame_yuv444.stride()),
            ),
        ),
    ) {
        for (y_dest_pix, (fullsize_u_dest_pix, (fullsize_v_dest_pix, yuv444_pix))) in
            y_plane_dest_row[..width].iter_mut().zip(
                fullsize_u_plane_dest_row[..width].iter_mut().zip(
                    fullsize_v_plane_dest_row[..width]
                        .iter_mut()
                        .zip(src_yuv444_row.chunks_exact(3)),
                ),
            )
        {
            *y_dest_pix = yuv444_pix[0];
            *fullsize_u_dest_pix = yuv444_pix[1];
            *fullsize_v_dest_pix = yuv444_pix[2];
        }
    }

    let y_data_ptr = y_plane_dest.as_ptr();
    let u_data_ptr = u_plane_dest.as_ptr();
    let v_data_ptr = v_plane_dest.as_ptr();

    fn u16(v: u8) -> u16 {
        v as u16
    }

    fn u8(v: u16) -> u8 {
        v as u8
    }

    let valid_chroma_width: usize = half_width.try_into().unwrap();

    // Now, downsample U and V planes into 420 scaling.
    for (dest_plane, src_plane_fullsize) in [
        (u_plane_dest, fullsize_u_plane),
        (v_plane_dest, fullsize_v_plane),
    ]
    .into_iter()
    {
        for (dest_row, dest_data) in dest_plane[0..valid_chroma_size]
            .chunks_exact_mut(halfstride)
            .enumerate()
        {
            let src_row = dest_row * 2;
            for (dest_col, dest_pix) in dest_data[..valid_chroma_width].iter_mut().enumerate() {
                let src_col = dest_col * 2;

                let a = u16(src_plane_fullsize[src_row * fullstride + src_col]);
                let b = u16(src_plane_fullsize[src_row * fullstride + src_col + 1]);
                let c = u16(src_plane_fullsize[(src_row + 1) * fullstride + src_col]);
                let d = u16(src_plane_fullsize[(src_row + 1) * fullstride + src_col + 1]);
                *dest_pix = u8((a + b + c + d) / 4);
            }
        }
    }
    let result = Y4MFrame::new(
        data,
        frame_yuv444.width(),
        frame_yuv444.height(),
        fullstride.try_into().unwrap(),
        halfstride,
        num_dest_alloc_rows_luma.try_into().unwrap(),
        num_dest_allow_rows_chroma.try_into().unwrap(),
        false,
        Some(block_size),
        Y4MColorspace::C420paldv,
    );

    debug_assert_eq!(result.y_stride(), fullstride);
    debug_assert_eq!(result.u_stride(), halfstride);
    debug_assert_eq!(result.v_stride(), halfstride);

    debug_assert_eq!(result.y_size(), y_size);
    debug_assert_eq!(result.uv_size(), full_chroma_size);

    // ---

    debug_assert_eq!(result.y_plane_data().as_ptr(), y_data_ptr);
    debug_assert_eq!(result.u_plane_data().as_ptr(), u_data_ptr);
    debug_assert_eq!(result.v_plane_data().as_ptr(), v_data_ptr);

    Ok(result)
}

fn generic_to_c420paldv<FMT>(frame: &dyn ImageStride<FMT>) -> Result<Y4MFrame>
where
    FMT: PixelFormat,
{
    // let colorspace = Y4MColorspace::C420paldv;
    // Convert to YUV444 first, then convert and downsample to YUV420
    // planar.

    // TODO: convert to YUV422 instead of YUV444 for efficiency.
    let frame = convert::<_, pixel_format::YUV444>(frame)?;

    // Convert to planar data.

    // TODO: allocate final buffer first and write directly into that. Here we make
    // intermediate copies.
    let h = frame.height() as usize;
    let width = frame.width() as usize;

    let yuv_iter = frame.image_data().chunks_exact(3).map(|yuv| YUV444 {
        Y: yuv[0],
        U: yuv[1],
        V: yuv[2],
    });
    // intermediate copy 1
    let yuv_vec: Vec<YUV444> = yuv_iter.collect();

    // intermediate copy 2a
    let y_plane: Vec<u8> = yuv_vec.iter().map(|yuv| yuv.Y).collect();
    let y_size = y_plane.len();

    // intermediate copy 2b
    let full_u_plane: Vec<u8> = yuv_vec.iter().map(|yuv| yuv.U).collect();
    // intermediate copy 2c
    let full_v_plane: Vec<u8> = yuv_vec.iter().map(|yuv| yuv.V).collect();

    // intermediate copy 3a
    let u_plane = downsample_plane(&full_u_plane, h, width);
    // intermediate copy 3b
    let v_plane = downsample_plane(&full_v_plane, h, width);

    let u_size = u_plane.len();
    let v_size = v_plane.len();
    debug_assert!(y_size == 4 * u_size);
    debug_assert!(u_size == v_size);

    // final copy
    let mut final_buf = vec![EMPTY_BYTE; y_size + u_size + v_size];
    final_buf[..y_size].copy_from_slice(&y_plane);
    final_buf[y_size..(y_size + u_size)].copy_from_slice(&u_plane);
    final_buf[(y_size + u_size)..].copy_from_slice(&v_plane);

    Ok(Y4MFrame::new(
        final_buf,
        frame.width(),
        frame.height(),
        width.try_into().unwrap(),
        width / 2,
        h.try_into().unwrap(),
        (h / 2).try_into().unwrap(),
        false,
        None,
        Y4MColorspace::C420paldv,
    ))
}

/// Convert any type implementing `ImageStride<FMT>` to a y4m buffer.
///
/// The y4m format is described at <http://wiki.multimedia.cx/index.php?title=YUV4MPEG2>
pub fn encode_y4m_frame<FMT>(
    frame: &dyn ImageStride<FMT>,
    out_colorspace: Y4MColorspace,
    forced_block_size: Option<u32>,
) -> Result<Y4MFrame>
where
    FMT: PixelFormat,
{
    match out_colorspace {
        Y4MColorspace::CMono => {
            if let Some(block_size) = forced_block_size {
                if !((frame.width() % block_size == 0) && (frame.height() % block_size == 0)) {
                    unimplemented!("conversion to mono with forced block size");
                }
            }
            let frame = convert::<_, Mono8>(frame)?;
            if frame.width() as usize != frame.stride() {
                // Copy into new buffer with no padding.
                let mut buf = vec![EMPTY_BYTE; frame.height() as usize * frame.width() as usize];
                for (dest_row, src_row) in buf
                    .chunks_exact_mut(frame.width() as usize)
                    .zip(frame.image_data().chunks_exact(frame.stride()))
                {
                    dest_row.copy_from_slice(&src_row[..frame.width() as usize]);
                }
                Ok(Y4MFrame::new_mono8(buf, frame.width(), frame.height())?)
            } else {
                Ok(Y4MFrame::new_mono8(
                    frame.image_data().to_vec(),
                    frame.width(),
                    frame.height(),
                )?)
            }
        }
        Y4MColorspace::C420paldv => {
            let input_pixfmt = formats::pixel_format::pixfmt::<FMT>().unwrap();
            match input_pixfmt {
                PixFmt::Mono8 => {
                    // Special case for mono8.
                    Ok(mono8_into_yuv420_planar(frame, forced_block_size))
                }
                _ => {
                    if let Some(block_size) = forced_block_size {
                        generic_to_c420paldv_macroblocks(frame, block_size)
                    } else {
                        generic_to_c420paldv(frame)
                    }
                }
            }
        }
    }
}

fn mono8_into_yuv420_planar<FMT>(
    frame: &dyn ImageStride<FMT>,
    forced_block_size: Option<u32>,
) -> Y4MFrame
where
    FMT: PixelFormat,
{
    // Copy intensity data, other planes
    // at 128.
    let width: usize = frame.width().try_into().unwrap();
    let height: usize = frame.height().try_into().unwrap();
    let src_stride = frame.stride();

    let (luma_stride, chroma_stride): (usize, usize) = if let Some(block_size) = forced_block_size {
        let w_mbs = div_ceil(frame.width(), block_size);
        let dest_stride = (w_mbs * block_size).try_into().unwrap();

        let chroma_w_mbs = div_ceil(frame.width() / 2, block_size);
        let chroma_stride = (chroma_w_mbs * block_size).try_into().unwrap();
        (dest_stride, chroma_stride)
    } else {
        (width, width / 2)
    };

    let (num_luma_alloc_rows, num_chroma_alloc_rows): (usize, usize) =
        if let Some(block_size) = forced_block_size {
            let h_mbs = div_ceil(frame.height(), block_size);
            let num_dest_alloc_rows = (h_mbs * block_size).try_into().unwrap();

            let chroma_h_mbs = div_ceil(frame.height() / 2, block_size);
            let num_chroma_alloc_rows = (chroma_h_mbs * block_size).try_into().unwrap();

            (num_dest_alloc_rows, num_chroma_alloc_rows)
        } else {
            (height, height / 2)
        };

    // allocate space for Y U and V planes
    let expected_size =
        luma_stride * num_luma_alloc_rows + chroma_stride * num_chroma_alloc_rows * 2;
    // Fill with value 128, which is neutral chrominance
    let mut data = vec![128u8; expected_size];
    // We fill the Y plane (and only the Y plane, leaving the
    // chrominance planes at 128).

    let luma_fill_size = luma_stride * height;

    for (dest_luma_row_slice, src) in data[..luma_fill_size]
        .chunks_exact_mut(luma_stride)
        .zip(frame.image_data().chunks_exact(src_stride))
    {
        dest_luma_row_slice[..width].copy_from_slice(&src[..width]);
    }

    let stride = luma_stride.try_into().unwrap();

    Y4MFrame::new(
        data,
        frame.width(),
        frame.height(),
        stride,
        chroma_stride,
        num_luma_alloc_rows.try_into().unwrap(),
        num_chroma_alloc_rows.try_into().unwrap(),
        true,
        forced_block_size,
        Y4MColorspace::C420paldv,
    )
}

fn downsample_plane(arr: &[u8], h: usize, w: usize) -> Vec<u8> {
    // This could be optimized for speed.
    let mut result = Vec::with_capacity((h / 2) * (w / 2));
    for i in 0..(h / 2) {
        for j in 0..(w / 2) {
            let tmp: u8 = ((arr[2 * i * w + 2 * j] as u16
                + arr[2 * i * w + 2 * j + 1] as u16
                + arr[(2 * i + 1) * w + 2 * j] as u16
                + arr[(2 * i + 1) * w + 2 * j + 1] as u16)
                / 4) as u8;
            result.push(tmp);
        }
    }
    result
}

fn next_multiple(a: u32, b: u32) -> u32 {
    div_ceil(a, b) * b
}

#[test]
fn test_next_multiple() {
    assert_eq!(next_multiple(10, 2), 10);
    assert_eq!(next_multiple(11, 2), 12);
    assert_eq!(next_multiple(15, 3), 15);
    assert_eq!(next_multiple(16, 3), 18);
    assert_eq!(next_multiple(18, 3), 18);
}

fn div_ceil(a: u32, b: u32) -> u32 {
    // See https://stackoverflow.com/a/72442854
    (a + b - 1) / b
}

#[test]
fn test_div_ceil() {
    assert_eq!(div_ceil(10, 2), 5);
    assert_eq!(div_ceil(11, 2), 6);
    assert_eq!(div_ceil(15, 3), 5);
    assert_eq!(div_ceil(16, 3), 6);
    assert_eq!(div_ceil(18, 3), 6);
}

fn encode_into_nv12_inner<FMT>(
    frame: &dyn ImageStride<FMT>,
    dest: &mut ImageBufferMutRef<NV12>,
    dest_stride: usize,
) -> Result<()>
where
    FMT: PixelFormat,
{
    use itertools::izip;

    let frame = to_rgb8_or_mono8(frame)?;

    let luma_size = frame.height() as usize * dest_stride;

    let (nv12_luma, nv12_chroma) = dest.data.split_at_mut(luma_size);

    match &frame {
        SupportedEncoding::Mono(frame) => {
            // ported from convertYUVpitchtoNV12 in NvEncoder.cpp
            let w = frame.width() as usize;
            for y in 0..frame.height() as usize {
                let start = dest_stride * y;
                let src = frame.stride() * y;
                nv12_luma[start..(start + w)].copy_from_slice(&frame.image_data()[src..(src + w)]);
            }

            for y in 0..(frame.height() as usize / 2) {
                let start = dest_stride * y;
                for x in (0..frame.width() as usize).step_by(2) {
                    nv12_chroma[start + x] = 128u8;
                    nv12_chroma[start + (x + 1)] = 128u8;
                }
            }
        }
        SupportedEncoding::Rgb(frame) => {
            let w = frame.width() as usize;

            // Allocate temporary storage for full-res chroma planes.
            // TODO: eliminate this or make it much smaller (e.g. two rows).
            let mut u_plane_full: Vec<u8> = vec![0; nv12_luma.len()];
            let mut v_plane_full: Vec<u8> = vec![0; nv12_luma.len()];

            for (src_row, dest_row, udest_row, vdest_row) in izip![
                frame.image_data().chunks_exact(frame.stride()),
                nv12_luma.chunks_exact_mut(dest_stride),
                u_plane_full.chunks_exact_mut(dest_stride),
                v_plane_full.chunks_exact_mut(dest_stride),
            ] {
                let yuv_iter = src_row[..w * 3]
                    .chunks_exact(3)
                    .map(|rgb| RGB888toYUV444_bt601_full_swing(rgb[0], rgb[1], rgb[2]));

                let dest_iter = dest_row[0..w].iter_mut();

                for (ydest, udest, vdest, yuv) in izip![dest_iter, udest_row, vdest_row, yuv_iter] {
                    *ydest = yuv.Y;
                    *udest = yuv.U;
                    *vdest = yuv.V;
                }
            }

            // Now downsample the full-res chroma planes.
            let half_stride = dest_stride; // This is not half because the two channels are interleaved.
            for y in 0..(frame.height() as usize / 2) {
                for x in 0..(frame.width() as usize / 2) {
                    let u_sum: u16 = u_plane_full[dest_stride * 2 * y + 2 * x] as u16
                        + u_plane_full[dest_stride * 2 * y + 2 * x + 1] as u16
                        + u_plane_full[dest_stride * (2 * y + 1) + 2 * x] as u16
                        + u_plane_full[dest_stride * (2 * y + 1) + 2 * x + 1] as u16;
                    let v_sum: u16 = v_plane_full[dest_stride * 2 * y + 2 * x] as u16
                        + v_plane_full[dest_stride * 2 * y + 2 * x + 1] as u16
                        + v_plane_full[dest_stride * (2 * y + 1) + 2 * x] as u16
                        + v_plane_full[dest_stride * (2 * y + 1) + 2 * x + 1] as u16;

                    nv12_chroma[(half_stride * y) + 2 * x] = (u_sum / 4) as u8;
                    nv12_chroma[(half_stride * y) + 2 * x + 1] = (v_sum / 4) as u8;
                }
            }
        }
    }
    Ok(())
}

fn invalid_buf_size_err() -> Error {
    Error::InvalidAllocatedBufferSize {
        #[cfg(feature = "backtrace")]
        backtrace: std::backtrace::Backtrace::capture(),
    }
}
