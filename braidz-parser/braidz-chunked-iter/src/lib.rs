// Copyright 2023 Andrew D. Straw.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT
// or http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use anyhow::Result;

use flydra_types::{FlydraFloatTimestampLocal, KalmanEstimatesRow, Triggerbox};

type KItem = std::result::Result<KalmanEstimatesRow, csv::Error>;

pub enum ChunkSize {
    TimestampDuration(std::time::Duration),
    FrameNumber(usize),
}

enum ChunkStartAndDuration {
    Timestamp(chrono::DateTime<chrono::Utc>, std::time::Duration),
    Frame(u64, usize),
}

pub struct ChunkIter<I>
where
    I: Iterator<Item = KItem>,
{
    source: ChunkStartAndDuration,
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

        let mut stop_time = None;
        let mut stop_frame = None;
        match &self.source {
            ChunkStartAndDuration::Timestamp(start_stamp, dur) => {
                let stop_dur = *dur * next;
                stop_time = Some(FlydraFloatTimestampLocal::<Triggerbox>::from(
                    *start_stamp + stop_dur,
                ));
            }
            ChunkStartAndDuration::Frame(start_frame, n_frames_in_chunk) => {
                let next_u64: u64 = next.try_into().unwrap();
                let n_frames_in_chunk_u64: u64 = (*n_frames_in_chunk).try_into().unwrap();
                let stop_dur = n_frames_in_chunk_u64 * next_u64;
                stop_frame = Some(*start_frame + stop_dur);
            }
        }
        self.next_chunk_index += 1;

        let mut rows: Vec<KalmanEstimatesRow> = vec![];
        let mut do_return_rows = false;
        while let Some(Ok(peek_row)) = self.inner.peek() {
            match &self.source {
                ChunkStartAndDuration::Timestamp(_start_stamp, _dur) => {
                    let stop_time = stop_time.as_ref().unwrap();

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
                }

                ChunkStartAndDuration::Frame(_start_frame, _n_frames_in_chunk) => {
                    let stop_frame = stop_frame.as_ref().unwrap();
                    let this_frame = &peek_row.frame.0;
                    if this_frame >= stop_frame {
                        do_return_rows = true;
                        // done iterating
                        break;
                    }
                }
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
    fn to_chunk_iter(self, first_row: KalmanEstimatesRow, sz: ChunkSize) -> Result<ChunkIter<I>>;
}

impl<I> ToChunkIter<I> for I
where
    I: Iterator<Item = KItem>,
{
    fn to_chunk_iter(self, first_row: KalmanEstimatesRow, sz: ChunkSize) -> Result<ChunkIter<I>> {
        let source = match sz {
            ChunkSize::TimestampDuration(dur) => {
                let start_stamp = first_row
                    .timestamp
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("no timestamp in first row"))?;
                let start_stamp = start_stamp.into();
                ChunkStartAndDuration::Timestamp(start_stamp, dur)
            }
            ChunkSize::FrameNumber(num_frames) => {
                let start_frame = first_row.frame.0;
                ChunkStartAndDuration::Frame(start_frame, num_frames)
            }
        };
        Ok(ChunkIter {
            source,
            next_chunk_index: 0,
            inner: self.peekable(),
        })
    }
}
