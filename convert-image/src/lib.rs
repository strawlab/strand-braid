#![recursion_limit = "512"]

// TODO: Add support for Reversible Color Transform (RCT) YUV types

use bayer as wang_debayer;
use machine_vision_formats as formats;

use formats::{
    pixel_format::{self, Mono8, NV12, RGB8},
    ImageBuffer, ImageBufferMutRef, ImageBufferRef, ImageData, ImageStride, OwnedImageStride,
    PixFmt, PixelFormat, Stride,
};
use simple_frame::SimpleFrame;

type Result<T> = std::result::Result<T, Error>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("unimplemented pixel_format: {0:?}")]
    UnimplementedPixelFormat(PixFmt),
    #[error("unimplemented ROI width conversion")]
    UnimplementedRoiWidthConversion,
    #[error("invalid allocated buffer size")]
    InvalidAllocatedBufferSize,
    #[error("invalid allocated buffer stride")]
    InvalidAllocatedBufferStride,
    #[error("{0:?}")]
    Bayer(wang_debayer::BayerError),
    #[error("{0}")]
    Io(std::io::Error),
    #[error("{0}")]
    Image(#[from] image::ImageError),
    #[error("unimplemented conversion {0} -> {1}")]
    UnimplementedConversion(PixFmt, PixFmt),
}

impl From<wang_debayer::BayerError> for Error {
    fn from(orig: wang_debayer::BayerError) -> Error {
        Error::Bayer(orig)
    }
}

impl From<std::io::Error> for Error {
    fn from(orig: std::io::Error) -> Error {
        Error::Io(orig)
    }
}

#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
#[derive(PartialEq, Debug)]
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

        let m2 = if m1 > db { m1 } else { db };

        m2
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
#[derive(PartialEq, Debug)]
pub struct YUV444 {
    pub Y: u8,
    pub U: u8,
    pub V: u8,
}

#[inline]
fn clamp(i: i32) -> u8 {
    if i < 0 {
        0
    } else if i > 255 {
        255
    } else {
        i as u8
    }
}

#[allow(non_snake_case)]
fn YUV444toRGB888(Y: u8, U: u8, V: u8) -> RGB888 {
    // see http://en.wikipedia.org/wiki/YUV
    let C: i32 = Y as i32 - 16;
    let D: i32 = U as i32 - 128;
    let E: i32 = V as i32 - 128;

    let R: u8 = clamp((298 * C + 409 * E + 128) >> 8);
    let G: u8 = clamp((298 * C - 100 * D - 208 * E + 128) >> 8);
    let B: u8 = clamp((298 * C + 516 * D + 128) >> 8);

    RGB888 { R, G, B }
}

#[allow(non_snake_case)]
#[inline]
fn RGB888toYUV444(R: u8, G: u8, B: u8) -> YUV444 {
    let Y = RGB888toY4(R, G, B);
    let R = R as i32;
    let G = G as i32;
    let B = B as i32;
    let U = ((-38 * R - 74 * G + 112 * B + 128) >> 8) + 128;
    let V = ((112 * R - 94 * G - 18 * B + 128) >> 8) + 128;
    YUV444 {
        Y: Y as u8,
        U: U as u8,
        V: V as u8,
    }
}

#[allow(non_snake_case)]
#[inline]
fn RGB888toY4(R: u8, G: u8, B: u8) -> u8 {
    let R = R as i32;
    let G = G as i32;
    let B = B as i32;
    let Y = ((66 * R + 129 * G + 25 * B + 128) >> 8) + 16;
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

    Ok(SimpleFrame {
        width,
        height,
        stride,
        image_data: data,
        fmt: std::marker::PhantomData,
    })
}

/// Copy an input image to a pre-allocated RGB8 buffer.
fn yuv422_into_rgb(
    frame: &dyn ImageStride<formats::pixel_format::YUV422>,
    dest: &mut ImageBufferMutRef<RGB8>,
    dest_stride: usize,
) -> Result<()> {
    // The destination must be at least this large per row.
    let min_stride = frame.width() as usize * PixFmt::RGB8.bits_per_pixel() as usize / 8;
    if dest_stride < min_stride {
        return Err(Error::InvalidAllocatedBufferStride);
    }

    let expected_size = dest_stride * frame.height() as usize;
    if dest.data.len() != expected_size {
        return Err(Error::InvalidAllocatedBufferSize);
    }

    use itertools::izip;
    let w = frame.width() as usize;
    for (src_row, dest_row) in izip![
        frame.image_data().chunks_exact(frame.stride()),
        dest.data.chunks_exact_mut(dest_stride),
    ] {
        for (result_chunk, yuv422_pixpair) in dest_row[..(w * 3)]
            .chunks_exact_mut(6)
            .zip(src_row[..w * 2].chunks_exact(4))
        {
            let u = yuv422_pixpair[0];
            let y1 = yuv422_pixpair[1];
            let v = yuv422_pixpair[2];
            let y2 = yuv422_pixpair[3];

            let tmp_rgb1 = YUV444toRGB888(y1, u, v);
            let tmp_rgb2 = YUV444toRGB888(y2, u, v);

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
                    let yuv = RGB888toYUV444(src_pixel[0], src_pixel[1], src_pixel[2]);
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
    dest: &mut ImageBufferMutRef<RGB8>,
    dest_stride: usize,
) -> Result<()>
where
    FMT: formats::PixelFormat,
{
    if frame.stride() != frame.width() as usize {
        return Err(Error::UnimplementedRoiWidthConversion);
    }

    // The debayer code expects exactly this stride.
    let expected_stride = frame.width() as usize * PixFmt::RGB8.bits_per_pixel() as usize / 8;
    if dest_stride != expected_stride {
        return Err(Error::InvalidAllocatedBufferStride);
    }

    let expected_size = frame.width() as usize * frame.height() as usize * 3;
    if dest.data.len() != expected_size {
        return Err(Error::InvalidAllocatedBufferSize);
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
            &mut dest.data,
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
fn mono_into_rgb(
    frame: &dyn ImageStride<formats::pixel_format::Mono8>,
    dest: &mut ImageBufferMutRef<RGB8>,
    dest_stride: usize,
) -> Result<()> {
    // The destination must be at least this large per row.
    let min_stride = frame.width() as usize * PixFmt::RGB8.bits_per_pixel() as usize / 8;
    if dest_stride < min_stride {
        return Err(Error::InvalidAllocatedBufferStride);
    }

    let expected_size = dest_stride * frame.height() as usize;
    if dest.data.len() != expected_size {
        return Err(Error::InvalidAllocatedBufferSize);
    }

    use itertools::izip;
    let w = frame.width() as usize;
    for (src_row, dest_row) in izip![
        frame.image_data().chunks_exact(frame.stride()),
        dest.data.chunks_exact_mut(dest_stride),
    ] {
        for (dest_pix, src_pix) in dest_row[..(w * 3)].chunks_exact_mut(3).zip(&src_row[..w]) {
            dest_pix[0] = *src_pix;
            dest_pix[1] = *src_pix;
            dest_pix[2] = *src_pix;
        }
    }
    Ok(())
}

/// Convert RGB8 image data into pre-allocated Mono8 buffer.
fn rgb8_into_mono8(
    frame: &dyn ImageStride<formats::pixel_format::RGB8>,
    dest: &mut ImageBufferMutRef<Mono8>,
    dest_stride: usize,
) -> Result<()> {
    let luma_size = frame.height() as usize * dest_stride;
    if dest.data.len() != luma_size {
        return Err(Error::InvalidAllocatedBufferSize);
    }

    use itertools::izip;
    let w = frame.width() as usize;
    for (src_row, dest_row) in izip![
        frame.image_data().chunks_exact(frame.stride()),
        dest.data.chunks_exact_mut(dest_stride),
    ] {
        let y_iter = src_row[..w * 3]
            .chunks_exact(3)
            .map(|rgb| RGB888toY4(rgb[0], rgb[1], rgb[2]));

        let dest_iter = dest_row[0..w].iter_mut();

        for (ydest, y) in izip![dest_iter, y_iter] {
            *ydest = y;
        }
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
        ImageBufferRef::new(&self.orig.image_data())
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
            CowImage::Owned(im) => ImageBuffer::new(im.image_data),
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
{
    let width = frame.width();
    let height = frame.height();
    let stride = frame.stride() as u32;
    let image_data = frame.into(); // Move the original data.

    SimpleFrame {
        width,
        height,
        stride,
        image_data,
        fmt: std::marker::PhantomData,
    }
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

/// Force interpretation of data from frame into another pixel_format.
fn force_buffer_pixel_format_ref<'a, 'b, FMT1, FMT2>(
    orig: &'b mut ImageBufferMutRef<'a, FMT1>,
) -> ImageBufferMutRef<'b, FMT2>
where
    FMT1: 'a,
    FMT2: 'b,
{
    ImageBufferMutRef::new(orig.data)
}

/// Convert input frame with pixel_format `SRC` into pixel_format `DEST`
///
/// This is a general purpose function which should be able to convert between
/// many types as efficiently as possible. In case no data needs to be copied,
/// no data is copied.
pub fn convert<SRC, DEST>(frame: &dyn ImageStride<SRC>) -> Result<impl ImageStride<DEST> + '_>
where
    SRC: PixelFormat,
    DEST: PixelFormat,
{
    // TODO: add a variant which copies into pre-allocated buffer.

    let src_fmt = machine_vision_formats::pixel_format::pixfmt::<SRC>().unwrap();
    let dest_fmt = machine_vision_formats::pixel_format::pixfmt::<DEST>().unwrap();

    // If format does not change, return reference to original image without copy.
    if src_fmt == dest_fmt {
        return Ok(CowImage::Reinterpreted(force_pixel_format_ref(frame)));
    }

    // Allocate buffer for new image.
    let dest_stride = dest_fmt.bits_per_pixel() as usize * frame.width() as usize / 8;
    let dest_size = frame.height() as usize * dest_stride;
    let mut dest_buf = vec![0u8; dest_size];
    {
        let mut dest: ImageBufferMutRef<DEST> = ImageBufferMutRef::new(&mut dest_buf);
        // Fill the new buffer.
        convert_into(frame, &mut dest, dest_stride)?;
    }

    // Return the new buffer as a new image.
    Ok(CowImage::Owned(SimpleFrame {
        width: frame.width(),
        height: frame.height(),
        stride: dest_stride as u32,
        image_data: dest_buf,
        fmt: std::marker::PhantomData,
    }))
}

/// Convert input frame with pixel_format `SRC` into pixel_format `DEST`
///
/// This is a general purpose function which should be able to convert between
/// many types as efficiently as possible.
pub fn convert_into<SRC, DEST>(
    frame: &dyn ImageStride<SRC>,
    dest: &mut ImageBufferMutRef<DEST>,
    dest_stride: usize,
) -> Result<()>
where
    SRC: PixelFormat,
    DEST: PixelFormat,
{
    let src_fmt = machine_vision_formats::pixel_format::pixfmt::<SRC>().unwrap();
    let dest_fmt = machine_vision_formats::pixel_format::pixfmt::<DEST>().unwrap();

    // If format does not change, copy the data row-by-row to respect strides.
    if src_fmt == dest_fmt {
        let dest_size = frame.height() as usize * dest_stride;
        if dest.data.len() != dest_size {
            return Err(Error::InvalidAllocatedBufferSize);
        }

        use itertools::izip;
        let w = frame.width() as usize;
        let nbytes = dest_fmt.bits_per_pixel() as usize * w / 8;
        for (src_row, dest_row) in izip![
            frame.image_data().chunks_exact(frame.stride()),
            dest.data.chunks_exact_mut(dest_stride),
        ] {
            dest_row[..nbytes].copy_from_slice(&src_row[..nbytes]);
        }
    }

    match dest_fmt {
        formats::pixel_format::PixFmt::RGB8 => {
            // Convert to RGB8..
            match src_fmt {
                formats::pixel_format::PixFmt::BayerRG8
                | formats::pixel_format::PixFmt::BayerGB8
                | formats::pixel_format::PixFmt::BayerGR8
                | formats::pixel_format::PixFmt::BayerBG8 => {
                    // .. from bayer.
                    let mut rgb = force_buffer_pixel_format_ref(dest);
                    bayer_into_rgb(frame, &mut rgb, dest_stride)?;
                    Ok(())
                }
                formats::pixel_format::PixFmt::Mono8 => {
                    // .. from mono8.
                    let mono8 = force_pixel_format_ref(frame);
                    let mut rgb = force_buffer_pixel_format_ref(dest);
                    mono_into_rgb(&mono8, &mut rgb, dest_stride)?;
                    Ok(())
                }
                formats::pixel_format::PixFmt::YUV422 => {
                    // .. from YUV422.
                    let yuv422 = force_pixel_format_ref(frame);
                    let mut rgb = force_buffer_pixel_format_ref(dest);
                    yuv422_into_rgb(&yuv422, &mut rgb, dest_stride)?;
                    Ok(())
                }
                _ => Err(Error::UnimplementedConversion(src_fmt, dest_fmt)),
            }
        }
        formats::pixel_format::PixFmt::Mono8 => {
            // Convert to Mono8..
            match src_fmt {
                formats::pixel_format::PixFmt::RGB8 => {
                    // .. from RGB8.
                    let tmp = force_pixel_format_ref(frame);
                    {
                        let mut dest2 = force_buffer_pixel_format_ref(dest);
                        rgb8_into_mono8(&tmp, &mut dest2, dest_stride)?;
                    }
                    Ok(())
                }
                _ => Err(Error::UnimplementedConversion(src_fmt, dest_fmt)),
            }
        }
        formats::pixel_format::PixFmt::YUV444 => {
            // Convert to YUV444.
            let mut dest2 = force_buffer_pixel_format_ref(dest);
            into_yuv444(frame, &mut dest2, dest_stride)?;
            Ok(())
        }
        formats::pixel_format::PixFmt::NV12 => {
            // Convert to NV12.
            let mut dest2 = force_buffer_pixel_format_ref(dest);
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

#[derive(Copy, Clone, Debug, PartialEq)]
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

    let coding = match &frame {
        SupportedEncoding::Mono(_) => image::ColorType::L8,
        SupportedEncoding::Rgb(_) => image::ColorType::Rgb8,
    };

    // The encoders in the `image` crate only handle packed inputs. We check if
    // our data is packed and if not, make a packed copy.

    let mut packed = None;
    let bytes_per_pixel = machine_vision_formats::pixel_format::pixfmt::<FMT>()
        .unwrap()
        .bits_per_pixel()
        / 8;
    let packed_stride = frame.width() as usize * bytes_per_pixel as usize;
    if frame.stride() != packed_stride {
        let mut dest = Vec::with_capacity(packed_stride * frame.height() as usize);
        let src = frame.image_data();
        let chunk_iter = src.chunks_exact(frame.stride());
        if chunk_iter.remainder().len() != 0 {
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
        ImageOptions::Jpeg(quality) => {
            let mut encoder = image::jpeg::JpegEncoder::new_with_quality(&mut result, quality);
            encoder.encode(&use_frame, frame.width(), frame.height(), coding)?;
        }
        ImageOptions::Png => {
            let encoder = image::png::PngEncoder::new(&mut result);
            encoder.encode(&use_frame, frame.width(), frame.height(), coding)?;
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use crate::*;

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
        let frame: SimpleFrame<formats::pixel_format::Mono8> = SimpleFrame {
            width: W,
            height: H,
            stride: STRIDE as u32,
            image_data,
            fmt: std::marker::PhantomData,
        };
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
    fn prevent_unnecessary_copy_mono8() {
        let frame: SimpleFrame<formats::pixel_format::Mono8> = SimpleFrame {
            width: 10,
            height: 10,
            stride: 10,
            image_data: vec![42; 100],
            fmt: std::marker::PhantomData,
        };
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
        let frame: SimpleFrame<formats::pixel_format::RGB8> = SimpleFrame {
            width: 10,
            height: 10,
            stride: 30,
            image_data: vec![42; 300],
            fmt: std::marker::PhantomData,
        };
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
        let black_rgb = RGB888 { R: 0, G: 0, B: 0 };
        let black_yuv = RGB888toYUV444(black_rgb.R, black_rgb.G, black_rgb.B);
        let black_rgb2 = YUV444toRGB888(black_yuv.Y, black_yuv.U, black_yuv.V);
        assert_eq!(black_rgb, black_rgb2);

        let white_rgb = RGB888 {
            R: 255,
            G: 255,
            B: 255,
        };
        let white_yuv = RGB888toYUV444(white_rgb.R, white_rgb.G, white_rgb.B);
        let white_rgb2 = YUV444toRGB888(white_yuv.Y, white_yuv.U, white_yuv.V);
        assert_eq!(white_rgb, white_rgb2);

        for r in 0..255 {
            for g in 0..255 {
                for b in 0..255 {
                    let expected = RGB888 { R: r, G: g, B: b };
                    let yuv = RGB888toYUV444(expected.R, expected.G, expected.B);
                    let actual = YUV444toRGB888(yuv.Y, yuv.U, yuv.V);
                    assert!(
                        actual.distance(&expected) <= 5,
                        "expected: {:?}, actual: {:?}",
                        expected,
                        actual
                    );
                    assert!(
                        actual.max_channel_distance(&expected) <= 3,
                        "expected: {:?}, actual: {:?}",
                        expected,
                        actual
                    );
                }
            }
        }
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

/// Convert any type implementing `ImageStride<FMT>` to a y4m buffer.
///
/// The y4m format is described at <http://wiki.multimedia.cx/index.php?title=YUV4MPEG2>
pub fn encode_y4m_frame<FMT>(
    frame: &dyn ImageStride<FMT>,
    colorspace: Y4MColorspace,
) -> Result<Vec<u8>>
where
    FMT: PixelFormat,
{
    match colorspace {
        Y4MColorspace::CMono => {
            let frame = convert::<_, Mono8>(frame)?;
            if frame.width() as usize != frame.stride() {
                // Copy into new buffer with no padding.
                let mut buf = vec![0u8; frame.height() as usize * frame.width() as usize];
                for (dest_row, src_row) in buf
                    .chunks_exact_mut(frame.width() as usize)
                    .zip(frame.image_data().chunks_exact(frame.stride()))
                {
                    dest_row.copy_from_slice(&src_row[..frame.width() as usize]);
                }
                return Ok(buf);
            } else {
                Ok(frame.image_data().to_vec())
            }
        }
        Y4MColorspace::C420paldv => {
            // Convert to YUV444.
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
            let mut final_buf = vec![0u8; y_size + u_size + v_size];
            final_buf[..y_size].copy_from_slice(&y_plane);
            final_buf[y_size..(y_size + u_size)].copy_from_slice(&u_plane);
            final_buf[(y_size + u_size)..].copy_from_slice(&v_plane);
            Ok(final_buf)
        }
    }
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

/// Copy any type implementing `ImageStride<FMT>` into an nv12 buffer.
///
/// The encoded image is placed into the buffer specified by `dest`. It
/// will have stride `dest_stride`.
pub fn encode_into_nv12<FMT>(
    frame: &dyn ImageStride<FMT>,
    dest: &mut ImageBufferMutRef<NV12>,
    dest_stride: usize,
) -> Result<()>
where
    FMT: PixelFormat,
{
    convert_into(frame, dest, dest_stride)
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
                    .map(|rgb| RGB888toYUV444(rgb[0], rgb[1], rgb[2]));

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

/// Copy any type implementing `ImageStride<FMT>` to a "gray8" ("mono8") buffer.
pub fn encode_into_mono8<FMT>(
    frame: &dyn ImageStride<FMT>,
    dest: &mut ImageBufferMutRef<Mono8>,
    dest_stride: usize,
) -> Result<()>
where
    FMT: PixelFormat,
{
    convert_into(frame, dest, dest_stride)
}
