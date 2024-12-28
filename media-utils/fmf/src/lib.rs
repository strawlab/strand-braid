extern crate basic_frame;
extern crate byteorder;
extern crate chrono;
extern crate machine_vision_formats as formats;

extern crate datetime_conversion;

use std::f64;
use std::io::{Seek, SeekFrom, Write};

use byteorder::{LittleEndian, WriteBytesExt};
use formats::{ImageStride, PixFmt, PixelFormat};

pub type FMFResult<M> = std::result::Result<M, FMFError>;

pub(crate) mod pixel_formats;

#[derive(thiserror::Error, Debug)]
pub enum FMFError {
    #[error("unexpected size")]
    UnexpectedSize,
    #[error("unexpected pixel_format {0} (expected {1})")]
    UnexpectedEncoding(PixFmt, PixFmt),
    #[error("unimplemented pixel_format {0}")]
    UnimplementedPixelFormat(PixFmt),

    #[error("Unimplemented FMF file version {0}. Only FMF v3 files supported.")]
    UnimplementedVersion(u32),
    #[error("premature file end")]
    PrematureFileEnd,
    #[error("unknown format {0}")]
    UnknownFormat(String),
    #[error("inconsistent state")]
    InconsistentState,
    #[error("already closed")]
    AlreadyClosed,

    #[error("reading past the end of the file")]
    ReadingPastEnd,

    #[error("{source}")]
    Io {
        #[from]
        source: std::io::Error,

    },

    #[error("From {path}: {source}")]
    IoPath {
        path: String,
        #[source]
        source: std::io::Error,

    },

    #[error("{0}")]
    Cell(std::cell::BorrowMutError),
}

impl From<std::cell::BorrowMutError> for FMFError {
    fn from(orig: std::cell::BorrowMutError) -> FMFError {
        FMFError::Cell(orig)
    }
}

pub mod reader;
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
    e: PixFmt,
    row_bytes: usize,
    n_frames_pos_bytes: u64,
    n_frames: u64,
}

// Things to improve in FMF v 4:
//  * Specify a [file signature](https://en.wikipedia.org/wiki/List_of_file_signatures).
//  * Specify that timestamp is in UTC. (Provide timezone in header?)
//  * Provide magic number at start of file.
//  * Allow ability to save arbitrary (timestamp,event) data or
//    (timestamp,value) data
//  * Save camera device timestamps and host computer timestamps and experiment
//    timestamps (or minimally specify which)
//  * Allow metadata such as camera name.
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
    pub fn write<TZ, FMT>(
        &mut self,
        frame: &dyn ImageStride<FMT>,
        dtl: chrono::DateTime<TZ>,
    ) -> FMFResult<()>
    where
        TZ: chrono::TimeZone,
        FMT: PixelFormat,
    {
        let timestamp = datetime_conversion::datetime_to_f64(&dtl);

        // We need this dance with InconsistentState to prevent moving out of
        // borrowed struct. TODO: remove it.
        let state = std::mem::replace(&mut self.state, WriterState::InconsistentState);
        let new_state = match state {
            WriterState::FileOpened(f) => {
                let inner = FMFWriterInner::new(
                    f,
                    frame.width(),
                    frame.height(),
                    machine_vision_formats::pixel_format::pixfmt::<FMT>().unwrap(),
                )?;
                WriterState::Writing(inner)
            }
            WriterState::Writing(inner) => WriterState::Writing(inner),
            WriterState::InconsistentState => return Err(FMFError::InconsistentState),
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
                if machine_vision_formats::pixel_format::pixfmt::<FMT>().unwrap() != inner.e {
                    return Err(FMFError::UnexpectedEncoding(
                        machine_vision_formats::pixel_format::pixfmt::<FMT>().unwrap(),
                        inner.e,
                    ));
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
            WriterState::InconsistentState => Err(FMFError::InconsistentState),
        }
    }
}

impl<F: Write + Seek> FMFWriterInner<F> {
    fn new(mut f: F, w: u32, h: u32, pixel_format: PixFmt) -> FMFResult<Self> {
        let format = pixel_formats::get_format(pixel_format)?;

        let bytes_per_pixel = pixel_format.bits_per_pixel() / 8;

        let row_bytes = w as usize * bytes_per_pixel as usize;
        let chunksize = row_bytes * h as usize + 8;

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

    fn write_inner<FMT>(&mut self, frame: &dyn ImageStride<FMT>, timestamp: f64) -> FMFResult<()>
    where
        FMT: PixelFormat,
    {
        let self_f = match self.f {
            Some(ref mut f) => f,
            None => {
                return Err(FMFError::AlreadyClosed);
            }
        };
        self_f.write_f64::<LittleEndian>(timestamp)?;

        // `frame` might be a region of interest into an outer frame. In this
        // case, `width*bytes_per_pixel` may be less than stride, and we do not
        // want to write all these bytes.

        let image_data = frame.image_data();
        let e = machine_vision_formats::pixel_format::pixfmt::<FMT>().unwrap();
        let bpp = e.bits_per_pixel();
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
                return Err(FMFError::AlreadyClosed);
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

#[cfg(test)]
mod tests {
    use super::FMFWriter;
    use basic_frame::{BasicExtra, BasicFrame};

    use timestamped_frame::ExtraTimeData;

    use chrono::{DateTime, Local};
    use machine_vision_formats::pixel_format::Mono8;

    fn zeros(w: u32, h: u32) -> BasicFrame<Mono8> {
        let mut image_data = Vec::new();
        image_data.resize((w * h) as usize, 0);
        let local: DateTime<Local> = Local::now();
        let host_timestamp = local.with_timezone(&chrono::Utc);
        let extra = Box::new(BasicExtra {
            host_timestamp,
            host_framenumber: 0,
        });

        BasicFrame {
            width: w,
            height: h,
            stride: w,
            image_data,
            extra,
            pixel_format: std::marker::PhantomData,
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
        writer.write(&f1, f1.extra().host_timestamp()).unwrap();

        let f2 = zeros(w, h);
        writer.write(&f2, f2.extra().host_timestamp()).unwrap();

        let f3 = zeros(w, h);
        writer.write(&f3, f3.extra().host_timestamp()).unwrap();

        let f = writer.close().unwrap();

        // check what was writen
        let buf = f.into_inner();

        let expected = [3, 0, 0, 0, 5, 0, 0, 0, 77, 79]; // TODO improve test
        assert_eq!(&buf[0..10], expected);
    }
}
