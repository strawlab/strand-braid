use chrono::{DateTime, Utc};
use color_eyre::{
    eyre::{self as anyhow},
    Result,
};

use crate::{peek2::Peek2, SyncedPictures};
use frame_source::FrameData;
use timestamped_frame::ExtraTimeData;

/// Iterate across multiple movies using the frame timestamps to synchronize.
///
/// There is no braidz source of truth in this case.
pub(crate) struct SyncedIter {
    frame_readers: Vec<Peek2<Box<dyn Iterator<Item = Result<FrameData>>>>>,
    /// The shortest value to consider frames synchronized.
    sync_threshold: chrono::Duration,
    /// The expected interval between frames.
    frame_duration: chrono::Duration,
    previous_min: DateTime<Utc>,
    previous_max: DateTime<Utc>,
}

impl SyncedIter {
    pub(crate) fn new(
        frame_readers: Vec<Peek2<Box<dyn Iterator<Item = Result<FrameData>>>>>,
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
            .map(|x| {
                x.peek1()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .decoded()
                    .unwrap()
                    .extra()
                    .host_timestamp()
            })
            .collect();
        let mut previous_min = *t0.iter().min().unwrap();
        let mut previous_max = *t0.iter().max().unwrap();
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
        })
    }
}

impl Iterator for SyncedIter {
    type Item = Result<crate::SyncedPictures>;
    fn next(&mut self) -> std::option::Option<Self::Item> {
        let min_threshold = self.previous_min + self.frame_duration - self.sync_threshold;
        let max_threshold = self.previous_max + self.frame_duration + self.sync_threshold;

        let mut have_more_data = false;

        let mut stamps = Vec::with_capacity(self.frame_readers.len());

        let camera_pictures: Vec<Result<crate::OutTimepointPerCamera>> = self
            .frame_readers
            .iter_mut()
            .filter_map(|frame_reader| {
                let timestamp1 = frame_reader.peek1().map(|x| x.as_ref().unwrap().decoded().unwrap().extra().host_timestamp());

                let mp4_frame = if let Some(timestamp1) = timestamp1 {
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
                            // tracing::warn!(
                            //     "Two frames within minimum threshold file {}. Skipping frame with timestamp {}.",
                            //     frame_reader.as_ref().filename().display(), timestamp1,
                            // );
                            tracing::warn!(
                                "Two frames within minimum threshold. Skipping frame with timestamp {}.",
                                timestamp1,
                            );
                            frame_reader.next();
                            frame_reader.next()
                        } else {
                            // Hmmm
                            todo!();
                        }
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
            let camera_pictures: Result<Vec<crate::OutTimepointPerCamera>> =
                camera_pictures.into_iter().collect();

            let camera_pictures = match camera_pictures {
                Ok(cp) => cp,
                Err(e) => {
                    return Some(Err(e));
                }
            };

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
