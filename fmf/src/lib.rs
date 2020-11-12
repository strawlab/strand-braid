extern crate basic_frame;
extern crate byteorder;
extern crate chrono;
extern crate machine_vision_formats as formats;

extern crate datetime_conversion;

use std::f64;
use std::io::{Seek, SeekFrom, Write};

use crate::formats::{ImageStride, PixelFormat};
use byteorder::{LittleEndian, WriteBytesExt};

pub type FMFResult<M> = std::result::Result<M, FMFError>;

#[derive(thiserror::Error, Debug)]
pub enum FMFError {
    #[error("unexpected size")]
    UnexpectedSize,
    #[error("unexpected pixel_format {0} (expected {1})")]
    UnexpectedEncoding(PixelFormat, PixelFormat),
    #[error("unimplemented pixel_format {0}")]
    UnimplementedPixelFormat(PixelFormat),

    #[error("unimplemented version")]
    UnimplementedVersion,
    #[error("premature file end")]
    PrematureFileEnd,
    #[error("unknown format {0}")] // TODO render utf-8 component?
    UnknownFormat(String),
    #[error("inconsistent state")]
    InconsistentState,
    #[error("already closed")]
    AlreadyClosed,

    #[error("{0}")]
    Io(std::io::Error),

    #[error("From {path}: {source}")]
    IoPath {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("{0}")]
    Cell(std::cell::BorrowMutError),
}

impl From<std::io::Error> for FMFError {
    fn from(orig: std::io::Error) -> FMFError {
        FMFError::Io(orig)
    }
}

impl From<std::cell::BorrowMutError> for FMFError {
    fn from(orig: std::cell::BorrowMutError) -> FMFError {
        FMFError::Cell(orig)
    }
}

pub type FMFFrame = basic_frame::BasicFrame;

mod reader;
pub use crate::reader::FMFReader;

/// Writes FMF (fly movie format) movie files.
///
/// The FMF format is very simple and writes a fixed sized chunk of bytes to
/// disk on every frame. This allows random access to individual frames. The
/// bytes are not compressed but rather store the raw image bytes.
pub struct FMFWriter<F: Write + Seek> {
    state: WriterState<F>,
}

enum WriterState<F: Write + Seek> {
    FileOpened(F),
    Writing(FMFWriterInner<F>),
    InconsistentState,
}

struct FMFWriterInner<F: Write + Seek> {
    f: Option<F>,
    w: u32,
    h: u32,
    e: PixelFormat,
    row_bytes: usize,
    n_frames_pos_bytes: u64,
    n_frames: u64,
}

// Things to improve in FMF v 4:
//  * Specify that timestamp is in UTC. (Provide timezone in header?)
//  * Provide magic number at start of file.
//  * Allow ability to save arbitrary (timestamp,event) data or
//    (timestamp,value) data
//  * Save camera device timestamps and host computer timestamps and experiment
//    timestamps (or minimally specify which)
//
// Note also the file replacing-fmf.md which discusses eliminating FMF
// completely.

impl<F: Write + Seek> FMFWriter<F> {
    /// Open a new writer.
    pub fn new(f: F) -> FMFResult<Self> {
        Ok(Self {
            state: WriterState::FileOpened(f),
        })
    }

    /// Write a frame.
    pub fn write<TZ>(&mut self, frame: &dyn ImageStride, dtl: chrono::DateTime<TZ>) -> FMFResult<()>
    where
        TZ: chrono::TimeZone,
    {
        let timestamp = datetime_conversion::datetime_to_f64(&dtl);

        // We need this dance with InconsistentState to prevent moving out of
        // borrowed struct. TODO: remove it.
        let state = std::mem::replace(&mut self.state, WriterState::InconsistentState);
        let new_state = match state {
            WriterState::FileOpened(f) => {
                let inner =
                    FMFWriterInner::new(f, frame.width(), frame.height(), frame.pixel_format())?;
                WriterState::Writing(inner)
            }
            WriterState::Writing(inner) => WriterState::Writing(inner),
            WriterState::InconsistentState => return Err(FMFError::InconsistentState.into()),
        };
        self.state = new_state;

        match self.state {
            WriterState::Writing(ref mut inner) => {
                if (frame.width() != inner.w)
                    || (frame.height() != inner.h)
                    || (inner.row_bytes > frame.stride())
                {
                    return Err(FMFError::UnexpectedSize);
                }
                if frame.pixel_format() != inner.e {
                    return Err(FMFError::UnexpectedEncoding(frame.pixel_format(), inner.e));
                }
                inner.write_inner(frame, timestamp)?;
            }
            _ => {
                return Err(FMFError::InconsistentState);
            }
        }
        Ok(())
    }

    /// Close the writer.
    ///
    /// Ideally, this is called prior to dropping to prevent the possibility of
    /// silently ignoring errors.
    pub fn close(self) -> FMFResult<F> {
        match self.state {
            WriterState::FileOpened(f) => Ok(f),
            WriterState::Writing(mut inner) => inner.close(),
            WriterState::InconsistentState => Err(FMFError::InconsistentState.into()),
        }
    }
}

impl<F: Write + Seek> FMFWriterInner<F> {
    fn new(mut f: F, w: u32, h: u32, pixel_format: PixelFormat) -> FMFResult<Self> {
        let format = get_format(pixel_format)?;

        let bytes_per_pixel = match pixel_format.bits_per_pixel() {
            Some(bit_per_pixel) => bit_per_pixel.get() / 8,
            None => {
                return Err(FMFError::UnimplementedPixelFormat(pixel_format));
            }
        };

        let row_bytes = w as usize * bytes_per_pixel as usize;
        let chunksize = (row_bytes * h as usize + 8) as usize;

        let mut pos = 0;
        f.write_u32::<LittleEndian>(3)?;
        pos += 4; // FMF version = 3
        f.write_u32::<LittleEndian>(format.len() as u32)?;
        pos += 4;
        f.write_all(&format)?;
        pos += format.len();
        f.write_u32::<LittleEndian>(bytes_per_pixel as u32 * 8)?;
        pos += 4;
        f.write_u32::<LittleEndian>(h)?;
        pos += 4;
        f.write_u32::<LittleEndian>(w)?;
        pos += 4;
        f.write_u64::<LittleEndian>(chunksize as u64)?;
        pos += 8;
        f.write_u64::<LittleEndian>(0)?; // n_frames = 0

        let f = Some(f);

        Ok(Self {
            f,
            w,
            h,
            e: pixel_format,
            row_bytes,
            n_frames_pos_bytes: pos as u64,
            n_frames: 0,
        })
    }

    fn write_inner(&mut self, frame: &dyn ImageStride, timestamp: f64) -> FMFResult<()> {
        let self_f = match self.f {
            Some(ref mut f) => f,
            None => {
                return Err(FMFError::AlreadyClosed.into());
            }
        };
        self_f.write_f64::<LittleEndian>(timestamp)?;

        // `frame` might be a region of interest into an outer frame. In this
        // case, `width*bytes_per_pixel` may be less than stride, and we do not
        // want to write all these bytes.

        let image_data = frame.image_data();
        let e = frame.pixel_format();
        let bpp = match e.bits_per_pixel() {
            Some(nz) => nz.get(),
            None => {
                return Err(FMFError::UnimplementedPixelFormat(e));
            }
        };
        let n_bytes_per_row = self.w as usize * (bpp / 8) as usize;
        let mut ptr = 0;
        for _ in 0..self.h {
            let end = ptr + n_bytes_per_row;
            let row_buf = &image_data[ptr..end];
            self_f.write_all(row_buf)?;
            ptr += frame.stride();
        }

        self.n_frames += 1;

        Ok(())
    }

    /// Close the writer.
    ///
    /// Ideally, this is called prior to dropping to prevent the possibility of
    /// silently ignoring errors.
    fn close(&mut self) -> FMFResult<F> {
        let opt_f = self.f.take();

        let mut self_f = match opt_f {
            Some(f) => f,
            None => {
                return Err(FMFError::AlreadyClosed.into());
            }
        };

        // Write n_frames to the file header.
        self_f.seek(SeekFrom::Start(self.n_frames_pos_bytes))?;
        self_f.write_u64::<LittleEndian>(self.n_frames)?;
        self_f.flush()?;
        Ok(self_f)
    }
}

/// This will silently ignore any error.
impl<F: Write + Seek> Drop for FMFWriterInner<F> {
    fn drop(&mut self) {
        // We silently drop error.
        match self.close() {
            Ok(_f) => {}
            Err(_e) => {} // See https://github.com/rust-lang/rfcs/issues/814
        }
    }
}

fn get_format(pixel_format: PixelFormat) -> FMFResult<Vec<u8>> {
    let r = match pixel_format {
        PixelFormat::MONO8 => b"MONO8".to_vec(),
        PixelFormat::BayerRG8 => b"RAW8:RGGB".to_vec(),
        PixelFormat::BayerGB8 => b"RAW8:GBRG".to_vec(),
        PixelFormat::BayerGR8 => b"RAW8:GRBG".to_vec(),
        PixelFormat::BayerBG8 => b"RAW8:BGGR".to_vec(),
        PixelFormat::YUV422 => b"YUV422".to_vec(),
        PixelFormat::RGB8 => b"RGB8".to_vec(),
        e => {
            return Err(FMFError::UnimplementedPixelFormat(e));
        }
    };
    Ok(r)
}

fn get_pixel_format(format: &[u8]) -> FMFResult<PixelFormat> {
    match format {
        b"MONO8" => Ok(PixelFormat::MONO8),
        b"RAW8:RGGB" | b"MONO8:RGGB" => Ok(PixelFormat::BayerRG8),
        b"RAW8:GBRG" | b"MONO8:GBRG" => Ok(PixelFormat::BayerGB8),
        b"RAW8:GRBG" | b"MONO8:GRBG" => Ok(PixelFormat::BayerGR8),
        b"RAW8:BGGR" | b"MONO8:BGGR" => Ok(PixelFormat::BayerBG8),
        b"YUV422" => Ok(PixelFormat::YUV422),
        b"RGB8" => Ok(PixelFormat::RGB8),
        f => Err(FMFError::UnknownFormat(
            String::from_utf8_lossy(&f).into_owned(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{FMFFrame, FMFWriter};

    use timestamped_frame::HostTimeData;

    use crate::formats::PixelFormat;
    use chrono;
    use chrono::{DateTime, Local};

    fn zeros(w: u32, h: u32) -> FMFFrame {
        let mut image_data = Vec::new();
        image_data.resize((w * h) as usize, 0);
        let local: DateTime<Local> = Local::now();
        let host_timestamp = local.with_timezone(&chrono::Utc);

        FMFFrame {
            width: w,
            height: h,
            stride: w,
            image_data,
            pixel_format: PixelFormat::MONO8,
            host_timestamp,
            host_framenumber: 0,
        }
    }

    #[test]
    fn test_writing() {
        let w = 320;
        let h = 240;
        let f = std::io::Cursor::new(Vec::new());
        // create fmf
        let mut writer = FMFWriter::new(f).unwrap();

        // write some frames
        let f1 = zeros(w, h);
        writer.write(&f1, f1.host_timestamp()).unwrap();

        let f2 = zeros(w, h);
        writer.write(&f2, f2.host_timestamp()).unwrap();

        let f3 = zeros(w, h);
        writer.write(&f3, f3.host_timestamp()).unwrap();

        let f = writer.close().unwrap();

        // check what was writen
        let buf = f.into_inner();

        let expected = [3, 0, 0, 0, 5, 0, 0, 0, 77, 79]; // TODO improve test
        assert_eq!(&buf[0..10], expected);
    }
}
