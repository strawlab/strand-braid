use chrono::{DateTime, FixedOffset};
use eyre::{self as anyhow, Result};

use crate::{peek2::Peek2, SyncedPictures};
use frame_source::FrameData;

/// Iterate across multiple movies using the frame timestamps to synchronize.
///
/// There is no braidz source of truth in this case.
pub(crate) struct SyncedIter {
    frame_readers: Vec<Peek2<Box<dyn Iterator<Item = Result<FrameData, frame_source::Error>>>>>,
    /// The shortest value to consider frames synchronized.
    sync_threshold: chrono::Duration,
    /// The expected interval between frames.
    frame_duration: chrono::Duration,
    previous_min: DateTime<FixedOffset>,
    previous_max: DateTime<FixedOffset>,
    frame0_times: Vec<DateTime<FixedOffset>>,
}

impl SyncedIter {
    pub(crate) fn new(
        frame_readers: Vec<Peek2<Box<dyn Iterator<Item = Result<FrameData, frame_source::Error>>>>>,
        sync_threshold: chrono::Duration,
        frame_duration: chrono::Duration,
        frame0_times: Vec<DateTime<FixedOffset>>,
    ) -> Result<Self> {
        if sync_threshold * 2 > frame_duration {
            anyhow::bail!(
                "Sync threshold must be at most half of frame duration. \
            However, the sync_threshold is {} and the frame_duration is {}",
                sync_threshold,
                frame_duration
            );
        }
        let mut previous_min = *frame0_times.iter().min().unwrap();
        let mut previous_max = *frame0_times.iter().max().unwrap();
        if (previous_max - previous_min) > sync_threshold {
            anyhow::bail!("range of timestamps in initial frame exceeds sync_threshold");
        }

        // Prepare for first frame.
        previous_min -= frame_duration;
        previous_max -= frame_duration;

        Ok(Self {
            frame_readers,
            sync_threshold,
            frame_duration,
            previous_min,
            previous_max,
            frame0_times,
        })
    }
}

impl Iterator for SyncedIter {
    type Item = Result<crate::SyncedPictures>;
    fn next(&mut self) -> std::option::Option<Self::Item> {
        let min_threshold = self.previous_min + self.frame_duration - self.sync_threshold;
        let max_threshold = self.previous_max + self.frame_duration + self.sync_threshold;

        let mut have_more_data = false;

        let camera_pictures: Vec<frame_source::Result<crate::OutTimepointPerCamera>> = self
            .frame_readers
            .iter_mut()
            .zip( self.frame0_times.iter())
            .filter_map(|(frame_reader, f0_time)| {
                let timestamp1 = frame_reader.peek1().map(|x| *f0_time+chrono::TimeDelta::from_std(x.as_ref().unwrap().timestamp().unwrap_duration()).unwrap());

                let mp4_frame = if let Some(timestamp1) = timestamp1 {
                    have_more_data = true;
                    if min_threshold <= timestamp1 && timestamp1 <= max_threshold {
                        frame_reader.next()
                    } else if timestamp1 > max_threshold {
                        tracing::warn!(
                            "The next frame is too far in the future."
                        );
                        None
                    } else  {
                        assert!(timestamp1 < min_threshold);
                        tracing::warn!(
                            "Two frames within minimum threshold. Skipping frame with timestamp {}.",
                            timestamp1,
                        );
                        frame_reader.next();
                        frame_reader.next()
                    }
                } else {
                    // end of stream:
                    None
                };

                if let Some(timestamp1) = timestamp1 {
                    let mp4_frame = match mp4_frame {
                        Some(Ok(f)) => Some(f.take_decoded().unwrap()),
                        Some(Err(e)) => {
                            return Some(Err(e));
                        }
                        None => None,
                    };

                    Some(Ok(crate::OutTimepointPerCamera::new(
                        timestamp1,
                        mp4_frame,
                        vec![],
                    )))
                } else {
                    None
                }
            })
            .collect();

        if have_more_data {
            let camera_pictures: frame_source::Result<Vec<crate::OutTimepointPerCamera>> =
                camera_pictures.into_iter().collect();

            let camera_pictures = match camera_pictures {
                Ok(cp) => cp,
                Err(e) => {
                    return Some(Err(e.into()));
                }
            };

            let stamps: Vec<DateTime<FixedOffset>> =
                camera_pictures.iter().map(|x| x.timestamp).collect();

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

            let timestamp = stamps[0];

            Some(Ok(SyncedPictures {
                timestamp,
                camera_pictures,
                braidz_info: None,
                recon: None,
            }))
        } else {
            assert_eq!(camera_pictures.len(), 0);
            None
        }
    }
}
