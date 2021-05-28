use chrono;
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
    f: File,
    pixel_format: PixFmt,
    height: u32,
    width: u32,
    chunksize: u64,
    // n_frames: u64,
    // pos: usize,
    // frame0_pos: usize,
    count: usize,
}

impl FMFReader {
    pub fn new<P: AsRef<Path>>(path: P) -> FMFResult<FMFReader> {
        let mut f = File::open(&path).map_err(|e| FMFError::IoPath {
            source: e,
            path: path.as_ref().display().to_string(),
        })?;

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
        let chunksize = f.read_u64::<LittleEndian>()?;
        pos += 8;
        let _n_frames = f.read_u64::<LittleEndian>()?;
        pos += 8;
        let _frame0_pos = pos;
        let count = 0;

        Ok(Self {
            f,
            pixel_format,
            height,
            width,
            chunksize,
            /*n_frames, pos, frame0_pos,*/ count,
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
        let f = &mut self.f;

        let timestamp_f64 = f.read_f64::<LittleEndian>()?;
        let host_timestamp_local = f64_to_datetime(timestamp_f64);

        let datasize = (self.chunksize - 8) as usize;
        let mut image_data: Vec<u8> = vec![0; datasize];
        let actual_data_len = f.read(&mut image_data)?;

        if actual_data_len < datasize {
            return Err(FMFError::PrematureFileEnd);
        }

        let width = self.width;
        let height = self.height;
        let pixel_format = self.pixel_format;
        let bpp = self.pixel_format.bits_per_pixel() as u32;
        let stride = (width * bpp) / 8;
        let host_framenumber = self.count;
        self.count += 1;

        // TODO XXX FIXME: check this timezone code is actually reasonable.
        let extra = Box::new(BasicExtra {
            host_timestamp: host_timestamp_local.with_timezone(&chrono::Utc),
            host_framenumber,
        });

        to_dynamic!(pixel_format, width, height, stride, image_data, extra)
    }
}

impl Iterator for FMFReader {
    type Item = DynamicFrame;
    fn next(&mut self) -> Option<Self::Item> {
        match self.next_frame() {
            Ok(f) => Some(f),
            Err(_) => None,
        }
    }
}
