use std::{fs::File, io::Read, path::Path};

use byteorder::{LittleEndian, ReadBytesExt};

use chrono::{DateTime, Utc};
use formats::PixFmt;
use strand_dynamic_frame::DynamicFrame;

use crate::{pixel_formats, FMFError, FMFResult};

const TIMESTAMP_SIZE: usize = 8;

fn open_buffered<P: AsRef<Path>>(p: &P) -> std::io::Result<std::io::BufReader<File>> {
    Ok(std::io::BufReader::new(File::open(p.as_ref())?))
}

pub struct FMFReader {
    // We cannot Seek because the gzip Decoder does not implement that.
    f: Box<dyn Read>,
    pixel_format: PixFmt,
    height: u32,
    width: u32,
    image_data_size: usize,
    // In theory, a corrupt file could have more frames than indicated by the
    // `n_frames` field in the header, but we assume the file is OK.
    n_frames: usize,
    count: usize,
    file_pos: usize,
    did_error: bool,
}

impl FMFReader {
    pub fn new<P: AsRef<Path>>(path: P) -> FMFResult<FMFReader> {
        let extension = path.as_ref().extension().and_then(|x| x.to_str());
        let mut f: Box<dyn Read> = if extension == Some("gz") {
            let gz_fd = open_buffered(&path).map_err(|e| FMFError::IoPath {
                source: e,
                path: path.as_ref().display().to_string(),
            })?;
            let decoder = libflate::gzip::Decoder::new(gz_fd)?;
            Box::new(decoder)
        } else {
            Box::new(open_buffered(&path).map_err(|e| FMFError::IoPath {
                source: e,
                path: path.as_ref().display().to_string(),
            })?)
        };

        // version
        let mut pos = 0;
        let version = f.read_u32::<LittleEndian>()?;
        pos += 4;
        if version != 3 {
            return Err(FMFError::UnimplementedVersion(version));
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
        let count = 0;

        Ok(Self {
            f,
            pixel_format,
            height,
            width,
            image_data_size,
            n_frames,
            count,
            file_pos: pos,
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

    pub fn file_pos(&self) -> usize {
        self.file_pos
    }

    /// Return the number of frames indicated in the header.
    pub fn n_frames(&self) -> usize {
        self.n_frames
    }

    fn next_frame(&mut self) -> FMFResult<(DynamicFrame, DateTime<Utc>)> {
        // Private function to actually read next frame.
        if self.count >= self.n_frames {
            return Err(FMFError::ReadingPastEnd);
        }

        let mut timestamp_data: Vec<u8> = vec![0; TIMESTAMP_SIZE];
        self.f.read_exact(&mut timestamp_data)?;
        self.file_pos += TIMESTAMP_SIZE;

        let mut image_data: Vec<u8> = vec![0; self.image_data_size];
        self.f.read_exact(&mut image_data)?;
        self.file_pos += self.image_data_size;

        let timestamp_f64 = timestamp_data.as_slice().read_f64::<LittleEndian>()?;
        let dt = datetime_conversion::f64_to_datetime(timestamp_f64);

        let width = self.width;
        let height = self.height;
        let pixel_format = self.pixel_format;
        let bpp = self.pixel_format.bits_per_pixel() as u32;
        let stride = (width * bpp) / 8;
        self.count += 1;

        if let Some(dframe) =
            DynamicFrame::new(width, height, stride as usize, image_data, pixel_format)
        {
            Ok((dframe, dt))
        } else {
            Err(FMFError::UnexpectedSize)
        }
    }
}

impl Iterator for FMFReader {
    type Item = FMFResult<(DynamicFrame, DateTime<Utc>)>;
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
