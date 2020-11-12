use chrono;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use byteorder::{LittleEndian, ReadBytesExt};

use datetime_conversion::f64_to_datetime;
use formats::PixelFormat;

use crate::{get_pixel_format, FMFError, FMFFrame, FMFResult};

pub struct FMFReader {
    f: File,
    pixel_format: PixelFormat,
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
        let pixel_format = get_pixel_format(&format)?;

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
    pub fn format(&self) -> PixelFormat {
        self.pixel_format
    }

    fn next_frame(&mut self) -> FMFResult<FMFFrame> {
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
        let bpp = match self.pixel_format.bits_per_pixel() {
            None => {
                return Err(FMFError::UnimplementedPixelFormat(pixel_format));
            }
            Some(bpp) => bpp.get() as u32,
        };
        let stride = (width * bpp) / 8;
        let host_framenumber = self.count;
        self.count += 1;

        // TODO XXX FIXME: check this timezone code is actually reasonable.
        let host_timestamp = host_timestamp_local.with_timezone(&chrono::Utc);

        Ok(FMFFrame {
            width,
            height,
            stride,
            image_data,
            host_timestamp,
            host_framenumber,
            pixel_format,
        })
    }
}

impl Iterator for FMFReader {
    type Item = FMFFrame;
    fn next(&mut self) -> Option<Self::Item> {
        match self.next_frame() {
            Ok(f) => Some(f),
            Err(_) => None,
        }
    }
}
