//! Conversion functions to convert between image formats.
//!
//! This crate contains a number of functions and helper types allow converting
//! to and from types specified in the [machine_vision_formats] crate, such as
//! the trait [machine_vision_formats::ImageData].

// TODO: Add support for Reversible Color Transform (RCT) YUV types

use bayer as wang_debayer;
use machine_vision_formats as formats;

use formats::{
    image_ref::{ImageRef, ImageRefMut},
    iter::{HasRowChunksExact, HasRowChunksExactMut},
    owned::OImage,
    pixel_format::{Mono8, NV12, RGB8},
    ImageBuffer, ImageBufferMutRef, ImageBufferRef, ImageData, OwnedImageStride, PixFmt,
    PixelFormat, Stride,
};

type Result<T> = std::result::Result<T, Error>;

/// Possible errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("unimplemented pixel_format: {0:?}")]
    UnimplementedPixelFormat(PixFmt),
    #[error("unimplemented ROI width conversion")]
    UnimplementedRoiWidthConversion,
    #[error("ROI size exceeds original image")]
    RoiExceedsOriginal,
    #[error("invalid allocated buffer size")]
    InvalidAllocatedBufferSize,
    #[error("invalid allocated buffer stride")]
    InvalidAllocatedBufferStride,
    #[error("{0}")]
    Bayer(#[from] wang_debayer::BayerError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Image(#[from] image::ImageError),
    #[error("unimplemented conversion {0} -> {1}")]
    UnimplementedConversion(PixFmt, PixFmt),
}

#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
#[derive(PartialEq, Eq, Debug)]
struct RGB888 {
    R: u8,
    G: u8,
    B: u8,
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
struct YUV444 {
    Y: u8,
    U: u8,
    V: u8,
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

/// Convert an input [image::DynamicImage] to an RGB8 image.
pub fn image_to_rgb8(
    input: image::DynamicImage,
) -> Result<impl OwnedImageStride<formats::pixel_format::RGB8>> {
    let rgb = input.to_rgb8();
    let (width, height) = rgb.dimensions();
    let stride = width as usize * 3;
    let data = rgb.into_vec();

    Ok(OImage::new(width, height, stride, data).unwrap())
}

/// Copy an YUV422 input image to a pre-allocated RGB8 buffer.
fn yuv422_into_rgb(
    src_yuv422: &dyn HasRowChunksExact<formats::pixel_format::YUV422>,
    dest_rgb: &mut dyn HasRowChunksExactMut<RGB8>,
) -> Result<()> {
    // The destination must be at least this large per row.
    let min_stride = src_yuv422.width() as usize * PixFmt::RGB8.bits_per_pixel() as usize / 8;
    if dest_rgb.stride() < min_stride {
        return Err(Error::InvalidAllocatedBufferStride);
    }

    let expected_size = dest_rgb.stride() * src_yuv422.height() as usize;
    if dest_rgb.buffer_mut_ref().data.len() != expected_size {
        return Err(Error::InvalidAllocatedBufferSize);
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
    frame: &dyn HasRowChunksExact<FMT>,
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
        return Err(Error::InvalidAllocatedBufferSize);
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
    frame: &dyn HasRowChunksExact<FMT>,
    dest_rgb: &mut dyn HasRowChunksExactMut<RGB8>,
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
    src: &dyn HasRowChunksExact<formats::pixel_format::Mono8>,
    dest_rgb: &mut dyn HasRowChunksExactMut<RGB8>,
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
    frame: &dyn HasRowChunksExact<formats::pixel_format::RGBA8>,
    dest: &mut dyn HasRowChunksExactMut<RGB8>,
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
    frame: &dyn HasRowChunksExact<formats::pixel_format::RGB8>,
    dest: &mut dyn HasRowChunksExactMut<Mono8>,
) -> Result<()> {
    if !(dest.height() == frame.height() && dest.width() == frame.width()) {
        return Err(Error::InvalidAllocatedBufferSize);
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
    frame: &dyn HasRowChunksExact<formats::pixel_format::YUV444>,
    dest: &mut dyn HasRowChunksExactMut<Mono8>,
) -> Result<()> {
    if !(dest.height() == frame.height() && dest.width() == frame.width()) {
        return Err(Error::InvalidAllocatedBufferSize);
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
    frame: &dyn HasRowChunksExact<formats::pixel_format::NV12>,
    dest: &mut dyn HasRowChunksExactMut<Mono8>,
) -> Result<()> {
    if !(dest.height() == frame.height() && dest.width() == frame.width()) {
        return Err(Error::InvalidAllocatedBufferSize);
    }

    for (src_row, dest_row) in frame.rowchunks_exact().zip(dest.rowchunks_exact_mut()) {
        dest_row[..frame.width() as usize].copy_from_slice(&src_row[..frame.width() as usize]);
    }

    Ok(())
}

/// If needed, copy original image data to remove stride.
fn remove_padding<FMT>(frame: &dyn HasRowChunksExact<FMT>) -> Result<CowImage<'_, FMT>>
where
    FMT: PixelFormat,
{
    let fmt = machine_vision_formats::pixel_format::pixfmt::<FMT>().unwrap();
    let bytes_per_pixel = fmt.bits_per_pixel() as usize / 8;
    let dest_stride = frame.width() as usize * bytes_per_pixel;
    if dest_stride == frame.stride() {
        Ok(CowImage::Borrowed(force_pixel_format_ref(frame)))
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
            OImage::new(frame.width(), frame.height(), dest_stride, dest_buf).unwrap(),
        ))
    }
}

enum CowImage<'a, F: PixelFormat> {
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

impl<'a, F: PixelFormat> Stride for CowImage<'a, F> {
    fn stride(&self) -> usize {
        match self {
            CowImage::Borrowed(im) => im.stride(),
            CowImage::Owned(im) => im.stride(),
        }
    }
}

impl<'a, F: PixelFormat> ImageData<F> for CowImage<'a, F> {
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

/// Force interpretation of data from frame into another pixel_format.
///
/// This moves the data and does not perform conversion of the underlying data,
/// but rather changes only the rust type. See [force_pixel_format_ref] for a
/// function which makes a view of the original data.
pub fn force_pixel_format<FRAME, FMT1, FMT2>(frame: FRAME) -> impl OwnedImageStride<FMT2>
where
    FRAME: OwnedImageStride<FMT1>,
    FMT2: PixelFormat,
{
    let width = frame.width();
    let height = frame.height();
    let stride = frame.stride();
    let image_data = frame.into(); // Move the original data.

    OImage::new(width, height, stride, image_data).unwrap()
}

/// Force interpretation of data from frame into another pixel_format.
///
/// This makes a view of the original data and does not perform conversion of
/// the underlying data, but rather changes only the rust type. See
/// [force_pixel_format] for a function which consumes the original data and
/// moves it into the output.
pub fn force_pixel_format_ref<'a, FMT1, FMT2>(
    frame: &'a dyn HasRowChunksExact<FMT1>,
) -> ImageRef<'a, FMT2>
where
    FMT1: 'a,
    FMT2: 'a + PixelFormat,
{
    ImageRef::new(
        frame.width(),
        frame.height(),
        frame.stride(),
        frame.image_data(),
    )
    .unwrap()
}

/// Force interpretation of data from frame into another pixel_format.
fn force_buffer_pixel_format_ref<FMT1, FMT2>(
    orig: ImageBufferMutRef<'_, FMT1>,
) -> ImageBufferMutRef<'_, FMT2> {
    ImageBufferMutRef::new(orig.data)
}

/// Convert input, a frame implementing [`OwnedImageStride<SRC>`], into
/// an output implementing [`HasRowChunksExact<DEST>`].
///
/// The source data will be moved, not copied, into the destination if no format
/// change is required, otherwise an image with a newly allocated image buffer
/// will be returned.
///
/// This is a general purpose function which should be able to convert between
/// many types as efficiently as possible. In case no data needs to be copied,
/// no data is copied.
///
/// For a version which converts into a pre-allocated buffer, use `convert_into`
/// (which will copy the image even if the format remains unchanged).
pub fn convert_owned<OWNED, SRC, DEST>(source: OWNED) -> Result<impl HasRowChunksExact<DEST>>
where
    OWNED: OwnedImageStride<SRC>,
    SRC: PixelFormat,
    DEST: PixelFormat,
{
    let src_fmt = machine_vision_formats::pixel_format::pixfmt::<SRC>().unwrap();
    let dest_fmt = machine_vision_formats::pixel_format::pixfmt::<DEST>().unwrap();

    // If format does not change, move original data without copy.
    if src_fmt == dest_fmt {
        let width = source.width();
        let height = source.height();
        let stride = source.stride();
        let buf: Vec<u8> = source.into();
        let dest = OImage::new(width, height, stride, buf).unwrap();
        return Ok(CowImage::Owned(dest));
    }

    // Allocate minimal size buffer for new image.
    let dest_min_stride = dest_fmt.bits_per_pixel() as usize * source.width() as usize / 8;
    let dest_size = source.height() as usize * dest_min_stride;
    let image_data = vec![0u8; dest_size];
    let mut dest =
        OImage::new(source.width(), source.height(), dest_min_stride, image_data).unwrap();

    // Fill the new buffer.
    convert_into(&source, &mut dest)?;

    // Return the new buffer as a new image.
    Ok(CowImage::Owned(dest))
}

/// Convert input image, a reference to a trait object implementing
/// [`HasRowChunksExact<SRC>`], into an output implementing
/// [`HasRowChunksExact<DEST>`].
///
/// The output will borrow from the source if no format change is required,
/// otherwise a newly allocated image will be returned.
///
/// This is a general purpose function which should be able to convert between
/// many types as efficiently as possible. In case no data needs to be copied,
/// no data will be copied.
///
/// For a version which converts into a pre-allocated buffer, use [convert_into]
/// (which will copy the image even if the format remains unchanged).
pub fn convert_ref<SRC, DEST>(
    source: &dyn HasRowChunksExact<SRC>,
) -> Result<impl HasRowChunksExact<DEST> + '_>
where
    SRC: PixelFormat,
    DEST: PixelFormat,
{
    let src_fmt = machine_vision_formats::pixel_format::pixfmt::<SRC>().unwrap();
    let dest_fmt = machine_vision_formats::pixel_format::pixfmt::<DEST>().unwrap();

    // If format does not change, return reference to original image without copy.
    if src_fmt == dest_fmt {
        return Ok(CowImage::Borrowed(force_pixel_format_ref(source)));
    }

    // Allocate minimal size buffer for new image.
    let dest_min_stride = dest_fmt.bits_per_pixel() as usize * source.width() as usize / 8;
    let dest_size = source.height() as usize * dest_min_stride;
    let image_data = vec![0u8; dest_size];
    let mut dest =
        OImage::new(source.width(), source.height(), dest_min_stride, image_data).unwrap();

    // Fill the new buffer.
    convert_into(source, &mut dest)?;

    // Return the new buffer as a new image.
    Ok(CowImage::Owned(dest))
}

/// Convert input image, a reference to a trait object implementing
/// [`HasRowChunksExact<SRC>`], into a mutable reference to an already allocated
///  destination frame implementing [`HasRowChunksExactMut<DEST>`].
///
/// This is a general purpose function which should be able to convert between
/// many types as efficiently as possible.
pub fn convert_into<SRC, DEST>(
    source: &dyn HasRowChunksExact<SRC>,
    dest: &mut dyn HasRowChunksExactMut<DEST>,
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
        let dest_size = source.height() as usize * dest_stride;
        if dest.buffer_mut_ref().data.len() != dest_size {
            return Err(Error::InvalidAllocatedBufferSize);
        }

        use itertools::izip;
        let w = source.width() as usize;
        let nbytes = dest_fmt.bits_per_pixel() as usize * w / 8;
        for (src_row, dest_row) in izip![
            source.image_data().chunks_exact(source.stride()),
            dest.buffer_mut_ref().data.chunks_exact_mut(dest_stride),
        ] {
            dest_row[..nbytes].copy_from_slice(&src_row[..nbytes]);
        }
    }

    match dest_fmt {
        formats::pixel_format::PixFmt::RGB8 => {
            let mut dest_rgb = ImageRefMut::new(
                dest.width(),
                dest.height(),
                dest.stride(),
                dest.buffer_mut_ref().data,
            )
            .unwrap();
            // Convert to RGB8..
            match src_fmt {
                formats::pixel_format::PixFmt::BayerRG8
                | formats::pixel_format::PixFmt::BayerGB8
                | formats::pixel_format::PixFmt::BayerGR8
                | formats::pixel_format::PixFmt::BayerBG8 => {
                    // .. from bayer.
                    // The bayer code requires no padding in the input image.
                    let exact_stride = remove_padding(source)?;
                    bayer_into_rgb(&exact_stride, &mut dest_rgb)?;
                    Ok(())
                }
                formats::pixel_format::PixFmt::Mono8 => {
                    // .. from mono8.
                    let mono8 = force_pixel_format_ref(source);
                    mono8_into_rgb8(&mono8, &mut dest_rgb)?;
                    Ok(())
                }
                formats::pixel_format::PixFmt::RGBA8 => {
                    // .. from rgba8.
                    let rgba8 = force_pixel_format_ref(source);
                    rgba_into_rgb(&rgba8, &mut dest_rgb)?;
                    Ok(())
                }
                formats::pixel_format::PixFmt::YUV422 => {
                    // .. from YUV422.
                    let yuv422 = force_pixel_format_ref(source);
                    yuv422_into_rgb(&yuv422, &mut dest_rgb)?;
                    Ok(())
                }
                _ => Err(Error::UnimplementedConversion(src_fmt, dest_fmt)),
            }
        }
        formats::pixel_format::PixFmt::Mono8 => {
            let mut dest_mono8 = ImageRefMut::new(
                dest.width(),
                dest.height(),
                dest.stride(),
                dest.buffer_mut_ref().data,
            )
            .unwrap();

            // Convert to Mono8..
            match src_fmt {
                formats::pixel_format::PixFmt::RGB8 => {
                    // .. from RGB8.
                    let tmp = force_pixel_format_ref(source);
                    {
                        rgb8_into_mono8(&tmp, &mut dest_mono8)?;
                    }
                    Ok(())
                }
                formats::pixel_format::PixFmt::YUV444 => {
                    // .. from YUV444.
                    let yuv444 = force_pixel_format_ref(source);
                    // let mut mono8 = force_buffer_pixel_format_ref(&mut dest.buffer_mut_ref());
                    yuv444_into_mono8(&yuv444, &mut dest_mono8)?;
                    Ok(())
                }
                formats::pixel_format::PixFmt::NV12 => {
                    // .. from NV12.
                    let nv12 = force_pixel_format_ref(source);
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
            into_yuv444(source, &mut dest2, dest_stride)?;
            Ok(())
        }
        formats::pixel_format::PixFmt::NV12 => {
            // Convert to NV12.
            let mut dest2 = force_buffer_pixel_format_ref(dest.buffer_mut_ref());
            encode_into_nv12_inner(source, &mut dest2, dest_stride)?;
            Ok(())
        }
        _ => Err(Error::UnimplementedConversion(src_fmt, dest_fmt)),
    }
}

/// An image which can be directly encoded as RGB8 or Mono8
///
/// This nearly supports the HasRowChunksExact trait, but we avoid it because it has a
/// type parameter specifying the pixel format, whereas we don't use that here
/// and instead explicitly represent an image with one of two possible pixel
/// formats.
enum SupportedEncoding<'a> {
    Rgb(Box<dyn HasRowChunksExact<formats::pixel_format::RGB8> + 'a>),
    Mono(Box<dyn HasRowChunksExact<formats::pixel_format::Mono8> + 'a>),
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
fn to_rgb8_or_mono8<FMT>(frame: &dyn HasRowChunksExact<FMT>) -> Result<SupportedEncoding<'_>>
where
    FMT: PixelFormat,
{
    if machine_vision_formats::pixel_format::pixfmt::<FMT>().unwrap()
        == formats::pixel_format::PixFmt::Mono8
    {
        let im = convert_ref::<_, formats::pixel_format::Mono8>(frame)?;
        Ok(SupportedEncoding::Mono(Box::new(im)))
    } else {
        let im = convert_ref::<_, formats::pixel_format::RGB8>(frame)?;
        Ok(SupportedEncoding::Rgb(Box::new(im)))
    }
}

/// Convert any type implementing [HasRowChunksExact] to an [image::DynamicImage].
pub fn frame_to_image<FMT>(frame: &dyn HasRowChunksExact<FMT>) -> Result<image::DynamicImage>
where
    FMT: PixelFormat,
{
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
            return Err(Error::InvalidAllocatedBufferSize);
        }
        for src_row in chunk_iter {
            dest.extend_from_slice(&src_row[..packed_stride]);
        }
        packed = Some(dest);
    }

    let packed = match packed {
        None => frame.image_data().to_vec(),
        Some(p) => p,
    };

    match coding {
        image::ColorType::L8 => {
            let imbuf: image::ImageBuffer<image::Luma<_>, _> =
                image::ImageBuffer::from_raw(frame.width(), frame.height(), packed).unwrap();
            Ok(imbuf.into())
        }
        image::ColorType::Rgb8 => {
            let imbuf: image::ImageBuffer<image::Rgb<_>, _> =
                image::ImageBuffer::from_raw(frame.width(), frame.height(), packed).unwrap();
            Ok(imbuf.into())
        }
        _ => {
            unreachable!()
        }
    }
}

/// How to encode to an image buffer
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EncoderOptions {
    /// Encode to a JPEG buffer with a quality specified from 0 to 100.
    Jpeg(u8),
    /// Encode to a PNG buffer.
    Png,
}

/// Convert any type implementing [HasRowChunksExact] to a Jpeg or Png buffer
/// using the [EncoderOptions] specified.
pub fn frame_to_encoded_buffer<FMT>(
    frame: &dyn HasRowChunksExact<FMT>,
    opts: EncoderOptions,
) -> Result<Vec<u8>>
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
            return Err(Error::InvalidAllocatedBufferSize);
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
        EncoderOptions::Jpeg(quality) => {
            let mut encoder =
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut result, quality);
            encoder.encode(use_frame, frame.width(), frame.height(), coding.into())?;
        }
        EncoderOptions::Png => {
            use image::ImageEncoder;
            let encoder = image::codecs::png::PngEncoder::new(&mut result);
            encoder.write_image(use_frame, frame.width(), frame.height(), coding.into())?;
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use crate::*;

    /// An RoiImage maintains a reference to the original image but views a
    /// subregion of the original data.
    struct RoiImage<'a, F> {
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
        fn new(
            frame: &'a dyn HasRowChunksExact<F>,
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

    fn imstr<F>(frame: &dyn HasRowChunksExact<F>) -> String
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
        let frame: OImage<formats::pixel_format::Mono8> =
            OImage::new(W, H, STRIDE, image_data).unwrap();
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
        let frame: OImage<formats::pixel_format::RGB8> =
            OImage::new(W, H, STRIDE, image_data).unwrap();
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
        let frame: OImage<formats::pixel_format::Mono8> =
            OImage::new(W, H, STRIDE, image_data).unwrap();
        let buf = frame_to_encoded_buffer(&frame, EncoderOptions::Png).unwrap();

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
        let frame: OImage<formats::pixel_format::BayerRG8> =
            OImage::new(W, H, STRIDE, image_data).unwrap();
        frame_to_encoded_buffer(&frame, EncoderOptions::Jpeg(100)).unwrap();
    }

    #[test]
    fn prevent_unnecessary_copy_mono8() {
        let frame: OImage<formats::pixel_format::Mono8> =
            OImage::new(10, 10, 10, vec![42; 100]).unwrap();
        // `im2` has only a reference to original data.
        let im2 = convert_ref::<_, formats::pixel_format::Mono8>(&frame).unwrap();
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
        let frame: OImage<formats::pixel_format::RGB8> =
            OImage::new(10, 10, 30, vec![42; 300]).unwrap();
        // `im2` has only a reference to original data.
        let im2 = convert_ref::<_, formats::pixel_format::RGB8>(&frame).unwrap();
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
        let orig: OImage<formats::pixel_format::Mono8> =
            OImage::new(256, 1, 256, (0u8..=255u8).collect()).unwrap();
        let rgb = convert_ref::<_, formats::pixel_format::RGB8>(&orig)?;
        for (i, rgb_pix) in rgb.image_data().chunks_exact(3).enumerate() {
            assert_eq!(i, rgb_pix[0] as usize);
            assert_eq!(i, rgb_pix[1] as usize);
            assert_eq!(i, rgb_pix[2] as usize);
        }
        Ok(())
    }

    #[test]
    fn test_mono8_rgb_roundtrip() -> Result<()> {
        let orig: OImage<formats::pixel_format::Mono8> =
            OImage::new(256, 1, 256, (0u8..=255u8).collect()).unwrap();
        let rgb = convert_ref::<_, formats::pixel_format::RGB8>(&orig)?;
        let actual = convert_ref::<_, formats::pixel_format::Mono8>(&rgb)?;
        assert_eq!(orig.image_data(), actual.image_data());
        Ok(())
    }

    #[test]
    fn test_mono8_nv12_roundtrip() -> Result<()> {
        let orig: OImage<formats::pixel_format::Mono8> =
            OImage::new(256, 1, 256, (0u8..=255u8).collect()).unwrap();
        let nv12 = convert_ref::<_, formats::pixel_format::NV12>(&orig)?;
        let actual = convert_ref::<_, formats::pixel_format::Mono8>(&nv12)?;
        for i in 0..256 {
            assert_eq!(orig.image_data()[i], actual.image_data()[i]);
        }
        assert_eq!(orig.image_data(), actual.image_data());
        Ok(())
    }

    #[test]
    // Test MONO8->YUV444->MONO8.
    fn test_mono8_yuv_roundtrip() -> Result<()> {
        let orig: OImage<formats::pixel_format::Mono8> =
            OImage::new(256, 1, 256, (0u8..=255u8).collect()).unwrap();
        let yuv = convert_ref::<_, formats::pixel_format::YUV444>(&orig)?;
        let actual = convert_ref::<_, formats::pixel_format::Mono8>(&yuv)?;
        assert_eq!(orig.image_data(), actual.image_data());
        Ok(())
    }
}

fn encode_into_nv12_inner<FMT>(
    frame: &dyn HasRowChunksExact<FMT>,
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
