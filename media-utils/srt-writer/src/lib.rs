use std::{io::Result, io::Write, time::Duration};

trait Srt {
    fn srt(&self) -> String;
}

impl Srt for Duration {
    fn srt(&self) -> String {
        // from https://en.wikipedia.org/wiki/SubRip :
        // "hours:minutes:seconds,milliseconds with time units fixed to two
        // zero-padded digits and fractions fixed to three zero-padded digits
        // (00:00:00,000). The fractional separator used is the comma, since the
        // program was written in France."
        let total_secs = self.as_secs();
        let hours = total_secs / (60 * 60);
        let minutes = (total_secs % (60 * 60)) / 60;
        let seconds = total_secs % 60;
        debug_assert_eq!(total_secs, hours * 60 * 60 + minutes * 60 + seconds);
        let millis = self.subsec_millis();
        format!("{hours:02}:{minutes:02}:{seconds:02},{millis:03}")
    }
}

pub struct SrtWriter {
    wtr: Box<dyn Write>,
    count: usize,
}

impl SrtWriter {
    pub fn new(wtr: Box<dyn Write>) -> Self {
        Self { wtr, count: 1 }
    }

    pub fn append(&mut self, start: Duration, stop: Duration, value: &str) -> Result<()> {
        self.wtr.write_all(
            format!(
                "{count}\n{start} --> {stop}\n{value}\n\n",
                count = self.count,
                start = start.srt(),
                stop = stop.srt(),
            )
            .as_bytes(),
        )?;
        self.count += 1;
        Ok(())
    }

    pub fn flush(&mut self) -> Result<()> {
        self.wtr.flush()
    }

    pub fn into_inner(self) -> Result<Box<dyn Write>> {
        Ok(self.wtr)
    }
}

/// A buffering [SrtWriter] which is meant to be called for every frame.
///
/// This buffers values from each frame so that it can calculate start and stop
/// times for each frames. The first write thus only stores the buffer, and the
/// buffer is written upon [Self::drop].
pub struct BufferingSrtFrameWriter {
    wtr: SrtWriter,
    prev: Option<(Duration, String)>,
}

impl BufferingSrtFrameWriter {
    pub fn new(wtr: Box<dyn Write>) -> Self {
        Self {
            wtr: SrtWriter::new(wtr),
            prev: None,
        }
    }
    pub fn add_frame(&mut self, pts: Duration, val: String) -> Result<()> {
        if let Some((prev_pts, prev_value)) = self.prev.take() {
            // write buffered value
            self.wtr.append(prev_pts, pts, &prev_value).unwrap()
        }
        // store current value
        self.prev = Some((pts, val));
        Ok(())
    }

    /// Flush the underlying writer.
    ///
    /// Note that this does not flush the currently buffered value, as that
    /// would require creating a new timestamp.
    pub fn flush(&mut self) -> Result<()> {
        self.wtr.flush()
    }
}

impl Drop for BufferingSrtFrameWriter {
    fn drop(&mut self) {
        if let Some((pts, value)) = self.prev.take() {
            // invent timestamp in the future
            let future_pts = pts + Duration::from_secs(1);
            self.wtr.append(pts, future_pts, &value).unwrap()
        }
        self.wtr.flush().unwrap()
    }
}
