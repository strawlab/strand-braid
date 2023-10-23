use std::{fs::File, io::BufReader};

use anyhow::Result;
use csv::DeserializeRecordsIntoIter;

use csv_eof::{EarlyEofOk, TerminateEarlyOnUnexpectedEof};
use flydra_types::{FlydraFloatTimestampLocal, KalmanEstimatesRow, Triggerbox};
use zip_or_dir::{MaybeGzReader, ZipDirArchive};

type KItem = std::result::Result<KalmanEstimatesRow, csv::Error>;

pub struct ChunkIter<I>
where
    I: Iterator<Item = KItem>,
{
    start_stamp: chrono::DateTime<chrono::Utc>,
    dur: std::time::Duration,
    next_chunk_index: usize,
    inner: std::iter::Peekable<I>,
}

impl<I> Iterator for ChunkIter<I>
where
    I: Iterator<Item = KItem>,
{
    type Item = DurationChunk;
    fn next(&mut self) -> Option<Self::Item> {
        let cur: u32 = self.next_chunk_index.try_into().unwrap();
        let next = cur + 1;
        let stop_dur = self.dur * next;
        let stop_time = FlydraFloatTimestampLocal::<Triggerbox>::from(self.start_stamp + stop_dur);
        self.next_chunk_index += 1;

        let mut rows: Vec<KalmanEstimatesRow> = vec![];
        let mut do_return_rows = false;
        while let Some(Ok(peek_row)) = self.inner.peek() {
            if let Some(ref this_timestamp) = &peek_row.timestamp {
                if this_timestamp.as_f64() >= stop_time.as_f64() {
                    do_return_rows = true;
                    // done iterating
                    break;
                }
            } else {
                // return Error - no timestamp on row
                panic!("row {} has no timestamp", peek_row.frame);
            }
            do_return_rows = true;
            rows.push(self.inner.next().unwrap().unwrap());
        }

        if do_return_rows {
            Some(DurationChunk { rows })
        } else {
            None
        }
    }
}

#[derive(Debug)]
pub struct DurationChunk {
    pub rows: Vec<KalmanEstimatesRow>,
}

pub trait ToChunkIter<I>
where
    I: Iterator<Item = KItem>,
{
    fn to_chunk_iter(
        self,
        first_row: KalmanEstimatesRow,
        dur: std::time::Duration,
    ) -> Result<ChunkIter<I>>;
}

impl<I> ToChunkIter<I> for I
where
    I: Iterator<Item = KItem>,
{
    fn to_chunk_iter(
        self,
        first_row: KalmanEstimatesRow,
        dur: std::time::Duration,
    ) -> Result<ChunkIter<I>> {
        let start_stamp = first_row
            .timestamp
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no timestamp in first row"))?;
        let start_stamp = start_stamp.into();
        Ok(ChunkIter {
            start_stamp,
            dur,
            next_chunk_index: 0,
            inner: self.peekable(),
        })
    }
}

pub fn chunk_by_duration<'a>(
    archive: &'a mut ZipDirArchive<BufReader<File>>,
    dur: std::time::Duration,
) -> Result<
    ChunkIter<
        TerminateEarlyOnUnexpectedEof<
            DeserializeRecordsIntoIter<MaybeGzReader<'a>, KalmanEstimatesRow>,
            KalmanEstimatesRow,
        >,
    >,
> {
    let mut first_row = None;
    let src_fname = flydra_types::KALMAN_ESTIMATES_CSV_FNAME;

    {
        let rdr = archive.open_raw_or_gz(src_fname)?;
        let kest_reader = csv::Reader::from_reader(rdr);

        if let Some(row) = kest_reader.into_deserialize().early_eof_ok().next() {
            let row = row?;
            first_row = Some(row);
        }
    }
    if let Some(first_row) = first_row {
        let rdr = archive.open_raw_or_gz(src_fname)?;
        let t1: csv::Reader<MaybeGzReader<'a>> = csv::Reader::from_reader(rdr);

        let inner_iter = t1.into_deserialize().early_eof_ok();
        Ok(ToChunkIter::to_chunk_iter(inner_iter, first_row, dur)?)
    } else {
        anyhow::bail!("no rows in {src_fname}");
    }
}
