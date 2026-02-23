use std::io::Write;

use machine_vision_formats as formats;
use strand_dynamic_frame::DynamicFrame;

use formats::{
    iter::HasRowChunksExact,
    owned::OImage,
    pixel_format::{self, Mono8, PixFmt},
    ImageData, PixelFormat, Stride,
};

use convert_image::{convert_owned, convert_ref};

const EMPTY_BYTE: u8 = 128;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("convert-image error: {0}")]
    ConvertImageError(#[from] convert_image::Error),
    #[error("format or size changed")]
    FormatOrSizeChanged,
    #[error("unknown pixel format: {0}")]
    UnknownPixelFormat(String),
    #[error("unsupported pixel format: {0}")]
    UnsupportedPixelFormat(formats::pixel_format::PixFmt),
    #[error("unsupported colorspace: {0:?}")]
    UnsupportedColorspace(y4m::Colorspace),
    #[error("invalid allocated buffer size")]
    InvalidAllocatedBufferSize,
    #[error("{0}")]
    Y4mError(#[from] y4m::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
#[derive(PartialEq, Eq, Debug)]
struct YUV444 {
    Y: u8,
    U: u8,
    V: u8,
}

#[derive(Debug)]
pub struct Y4MOptions {
    /// Frame rate (numerator)
    pub raten: usize,
    /// Frame rate (denominator)
    pub rated: usize,
    /// Aspect ratio (numerator)
    pub aspectn: usize,
    /// Aspect ratio (denominator)
    pub aspectd: usize,
}

enum Writer {
    NotStarted(Box<dyn Write>),
    Started(y4m::Encoder<Box<dyn Write>>),
    /// Placeholder value for internal use
    Undefined,
}

impl Writer {
    fn encoder(&mut self) -> Option<&mut y4m::Encoder<Box<dyn Write>>> {
        match self {
            Self::Started(e) => Some(e),
            _ => None,
        }
    }
}

/// An opinionated Y4M writer.
///
/// Saves only progressive video with full color range.
pub struct Y4MWriter {
    wtr: Writer,
    opts: Y4MOptions,
    info: Option<Y4MInfo>,
}

struct Y4MInfo {
    width: usize,
    height: usize,
    fmt: formats::pixel_format::PixFmt,
}

impl Y4MWriter {
    pub fn from_writer(wtr: Box<dyn Write>, opts: Y4MOptions) -> Self {
        Self {
            wtr: Writer::NotStarted(wtr),
            opts,
            info: None,
        }
    }
    pub fn write_dynamic_frame(&mut self, frame: &DynamicFrame) -> Result<()> {
        let this_fmt: formats::pixel_format::PixFmt = frame.pixel_format();
        let this_width: usize = frame.width().try_into().unwrap();
        let this_height: usize = frame.height().try_into().unwrap();

        let info = self.info.get_or_insert(Y4MInfo {
            width: this_width,
            height: this_height,
            fmt: this_fmt,
        });
        if this_width != info.width || this_height != info.height || this_fmt != info.fmt {
            return Err(Error::FormatOrSizeChanged);
        }

        let colorspace = match this_fmt {
            formats::pixel_format::PixFmt::Mono8 => y4m::Colorspace::Cmono,
            formats::pixel_format::PixFmt::RGB8 => y4m::Colorspace::C420paldv,
            formats::pixel_format::PixFmt::YUV422 => y4m::Colorspace::C420paldv,
            _ => {
                return Err(Error::UnsupportedPixelFormat(this_fmt));
            }
        };

        let wtr = std::mem::replace(&mut self.wtr, Writer::Undefined);

        match wtr {
            Writer::NotStarted(wtr) => {
                let builder = y4m::EncoderBuilder::new(
                    info.width,
                    info.height,
                    y4m::Ratio::new(self.opts.raten, self.opts.rated),
                )
                .with_pixel_aspect(y4m::Ratio::new(self.opts.aspectn, self.opts.aspectd))
                .with_colorspace(colorspace)
                .append_vendor_extension(y4m::VendorExtensionString::new(
                    b"COLORRANGE=FULL".into(),
                )?);
                let encoder = builder.write_header(wtr)?;
                self.wtr = Writer::Started(encoder);
            }
            Writer::Started(encoder) => {
                self.wtr = Writer::Started(encoder);
            }
            Writer::Undefined => {
                unreachable!();
            }
        };

        let encoder = self.wtr.encoder().unwrap();

        let encoded = encode_y4m_dynamic_frame(frame, colorspace, None)?;
        let frame = (&encoded).into();
        encoder.write_frame(&frame)?;

        Ok(())
    }

    // flush to disk.
    pub fn flush(&mut self) -> Result<()> {
        // Only if we started the writer is there anything to flush.
        if let Writer::Started(encoder) = &mut self.wtr {
            encoder.flush()?;
        }
        Ok(())
    }

    pub fn into_inner(self) -> Box<dyn Write> {
        match self.wtr {
            Writer::NotStarted(w) => w,
            Writer::Started(e) => e.into_inner(),
            _ => {
                unreachable!();
            }
        }
    }
}

// -----------

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

#[inline]
fn div_ceil(a: u32, b: u32) -> u32 {
    a.div_ceil(b)
}

#[test]
fn test_div_ceil() {
    assert_eq!(div_ceil(10, 2), 5);
    assert_eq!(div_ceil(11, 2), 6);
    assert_eq!(div_ceil(15, 3), 5);
    assert_eq!(div_ceil(16, 3), 6);
    assert_eq!(div_ceil(18, 3), 6);
}

/// Contains information to output y4m data.
///
/// The y4m format is described at
/// <http://wiki.multimedia.cx/index.php?title=YUV4MPEG2>
pub struct Y4MFrame {
    pub data: Vec<u8>,
    pub width: i32,
    pub height: i32,
    pub y_stride: i32,
    colorspace: y4m::Colorspace,
    chroma_stride: usize,
    alloc_rows: i32,
    alloc_chroma_rows: i32,
    /// True if the U and V planes are known to contain no data.
    is_known_mono_only: bool,
    forced_block_size: Option<u32>,
}

impl<'a> From<&'a Y4MFrame> for y4m::Frame<'a> {
    fn from(val: &'a Y4MFrame) -> Self {
        Self::new(
            [val.y_plane_data(), val.u_plane_data(), val.v_plane_data()],
            None,
        )
    }
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
        colorspace: y4m::Colorspace,
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

    pub fn convert<DEST>(&self) -> Result<impl HasRowChunksExact<DEST> + use<DEST>>
    where
        DEST: PixelFormat,
    {
        let y_data = self.y_plane_data();

        match &self.colorspace {
            y4m::Colorspace::C420paldv => {
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

                // let rgb8 = Image::<RGB8>::new(
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
            y4m::Colorspace::Cmono => {
                let mono8 = OImage::<Mono8>::new(
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
            cs => Err(Error::UnsupportedColorspace(*cs)),
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
            return Err(Error::InvalidAllocatedBufferSize);
        }
        let alloc_chroma_rows = 0;

        Ok(Self {
            data,
            width,
            height,
            y_stride,
            colorspace: y4m::Colorspace::Cmono,
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
    pub fn colorspace(&self) -> y4m::Colorspace {
        self.colorspace
    }
}

fn generic_to_c420paldv_macroblocks<FMT>(
    frame: &dyn HasRowChunksExact<FMT>,
    block_size: u32,
) -> Result<Y4MFrame>
where
    FMT: PixelFormat,
{
    // Convert to planar data with macroblock size

    // TODO: convert directly to YUV420 instead of YUV444 for efficiency.
    // Currently we convert to YUV444 first and then downsample later.
    let frame_yuv444 = convert_ref::<_, pixel_format::YUV444>(frame)?;

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

    // OK to use .chunks_exact() and .chunks_exact_mut() on `data` because we
    // know even the last row has a full stride worth of bytes. However, this is
    // not true for the source image.

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
            fullsize_v_plane
                .chunks_exact_mut(fullstride)
                .zip(frame_yuv444.rowchunks_exact()),
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
        y4m::Colorspace::C420paldv,
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

fn generic_to_c420paldv<FMT>(frame: &dyn HasRowChunksExact<FMT>) -> Result<Y4MFrame>
where
    FMT: PixelFormat,
{
    // let colorspace = Y4MColorspace::C420paldv;
    // Convert to YUV444 first, then convert and downsample to YUV420
    // planar.

    // TODO: convert to YUV422 instead of YUV444 for efficiency.
    let frame = convert_ref::<_, pixel_format::YUV444>(frame)?;

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
        y4m::Colorspace::C420paldv,
    ))
}

/// Converts input frame into a [Y4MFrame].
pub fn encode_y4m_dynamic_frame(
    frame: &DynamicFrame,
    out_colorspace: y4m::Colorspace,
    forced_block_size: Option<u32>,
) -> Result<Y4MFrame> {
    let pixfmt = frame.pixel_format();
    strand_dynamic_frame::match_all_dynamic_fmts!(
        frame,
        x,
        encode_y4m_frame(&x, out_colorspace, forced_block_size),
        Error::ConvertImageError(convert_image::Error::UnimplementedPixelFormat(pixfmt))
    )
}

/// Converts input, a reference to a trait object implementing
/// [`HasRowChunksExact<FMT>`], into a [Y4MFrame].
fn encode_y4m_frame<FMT>(
    frame: &dyn HasRowChunksExact<FMT>,
    out_colorspace: y4m::Colorspace,
    forced_block_size: Option<u32>,
) -> Result<Y4MFrame>
where
    FMT: PixelFormat,
{
    match out_colorspace {
        y4m::Colorspace::Cmono => {
            if let Some(block_size) = forced_block_size {
                if !((frame.width() % block_size == 0) && (frame.height() % block_size == 0)) {
                    unimplemented!("conversion to mono with forced block size");
                }
            }
            let frame = convert_ref::<_, Mono8>(frame)?;
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
        y4m::Colorspace::C420paldv => {
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
        cs => Err(Error::UnsupportedColorspace(cs)),
    }
}

fn mono8_into_yuv420_planar<FMT>(
    frame: &dyn HasRowChunksExact<FMT>,
    forced_block_size: Option<u32>,
) -> Y4MFrame
where
    FMT: PixelFormat,
{
    // Copy intensity data, other planes
    // at 128.
    let width: usize = frame.width().try_into().unwrap();
    let height: usize = frame.height().try_into().unwrap();

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
        .zip(frame.rowchunks_exact())
    {
        debug_assert_eq!(width, src.len());
        dest_luma_row_slice[..width].copy_from_slice(src);
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
        y4m::Colorspace::C420paldv,
    )
}
