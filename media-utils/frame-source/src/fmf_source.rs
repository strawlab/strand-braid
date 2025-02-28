use crate::{FrameData, FrameDataSource, ImageData, Result, Timestamp};
use fmf::reader::FMFReader;
use std::path::Path;

struct FmfSourceIter {
    rdr: FMFReader,
    frame0_time_utc: chrono::DateTime<chrono::Utc>,
    idx: usize,
}
impl FmfSourceIter {
    fn new(parent: &FmfSource) -> Result<Self> {
        let mut rdr = FMFReader::new(&parent.filename)?;
        let frame0_time_utc = parent.frame0_time_utc;
        for _ in 0..parent.skip_frames {
            rdr.next();
        }
        Ok(Self {
            rdr,
            frame0_time_utc,
            idx: 0,
        })
    }
}
impl Iterator for FmfSourceIter {
    type Item = Result<FrameData>;
    fn next(&mut self) -> Option<Self::Item> {
        let pos_start = self.rdr.file_pos();
        self.rdr.next().map(|fmf_result| match fmf_result {
            Ok((frame, frame_time_utc)) => {
                let pos_end = self.rdr.file_pos();
                let buf_len = pos_end - pos_start;
                let timestamp = frame_time_utc - self.frame0_time_utc;
                let timestamp = Timestamp::Duration(timestamp.to_std()?);
                let idx = self.idx;
                self.idx += 1;
                Ok(FrameData {
                    image: ImageData::Decoded(frame),
                    timestamp,
                    buf_len,
                    idx,
                })
            }
            Err(e) => Err(e.into()),
        })
    }
}

// Because of the need to create an iterator over the frames an arbitrary number
// of times but the inability of `FMFReader` to seek (due to underlying
// potential use of a .gz file reader which does not support seeking), we store
// the filename and repeatedly reopen the file as necessary. An an optimization,
// the opened reader and its last read frame could be kept in a cache. This
// would reduce the number of re-openings.
pub struct FmfSource {
    filename: std::path::PathBuf,
    width: u32,
    height: u32,
    frame0_time_utc: chrono::DateTime<chrono::Utc>,
    frame0_time: chrono::DateTime<chrono::FixedOffset>,
    skip_frames: usize,
}

impl FrameDataSource for FmfSource {
    fn width(&self) -> u32 {
        self.width
    }
    fn height(&self) -> u32 {
        self.height
    }
    fn frame0_time(&self) -> Option<chrono::DateTime<chrono::FixedOffset>> {
        Some(self.frame0_time)
    }
    fn skip_n_frames(&mut self, n_frames: usize) -> Result<()> {
        if n_frames == 0 {
            return Ok(());
        }
        let mut rdr = FMFReader::new(&self.filename)?;

        let mut frame_timestamp = None;
        for _ in 0..n_frames {
            frame_timestamp = rdr.next()
        }

        let frame_timestamp = frame_timestamp
            .map(|f| f.map_err(Into::into))
            .unwrap_or_else(|| Err(crate::Error::FmfWithNotEnoughData))?;

        let (_frame, frame_time_utc) = frame_timestamp;
        let duration = frame_time_utc - self.frame0_time_utc;
        let frame_time = self.frame0_time + duration;

        self.skip_frames = n_frames;
        self.frame0_time = frame_time;
        self.frame0_time_utc = frame_time_utc;
        Ok(())
    }
    fn average_framerate(&self) -> Option<f64> {
        None
    }
    fn estimate_luminance_range(&mut self) -> Result<(u16, u16)> {
        // FMF reader does not support seek because we may read .gz files.
        Err(crate::Error::UnsupportedForEsimatingLuminangeRange)
    }
    fn iter(&mut self) -> Box<dyn Iterator<Item = Result<FrameData>>> {
        Box::new(FmfSourceIter::new(self).unwrap())
    }
    fn timestamp_source(&self) -> &str {
        "FMF frame metadata"
    }
    fn has_timestamps(&self) -> bool {
        true
    }
}

impl FmfSource {
    fn new<P: AsRef<std::path::Path>>(filename: P) -> Result<Self> {
        let filename = filename.as_ref().to_path_buf();
        let mut rdr = FMFReader::new(&filename)?;
        let width = rdr.width();
        let height = rdr.height();
        let frame_timestamp0 = rdr
            .next()
            .map(|f| f.map_err(crate::Error::from))
            .unwrap_or_else(|| Err(crate::Error::FmfWithNotEnoughData))?;

        let (_frame0, frame0_time_utc) = frame_timestamp0;
        let frame0_time = mkv_strand_reader::infer_timezone(&frame0_time_utc, filename.to_str())?;

        Ok(Self {
            filename,
            width,
            height,
            frame0_time_utc,
            frame0_time,
            skip_frames: 0,
        })
    }
}

pub fn from_path<P: AsRef<Path>>(path: P) -> Result<FmfSource> {
    let filename = path.as_ref();
    FmfSource::new(filename)
}
