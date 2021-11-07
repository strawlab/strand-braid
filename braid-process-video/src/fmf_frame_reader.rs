use anyhow::{Context as ContextTrait, Result};
use chrono::{DateTime, Utc};

use basic_frame::DynamicFrame;
use timestamped_frame::ExtraTimeData;

use crate::{Frame, MovieReader};

pub struct FmfFrameReader {
    filename: String,
    rdr: fmf::reader::FMFReader,
    /// upon file open, we already read the first frame
    frame0: Option<DynamicFrame>,
    creation_time: DateTime<Utc>,
}

impl FmfFrameReader {
    pub fn new(filename: &str) -> Result<Self> {
        let mut rdr = fmf::reader::FMFReader::new(filename)
            .with_context(|| anyhow::anyhow!("Error from FMFReader opening '{}'", &filename))?;
        let frame0 = rdr
            .next()
            .map(|f| f.map_err(anyhow::Error::from))
            .unwrap_or_else(|| anyhow::bail!("fmf file with no data '{}'", &filename))?;
        let creation_time = frame0.extra().host_timestamp();
        let filename = filename.to_string();
        Ok(Self {
            filename,
            rdr,
            frame0: Some(frame0),
            creation_time,
        })
    }
}

impl MovieReader for FmfFrameReader {
    fn title(&self) -> Option<&str> {
        // There is no metadata, such as a title, in an FMF file.
        None
    }
    fn filename(&self) -> &str {
        &self.filename
    }
    fn creation_time(&self) -> &DateTime<Utc> {
        &self.creation_time
    }

    /// Get the next frame
    fn next_frame(&mut self) -> Option<Result<Frame>> {
        if let Some(frame0) = self.frame0.take() {
            Some(frame0.try_into().map_err(anyhow::Error::from))
        } else {
            self.rdr
                .next()
                .map(|f| f.map(|f| f.try_into())?.map_err(anyhow::Error::from))
        }
    }
}
