#![recursion_limit = "512"]

// TODO: Add support for Reversible Color Transform (RCT) YUV types

use bayer as wang_debayer;
use machine_vision_formats as formats;

#[macro_use]
extern crate itertools;

use basic_frame::BasicFrame;
use formats::{ImageData, ImageStride, PixelFormat, Stride};

mod repeat_elements;
use repeat_elements::RepeatElement;

type Result<T> = std::result::Result<T, Error>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("unimplemented pixel_format: {0:?}")]
    UnimplementedPixelFormat(formats::PixelFormat),
    #[error("unimplemented ROI width conversion")]
    UnimplementedRoiWidthConversion,
    #[error("{0:?}")]
    Bayer(wang_debayer::BayerError),
    #[error("{0}")]
    Io(std::io::Error),
    #[error("unimplemented conversion {0} -> {1}")]
    UnimplementedConversion(formats::PixelFormat, formats::PixelFormat),
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

// TODO: make this private and use only Box<OwnedImageStride> in this
// crate's public interface.

pub struct ConvertImageFrame {
    /// number of pixels wide
    width: u32,
    /// number of pixels high
    height: u32,
    /// number of bytes in an image row
    stride: u32,
    /// raw image data
    image_data: Vec<u8>,
    /// format of the data
    pixel_format: PixelFormat,
}

impl From<BasicFrame> for ConvertImageFrame {
    fn from(orig: BasicFrame) -> ConvertImageFrame {
        let width = orig.width;
        let height = orig.height;
        let stride = orig.stride;
        let pixel_format = orig.pixel_format;
        let image_data = orig.into();
        ConvertImageFrame {
            width,
            height,
            stride,
            pixel_format,
            image_data,
        }
    }
}

fn _test_convert_image_frame_is_send() {
    // Compile-time test to ensure BasicFrame implements Send trait.
    fn implements<T: Send>() {}
    implements::<ConvertImageFrame>();
}

fn _test_convert_image_frame_0() {
    fn implements<T: Into<Vec<u8>>>() {}
    implements::<ConvertImageFrame>();
}

fn _test_convert_image_frame_1() {
    fn implements<T: ImageStride>() {}
    implements::<ConvertImageFrame>();
}

impl std::fmt::Debug for ConvertImageFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "ConvertImageFrame {{ {}x{} }}", self.width, self.height)
    }
}

impl ImageData for ConvertImageFrame {
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

impl Stride for ConvertImageFrame {
    fn stride(&self) -> usize {
        self.stride as usize
    }
}

impl From<ConvertImageFrame> for Vec<u8> {
    fn from(orig: ConvertImageFrame) -> Vec<u8> {
        orig.image_data
    }
}

impl From<Box<ConvertImageFrame>> for Vec<u8> {
    fn from(orig: Box<ConvertImageFrame>) -> Vec<u8> {
        orig.image_data
    }
}

impl<F> From<Box<F>> for ConvertImageFrame
where
    F: formats::OwnedImageStride,
    Vec<u8>: From<Box<F>>,
{
    fn from(frame: Box<F>) -> ConvertImageFrame {
        let width = frame.width();
        let height = frame.height();
        let stride = frame.stride() as u32;
        let pixel_format = frame.pixel_format();

        ConvertImageFrame {
            width,
            height,
            stride,
            image_data: frame.into(),
            pixel_format,
        }
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
pub fn YUV444toRGB888(Y: u8, U: u8, V: u8) -> RGB888 {
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
pub fn RGB888toYUV444(R: u8, G: u8, B: u8) -> YUV444 {
    let R = R as i32;
    let G = G as i32;
    let B = B as i32;
    let Y = ((66 * R + 129 * G + 25 * B + 128) >> 8) + 16;
    let U = ((-38 * R - 74 * G + 112 * B + 128) >> 8) + 128;
    let V = ((112 * R - 94 * G - 18 * B + 128) >> 8) + 128;
    YUV444 {
        Y: Y as u8,
        U: U as u8,
        V: V as u8,
    }
}

pub fn piston_to_frame(piston_image: image::DynamicImage) -> Result<ConvertImageFrame> {
    let rgb = piston_image.to_rgb();
    let (width, height) = rgb.dimensions();
    let stride = width * 3;
    let data = rgb.into_vec();

    Ok(ConvertImageFrame {
        width,
        height,
        stride,
        image_data: data,
        pixel_format: formats::PixelFormat::RGB8,
    })
}

pub fn yuv422_to_rgb<F: ImageStride>(frame: &F) -> Result<ConvertImageFrame> {
    if frame.stride() != frame.width() as usize {
        return Err(Error::UnimplementedRoiWidthConversion);
    }

    let width = frame.width() as usize;
    let height = frame.height() as usize;

    let mut result = vec![0u8; width * height * 3];

    for (yuv422_pixpair, result_chunk) in frame
        .image_data()
        .chunks(4)
        .zip(result.as_mut_slice().chunks_mut(6))
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

    Ok(ConvertImageFrame {
        image_data: result,
        width: frame.width(),
        height: frame.height(),
        stride: width as u32 * 3,
        pixel_format: formats::PixelFormat::RGB8,
    })
}

pub fn bayer_to_rgb(frame: &dyn ImageStride) -> Result<ConvertImageFrame> {
    if frame.stride() != frame.width() as usize {
        return Err(Error::UnimplementedRoiWidthConversion);
    }
    let cfa = match frame.pixel_format() {
        formats::PixelFormat::BayerRG8 => wang_debayer::CFA::RGGB,
        formats::PixelFormat::BayerGB8 => wang_debayer::CFA::GBRG,
        formats::PixelFormat::BayerGR8 => wang_debayer::CFA::GRBG,
        formats::PixelFormat::BayerBG8 => wang_debayer::CFA::BGGR,
        e => {
            return Err(Error::UnimplementedPixelFormat(e));
        }
    };

    use std::io::Cursor;

    let mut buf = vec![0; frame.width() as usize * frame.height() as usize * 3];

    {
        let mut dst = wang_debayer::RasterMut::new(
            frame.width() as usize,
            frame.height() as usize,
            wang_debayer::RasterDepth::Depth8,
            &mut buf,
        );

        wang_debayer::run_demosaic(
            &mut Cursor::new(&frame.image_data()),
            wang_debayer::BayerDepth::Depth8,
            cfa,
            wang_debayer::Demosaic::Cubic,
            &mut dst,
        )?;
    }

    Ok(ConvertImageFrame {
        width: frame.width(),
        height: frame.height(),
        stride: frame.stride() as u32 * 3,
        image_data: buf,
        pixel_format: formats::PixelFormat::RGB8,
    })
}

pub fn mono_to_rgb(frame: &dyn ImageStride) -> Result<ConvertImageFrame> {
    if frame.stride() != frame.width() as usize {
        return Err(Error::UnimplementedRoiWidthConversion);
    }
    debug_assert!(frame.pixel_format() == formats::PixelFormat::MONO8);

    let buf: Vec<u8> = frame.image_data().iter().repeat_elements(3).collect();

    Ok(ConvertImageFrame {
        width: frame.width(),
        height: frame.height(),
        stride: frame.stride() as u32,
        image_data: buf,
        pixel_format: formats::PixelFormat::RGB8,
    })
}

pub enum CowImage<'a> {
    Ref(&'a dyn ImageStride),
    Owned(ConvertImageFrame),
}

impl<'a> From<ConvertImageFrame> for CowImage<'a> {
    fn from(frame: ConvertImageFrame) -> CowImage<'a> {
        CowImage::Owned(frame)
    }
}

impl<'a> Stride for CowImage<'a> {
    fn stride(&self) -> usize {
        match self {
            CowImage::Ref(im) => im.stride(),
            CowImage::Owned(im) => im.stride(),
        }
    }
}

impl<'a> ImageData for CowImage<'a> {
    fn width(&self) -> u32 {
        match self {
            CowImage::Ref(im) => im.width(),
            CowImage::Owned(im) => im.width(),
        }
    }
    fn height(&self) -> u32 {
        match self {
            CowImage::Ref(im) => im.height(),
            CowImage::Owned(im) => im.height(),
        }
    }
    fn image_data(&self) -> &[u8] {
        match self {
            CowImage::Ref(im) => im.image_data(),
            CowImage::Owned(im) => im.image_data(),
        }
    }
    fn pixel_format(&self) -> formats::PixelFormat {
        match self {
            CowImage::Ref(im) => im.pixel_format(),
            CowImage::Owned(im) => im.pixel_format(),
        }
    }
}

impl<'a> From<CowImage<'a>> for Vec<u8> {
    fn from(orig: CowImage<'a>) -> Vec<u8> {
        match orig {
            CowImage::Ref(im) => im.image_data().to_vec(), // this forces a data copy (clone). TODO FIXME XXX Can I avoid this?
            CowImage::Owned(im) => From::from(Box::new(im)),
        }
    }
}

impl<'a> From<Box<CowImage<'a>>> for Vec<u8> {
    fn from(orig: Box<CowImage<'a>>) -> Vec<u8> {
        let unboxed = *orig;
        match unboxed {
            CowImage::Ref(im) => im.image_data().to_vec(), // this forces a data copy (clone)
            CowImage::Owned(im) => From::from(Box::new(im)),
        }
    }
}

/// force interpretation of data from frame into another pixel_format
pub fn force_pixel_formats<F>(
    frame: Box<F>,
    forced_pixfmt: formats::PixelFormat,
) -> ConvertImageFrame
where
    F: ImageStride,
    Vec<u8>: From<Box<F>>,
{
    let width = frame.width();
    let height = frame.height();
    let stride = frame.stride() as u32;
    let pixel_format = forced_pixfmt;

    ConvertImageFrame {
        width,
        height,
        stride,
        image_data: frame.into(),
        pixel_format,
    }
}

/// convert frame with one pixel_format into another pixel_format
pub fn convert<F: ImageStride>(frame: &F, new_pixfmt: formats::PixelFormat) -> Result<CowImage> {
    let old_pixfmt = frame.pixel_format();
    if old_pixfmt == new_pixfmt {
        return Ok(CowImage::Ref(frame));
    }
    {
        use crate::formats::PixelFormat::*;
        match (old_pixfmt, new_pixfmt) {
            (BayerRG8, RGB8) | (BayerGB8, RGB8) | (BayerGR8, RGB8) | (BayerBG8, RGB8) => {
                Ok(CowImage::Owned(bayer_to_rgb(frame)?))
            }
            (MONO8, RGB8) => Ok(CowImage::Owned(mono_to_rgb(frame)?)),
            _ => Err(Error::UnimplementedConversion(old_pixfmt, new_pixfmt)),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum ImageOptions {
    Jpeg(u8),
    Png,
}

pub fn frame_to_image<F>(frame: &F, opts: ImageOptions) -> Result<Vec<u8>>
where
    F: ImageStride,
{
    let mut result = Vec::new();
    #[allow(unused_mut)]
    let mut keep = None;

    let coding = match frame.pixel_format() {
        formats::PixelFormat::MONO8 => image::ColorType::Gray(8),
        formats::PixelFormat::BayerRG8
        | formats::PixelFormat::BayerGB8
        | formats::PixelFormat::BayerGR8
        | formats::PixelFormat::BayerBG8 => {
            keep = Some(bayer_to_rgb(frame)?);
            image::ColorType::RGB(8)
        }
        formats::PixelFormat::RGB8 => image::ColorType::RGB(8),
        formats::PixelFormat::YUV422 => {
            keep = Some(yuv422_to_rgb(frame)?);
            image::ColorType::RGB(8)
        }

        e => {
            return Err(Error::UnimplementedPixelFormat(e));
        }
    };

    let use_frame = match keep {
        None => frame.image_data(),
        Some(ref r) => r.image_data().as_ref(),
    };
    match opts {
        ImageOptions::Jpeg(quality) => {
            let mut encoder = image::jpeg::JPEGEncoder::new_with_quality(&mut result, quality);
            encoder.encode(&use_frame, frame.width(), frame.height(), coding)?;
        }
        ImageOptions::Png => {
            let encoder = image::png::PNGEncoder::new(&mut result);
            encoder.encode(&use_frame, frame.width(), frame.height(), coding)?;
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use crate::*;

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
                        format!("expected: {:?}, actual: {:?}", expected, actual)
                    );
                    assert!(
                        actual.max_channel_distance(&expected) <= 3,
                        format!("expected: {:?}, actual: {:?}", expected, actual)
                    );
                }
            }
        }
    }
}

/// Spec at http://wiki.multimedia.cx/index.php?title=YUV4MPEG2
#[derive(Debug, Clone, Copy)]
pub enum Colorspace {
    /// luminance
    ///
    /// WARNING: Not compatible with much software, not in spec.
    CMono,
    /// 4:2:0 with vertically-displaced chroma planes
    C420paldv,
    // /// 4:4:4
    // C444,
}

impl std::str::FromStr for Colorspace {
    type Err = &'static str;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "Mono" | "mono" => Ok(Colorspace::CMono),
            "C420paldv" | "420paldv" => Ok(Colorspace::C420paldv),
            // "C444" | "444" => Ok(Colorspace::C444),
            _ => Err("unknown colorspace"),
        }
    }
}

impl std::fmt::Display for Colorspace {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::result::Result<(), std::fmt::Error> {
        match self {
            Colorspace::CMono => write!(f, "mono"),
            Colorspace::C420paldv => write!(f, "420paldv"),
            // &Colorspace::C444 => write!(f, "444"),
        }
    }
}

pub fn encode_y4m_frame(frame: &dyn ImageStride, colorspace: Colorspace) -> Result<Vec<u8>> {
    use machine_vision_formats::PixelFormat::*;

    // convert bayer formats
    let frame: CowImage = match frame.pixel_format() {
        BayerRG8 | BayerGB8 | BayerGR8 | BayerBG8 => bayer_to_rgb(frame)?.into(),
        _ => CowImage::Ref(frame),
    };

    match frame.pixel_format() {
        MONO8 => {
            match colorspace {
                Colorspace::CMono => {
                    if frame.width() as usize != frame.stride() {
                        panic!(); // TODO: convert to error, not panic
                    }
                    Ok(frame.image_data().to_vec())
                }
                Colorspace::C420paldv => {
                    // Convert pure luminance data (mono8) into YCbCr. First plane
                    // is lumance data, next two planes are color chrominance.
                    let h = frame.height() as usize;
                    let w = frame.width() as usize;
                    let nh = h * 3 / 2;
                    let mut buf = vec![128u8; nh * w];
                    buf[..(h * w)].copy_from_slice(frame.image_data());
                    Ok(buf)
                } // Colorspace::C444 => {
                  //     // Convert pure luminance data (mono8) into YCbCr.
                  //     let h = frame.height() as usize;
                  //     let w = frame.width() as usize;
                  //     let mut buf = Vec::with_capacity(3*h*w);
                  //     for byte in frame.image_data() {
                  //         buf.push(*byte);
                  //         buf.push(128u8);
                  //         buf.push(128u8);
                  //     }
                  //     Ok(buf)
                  // }
            }
        }
        RGB8 => {
            let h = frame.height() as usize;
            let width = frame.width() as usize;

            let yuv_iter = frame
                .image_data()
                .chunks_exact(3)
                .map(|rgb| RGB888toYUV444(rgb[0], rgb[1], rgb[2]));

            match colorspace {
                // Colorspace::C444 => {
                //     let mut buf = Vec::with_capacity(3*h*width);
                //     for el in yuv_iter {
                //         buf.push(el.Y);
                //         buf.push(el.U);
                //         buf.push(el.V);
                //     }
                //     Ok(buf)
                // }
                Colorspace::C420paldv => {
                    // Can we make this more efficient by not converting to
                    // intermediate vector and looping through it multiple
                    // times?
                    let yuv_vec: Vec<crate::YUV444> = yuv_iter.collect();
                    let y_plane: Vec<u8> = yuv_vec.iter().map(|yuv| yuv.Y).collect();
                    let y_size = y_plane.len();

                    let full_u_plane: Vec<u8> = yuv_vec.iter().map(|yuv| yuv.U).collect();
                    let full_v_plane: Vec<u8> = yuv_vec.iter().map(|yuv| yuv.V).collect();

                    let u_plane = downsample_plane(&full_u_plane, h, width);
                    let v_plane = downsample_plane(&full_v_plane, h, width);

                    let u_size = u_plane.len();
                    let v_size = v_plane.len();
                    debug_assert!(y_size == 4 * u_size);
                    debug_assert!(u_size == v_size);

                    let mut final_buf = vec![0u8; y_size + u_size + v_size];
                    final_buf[..y_size].copy_from_slice(&y_plane);
                    final_buf[y_size..(y_size + u_size)].copy_from_slice(&u_plane);
                    final_buf[(y_size + u_size)..].copy_from_slice(&v_plane);
                    Ok(final_buf)
                }
                Colorspace::CMono => {
                    let y_plane = yuv_iter.map(|yuv| yuv.Y).collect();
                    Ok(y_plane)
                }
            }
        }
        fmt => Err(Error::UnimplementedPixelFormat(fmt)),
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

pub fn encode_into_nv12(
    frame: &dyn ImageStride,
    nv12_full: &mut [u8],
    dest_stride: usize,
) -> Result<()> {
    use machine_vision_formats::PixelFormat::*;

    // convert bayer formats
    let frame: CowImage = match frame.pixel_format() {
        BayerRG8 | BayerGB8 | BayerGR8 | BayerBG8 => bayer_to_rgb(frame)?.into(),
        _ => CowImage::Ref(frame),
    };

    let luma_size = frame.height() as usize * dest_stride;

    let (nv12_luma, nv12_chroma) = nv12_full.split_at_mut(luma_size);
    match frame.pixel_format() {
        MONO8 => {
            // ported from convertYUVpitchtoNV12 in NvEncoder.cpp

            let w = frame.width() as usize;
            for y in 0..frame.height() as usize {
                let dest = dest_stride * y;
                let src = frame.stride() * y;
                nv12_luma[dest..(dest + w)].copy_from_slice(&frame.image_data()[src..(src + w)]);
            }

            for y in 0..(frame.height() as usize / 2) {
                let dest = dest_stride * y;
                for x in (0..frame.width() as usize).step_by(2) {
                    nv12_chroma[dest + x] = 128u8;
                    nv12_chroma[dest + (x + 1)] = 128u8;
                }
            }
            Ok(())
        }
        RGB8 => {
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

                for (dest, udest, vdest, yuv) in izip![dest_iter, udest_row, vdest_row, yuv_iter] {
                    *dest = yuv.Y;
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
            Ok(())
        }
        fmt => Err(Error::UnimplementedPixelFormat(fmt)),
    }
}

pub fn encode_into_gray8(
    frame: &dyn ImageStride,
    gray8_data: &mut [u8],
    dest_stride: usize,
) -> Result<()> {
    use machine_vision_formats::PixelFormat::*;

    // convert bayer formats
    let frame: CowImage = match frame.pixel_format() {
        BayerRG8 | BayerGB8 | BayerGR8 | BayerBG8 => bayer_to_rgb(frame)?.into(),
        _ => CowImage::Ref(frame),
    };

    let luma_size = frame.height() as usize * dest_stride;
    debug_assert!(gray8_data.len() == luma_size);

    match frame.pixel_format() {
        MONO8 => {
            let w = frame.width() as usize;
            for y in 0..frame.height() as usize {
                let dest = dest_stride * y;
                let src = frame.stride() * y;
                gray8_data[dest..(dest + w)].copy_from_slice(&frame.image_data()[src..(src + w)]);
            }
            Ok(())
        }
        RGB8 => {
            let w = frame.width() as usize;
            for (src_row, dest_row) in izip![
                frame.image_data().chunks_exact(frame.stride()),
                gray8_data.chunks_exact_mut(dest_stride),
            ] {
                let yuv_iter = src_row[..w * 3]
                    .chunks_exact(3)
                    .map(|rgb| RGB888toYUV444(rgb[0], rgb[1], rgb[2]));

                let dest_iter = dest_row[0..w].iter_mut();

                for (dest, yuv) in izip![dest_iter, yuv_iter] {
                    *dest = yuv.Y;
                }
            }
            Ok(())
        }
        fmt => Err(Error::UnimplementedPixelFormat(fmt)),
    }
}
