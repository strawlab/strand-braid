use std::fs::File;
use std::io::Read;
use std::path::Path;

use byteorder::{LittleEndian, ReadBytesExt};

use basic_frame::{BasicExtra, BasicFrame, DynamicFrame};
use datetime_conversion::f64_to_datetime;
use formats::PixFmt;

use crate::{pixel_formats, FMFError, FMFResult};

macro_rules! bf {
    ($width:expr, $height:expr, $stride:expr, $image_data:expr, $extra:expr) => {{
        BasicFrame {
            width: $width,
            height: $height,
            stride: $stride,
            image_data: $image_data,
            extra: $extra,
            pixel_format: std::marker::PhantomData,
        }
    }};
}

const TIMESTAMP_SIZE: usize = 8;

/// Return an DynamicFrame variant according to $pixfmt.
#[macro_export]
macro_rules! to_dynamic {
    ($pixfmt:expr, $w:expr, $h:expr, $s:expr, $data:expr, $ex:expr) => {{
        Ok(match $pixfmt {
            PixFmt::Mono8 => DynamicFrame::Mono8(bf!($w, $h, $s, $data, $ex)),
            PixFmt::Mono32f => DynamicFrame::Mono32f(bf!($w, $h, $s, $data, $ex)),
            PixFmt::RGB8 => DynamicFrame::RGB8(bf!($w, $h, $s, $data, $ex)),
            PixFmt::BayerRG8 => DynamicFrame::BayerRG8(bf!($w, $h, $s, $data, $ex)),
            PixFmt::BayerRG32f => DynamicFrame::BayerRG32f(bf!($w, $h, $s, $data, $ex)),
            PixFmt::BayerBG8 => DynamicFrame::BayerBG8(bf!($w, $h, $s, $data, $ex)),
            PixFmt::BayerBG32f => DynamicFrame::BayerBG32f(bf!($w, $h, $s, $data, $ex)),
            PixFmt::BayerGB8 => DynamicFrame::BayerGB8(bf!($w, $h, $s, $data, $ex)),
            PixFmt::BayerGB32f => DynamicFrame::BayerGB32f(bf!($w, $h, $s, $data, $ex)),
            PixFmt::BayerGR8 => DynamicFrame::BayerGR8(bf!($w, $h, $s, $data, $ex)),
            PixFmt::BayerGR32f => DynamicFrame::BayerGR32f(bf!($w, $h, $s, $data, $ex)),
            PixFmt::YUV422 => DynamicFrame::YUV422(bf!($w, $h, $s, $data, $ex)),
            _ => {
                panic!("unsupported pixel format {}", $pixfmt);
            }
        })
    }};
}

pub struct FMFReader {
    f: Box<dyn Read>,
    pixel_format: PixFmt,
    height: u32,
    width: u32,
    image_data_size: usize,
    n_frames: usize,
    count: usize,
    did_error: bool,
}

impl FMFReader {
    pub fn new<P: AsRef<Path>>(path: P) -> FMFResult<FMFReader> {
        let extension = path.as_ref().extension().map(|x| x.to_str()).flatten();
        let mut f: Box<dyn Read> = if extension == Some("gz") {
            let gz_fd = std::fs::File::open(&path).map_err(|e| FMFError::IoPath {
                source: e,
                path: path.as_ref().display().to_string(),
                #[cfg(feature = "backtrace")]
                backtrace: std::backtrace::Backtrace::capture(),
            })?;
            let decoder = libflate::gzip::Decoder::new(gz_fd)?;
            Box::new(decoder)
        } else {
            Box::new(File::open(&path).map_err(|e| FMFError::IoPath {
                source: e,
                path: path.as_ref().display().to_string(),
                #[cfg(feature = "backtrace")]
                backtrace: std::backtrace::Backtrace::capture(),
            })?)
        };

        // version
        let mut pos = 0;
        let version = f.read_u32::<LittleEndian>()?;
        pos += 4;
        if version != 3 {
            return Err(FMFError::UnimplementedVersion);
        }

        // format
        let expected_format_len = f.read_u32::<LittleEndian>()? as usize;
        pos += 4;
        let mut format: Vec<u8> = vec![0; expected_format_len];
        let actual_format_len = f.read(&mut format)?;
        pos += actual_format_len;
        if expected_format_len != actual_format_len {
            return Err(FMFError::PrematureFileEnd);
        }
        let pixel_format = pixel_formats::get_pixel_format(&format)?;

        let _bpp = f.read_u32::<LittleEndian>()?;
        pos += 4;
        let height = f.read_u32::<LittleEndian>()?;
        pos += 4;
        let width = f.read_u32::<LittleEndian>()?;
        pos += 4;
        let chunksize: usize = f.read_u64::<LittleEndian>()?.try_into().unwrap();
        assert!(chunksize > TIMESTAMP_SIZE);
        let image_data_size = chunksize - TIMESTAMP_SIZE;
        pos += 8;
        let n_frames = f.read_u64::<LittleEndian>()?.try_into().unwrap();
        pos += 8;
        let _frame0_pos = pos;
        let count = 0;

        Ok(Self {
            f,
            pixel_format,
            height,
            width,
            image_data_size,
            n_frames,
            count,
            did_error: false,
        })
    }

    #[inline]
    pub fn width(&self) -> u32 {
        self.width
    }

    #[inline]
    pub fn height(&self) -> u32 {
        self.height
    }

    #[inline]
    pub fn format(&self) -> PixFmt {
        self.pixel_format
    }

    fn next_frame(&mut self) -> FMFResult<DynamicFrame> {
        debug_assert!(self.count < self.n_frames);

        let f = &mut self.f;

        let mut timestamp_data: Vec<u8> = vec![0; TIMESTAMP_SIZE];
        f.read_exact(&mut timestamp_data)?;

        let mut image_data: Vec<u8> = vec![0; self.image_data_size];
        f.read_exact(&mut image_data)?;

        let timestamp_f64 = timestamp_data.as_slice().read_f64::<LittleEndian>()?;
        let host_timestamp = f64_to_datetime(timestamp_f64);

        let width = self.width;
        let height = self.height;
        let pixel_format = self.pixel_format;
        let bpp = self.pixel_format.bits_per_pixel() as u32;
        let stride = (width * bpp) / 8;
        let host_framenumber = self.count;
        self.count += 1;

        let extra = Box::new(BasicExtra {
            host_timestamp,
            host_framenumber,
        });

        to_dynamic!(pixel_format, width, height, stride, image_data, extra)
    }
}

impl Iterator for FMFReader {
    type Item = FMFResult<DynamicFrame>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.did_error {
            // Encountered error. Do not read more.
            return None;
        }

        if self.count >= self.n_frames {
            // Done reading all frames. Do not read more.
            return None;
        }

        let frame = self.next_frame();
        if frame.is_err() {
            self.did_error = true;
        }
        Some(frame)
    }
}
