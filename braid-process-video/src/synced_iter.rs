use anyhow::Result;
use chrono::{DateTime, Utc};

use crate::{peek2::Peek2, FrameReader};

/// Iterate across multiple movies using the frame timestamps to synchronize.
pub struct SyncedIter {
    frame_readers: Vec<Peek2<FrameReader>>,
    /// The shortest value to consider frames synchronized.
    sync_threshold: chrono::Duration,
    /// The expected interval between frames.
    frame_duration: chrono::Duration,
    previous_min: DateTime<Utc>,
    previous_max: DateTime<Utc>,
}

impl SyncedIter {
    pub fn new(
        frame_readers: Vec<Peek2<FrameReader>>,
        sync_threshold: chrono::Duration,
        frame_duration: chrono::Duration,
    ) -> Result<Self> {
        if sync_threshold * 2 > frame_duration {
            anyhow::bail!(
                "Sync threshold must be at most half of frame duration. \
            However, the syncthreshold is {} and the frame_duration is {}",
                sync_threshold,
                frame_duration
            );
        }
        let t0: Vec<DateTime<Utc>> = frame_readers
            .iter()
            .map(|x| x.peek1().unwrap().as_ref().unwrap().pts_chrono)
            .collect();
        let mut previous_min = *t0.iter().min().unwrap();
        let mut previous_max = *t0.iter().max().unwrap();
        if (previous_max - previous_min) > sync_threshold {
            anyhow::bail!("range of timestamps in initial frame exceeds sync_threshold");
        }

        // Prepare for first frame.
        previous_min = previous_min - frame_duration;
        previous_max = previous_max - frame_duration;

        Ok(Self {
            frame_readers,
            sync_threshold,
            frame_duration,
            previous_min,
            previous_max,
        })
    }
}

impl Iterator for SyncedIter {
    type Item = crate::OutFrameIterType;
    fn next(&mut self) -> std::option::Option<Self::Item> {
        let min_threshold = self.previous_min + self.frame_duration - self.sync_threshold;
        let max_threshold = self.previous_max + self.frame_duration + self.sync_threshold;

        let mut have_more_data = false;

        let mut stamps = Vec::with_capacity(self.frame_readers.len());

        let res = self
            .frame_readers
            .iter_mut()
            .map(|frame_reader| {
                let timestamp1 = frame_reader.peek1().map(|x| x.as_ref().unwrap().pts_chrono);

                let mkv_frame = if let Some(timestamp1) = timestamp1 {
                    have_more_data = true;
                    if min_threshold <= timestamp1 && timestamp1 <= max_threshold {
                        stamps.push(timestamp1);
                        frame_reader.next()
                    } else {
                        // The next frame is not within the range expected.
                        if timestamp1 > max_threshold {
                            // A frame was skipped and the next frame is too far in
                            // the future.
                            None
                        } else if timestamp1 < min_threshold {
                            // Just skip a frame in the file? Not sure about this.
                            log::warn!("Two frames in file? Skipping one.");
                            frame_reader.next();
                            frame_reader.next()
                        } else {
                            // Hmmm
                            todo!();
                        }
                    }
                } else {
                    // end of stream
                    None
                };
                crate::OutFramePerCamInput::new(mkv_frame, vec![])
            })
            .collect();

        self.previous_min = stamps
            .iter()
            .min()
            .copied()
            .unwrap_or_else(|| self.previous_min + self.frame_duration);
        self.previous_max = stamps
            .iter()
            .max()
            .copied()
            .unwrap_or_else(|| self.previous_max + self.frame_duration);

        if have_more_data {
            Some(res)
        } else {
            assert_eq!(res.iter().filter(|x| x.mkv_frame.is_some()).count(), 0);
            None
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::convert::TryInto;

    impl Timestamped for i32 {
        fn timestamp(&self) -> DateTime<Utc> {
            use chrono::NaiveDateTime;
            let base: DateTime<Utc> =
                DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(60, 0), Utc);

            let offset = chrono::Duration::from_std(std::time::Duration::from_secs(
                (*self).try_into().unwrap(),
            ))
            .unwrap();

            base + offset
        }
    }

    fn make_toy_data() -> Vec<Peek2<std::vec::IntoIter<i32>>> {
        let res = vec![
            // The second is missing 500.
            // All are missing 800.
            vec![1, 100, 200, 300, 400, 500, 600, 700, 900],
            vec![0, 99, 201, 300, 400, 600, 700, 901],
            vec![0, 100, 200, 301, 399, 500, 600, 700, 899, 1000],
        ];

        res.into_iter().map(|v| Peek2::new(v.into_iter())).collect()
    }

    #[test]
    fn test_sync_iter() -> Result<()> {
        let sync_threshold =
            chrono::Duration::from_std(std::time::Duration::from_secs(50)).unwrap();

        let frame_duration =
            chrono::Duration::from_std(std::time::Duration::from_secs(100)).unwrap();

        let mut iter = SyncedIter::new(make_toy_data(), sync_threshold, frame_duration)?;
        assert_eq!(iter.next(), Some(vec![Some(1), Some(0), Some(0)]));
        assert_eq!(iter.next(), Some(vec![Some(100), Some(99), Some(100)]));
        assert_eq!(iter.next(), Some(vec![Some(200), Some(201), Some(200)]));
        assert_eq!(iter.next(), Some(vec![Some(300), Some(300), Some(301)]));
        assert_eq!(iter.next(), Some(vec![Some(400), Some(400), Some(399)]));
        assert_eq!(iter.next(), Some(vec![Some(500), None, Some(500)]));

        assert_eq!(iter.next(), Some(vec![Some(600), Some(600), Some(600)]));
        assert_eq!(iter.next(), Some(vec![Some(700), Some(700), Some(700)]));
        assert_eq!(iter.next(), Some(vec![None, None, None]));
        assert_eq!(iter.next(), Some(vec![Some(900), Some(901), Some(899)]));
        assert_eq!(iter.next(), Some(vec![None, None, Some(1000)]));
        Ok(())
    }
}
