use std::{collections::BTreeMap, iter::Peekable};

use braidz_types::CalibrationInfo;
use chrono::{DateTime, FixedOffset, Utc};
use eyre::{self as anyhow, Result};

use flydra_mvg::FlydraMultiCameraSystem;
use braid_types::{CamNum, Data2dDistortedRow, KalmanEstimatesRow, SyncFno};
use frame_source::FrameData;

use crate::{argmin::Argmin, peek2::Peek2, SyncedPictures};

fn clocks_within<TZ1, TZ2>(a: DateTime<TZ1>, b: DateTime<TZ2>, dur: chrono::Duration) -> bool
where
    TZ1: chrono::TimeZone,
    TZ2: chrono::TimeZone,
{
    let dist = a.signed_duration_since(b);
    -dur < dist && dist < dur
}

struct IndexedKEsts {
    inner: Option<Peekable<std::vec::IntoIter<KalmanEstimatesRow>>>,
}

impl IndexedKEsts {
    fn new(inner: Option<Vec<KalmanEstimatesRow>>) -> Self {
        let inner = inner.map(|x| x.into_iter().peekable());
        Self { inner }
    }

    fn get_kest_rows(&mut self, request_fnum: i64) -> Vec<KalmanEstimatesRow> {
        let request_fnum = SyncFno::from(u64::try_from(request_fnum).unwrap());
        if let Some(all_row_iter) = self.inner.as_mut() {
            let mut frame_rows = Vec::new();

            // Loop over iterator until we get to our requested fnum and
            // accumulate those. Break when peeked fnum is higher than request.
            loop {
                let peek_fnum = all_row_iter.peek().map(|row| row.frame);
                match peek_fnum {
                    None => break,
                    Some(peek_fnum) => {
                        if peek_fnum > request_fnum {
                            // Next fnum in table is greater than request - break.
                            break;
                        }
                        let row = all_row_iter.next().unwrap();
                        if peek_fnum == request_fnum {
                            frame_rows.push(row);
                        } else {
                            debug_assert!(peek_fnum < request_fnum);
                        }
                    }
                }
            }
            frame_rows
        } else {
            // No kalman estimates
            vec![]
        }
    }
}

/// Iterate across timepoints where no movie sources were given, relying only on
/// data in the archive.
pub(crate) struct BraidArchiveNoVideoData {
    kests: IndexedKEsts,
    my_iter_peekable: Peekable<Box<dyn Iterator<Item = Result<Data2dDistortedRow, csv::Error>>>>,
    frame_num: i64,
    accum: Vec<Data2dDistortedRow>,
    camns: Vec<CamNum>,
    recon: Option<flydra_mvg::FlydraMultiCameraSystem<f64>>,
}

impl BraidArchiveNoVideoData {
    pub(crate) fn new(
        archive: &'static mut braidz_parser::BraidzArchive<std::io::BufReader<std::fs::File>>,
        camns: Vec<CamNum>,
    ) -> Result<Self> {
        let kalman_estimates_table = archive.kalman_estimates_table.clone();
        let recon = archive.calibration_info.as_ref().map(|x| {
            let CalibrationInfo { water, cameras } = x;
            flydra_mvg::FlydraMultiCameraSystem::from_system(cameras.clone(), *water)
        });
        let my_iter = Box::new(archive.iter_data2d_distorted()?);
        let my_iter: Box<dyn Iterator<Item = Result<Data2dDistortedRow, csv::Error>>> = my_iter;
        let mut my_iter_peekable = my_iter.peekable();
        let row0 = my_iter_peekable.peek().unwrap();
        let frame_num = row0.as_ref().unwrap().frame;
        Ok(Self {
            kests: IndexedKEsts::new(kalman_estimates_table),
            camns,
            my_iter_peekable,
            frame_num,
            accum: vec![],
            recon,
        })
    }
}

fn rows2result_no_video(
    kalman_estimates_table: &mut IndexedKEsts,
    camns: &[CamNum],
    rows: &[Data2dDistortedRow],
    recon: &Option<FlydraMultiCameraSystem<f64>>,
) -> SyncedPictures {
    assert!(!rows.is_empty());
    let frame_num = rows[0].frame;
    let cam_received_timestamp = rows[0].cam_received_timestamp.clone();
    let timestamp: DateTime<Utc> = cam_received_timestamp.into();
    let timestamp: DateTime<FixedOffset> = timestamp.into();
    let mut camera_pictures = Vec::new();

    for camn in camns {
        let mut this_cam_this_frame = vec![];
        for row in rows {
            debug_assert_eq!(frame_num, row.frame);
            if &row.camn == camn {
                this_cam_this_frame.push(row.clone());
            }
        }
        camera_pictures.push(crate::OutTimepointPerCamera {
            timestamp,
            image: None,
            this_cam_this_frame,
        });
    }

    let kalman_estimates = kalman_estimates_table.get_kest_rows(frame_num);

    let braidz_info = Some(crate::BraidzFrameInfo {
        frame_num,
        trigger_timestamp: None,
        kalman_estimates,
    });

    SyncedPictures {
        timestamp,
        braidz_info,
        camera_pictures,
        recon: recon.clone(),
    }
}

impl Iterator for BraidArchiveNoVideoData {
    type Item = Result<crate::SyncedPictures>;
    fn next(&mut self) -> std::option::Option<Self::Item> {
        loop {
            let opt_next_row_ref = self.my_iter_peekable.peek();
            match opt_next_row_ref {
                None => {
                    // no more frames
                    if self.accum.is_empty() {
                        return None;
                    } else {
                        let rows = std::mem::take(&mut self.accum);
                        return Some(Ok(rows2result_no_video(
                            &mut self.kests,
                            &self.camns,
                            &rows,
                            &self.recon,
                        )));
                    }
                }
                Some(Err(_)) => {
                    todo!()
                }
                Some(Ok(next_row_ref)) => {
                    if next_row_ref.frame < self.frame_num {
                        // Unexpected data from the past.
                        // TODO: could use `AscendingGroupIter` to handle this case.
                        tracing::error!("skipping data from the past (received data from frame {} while processing {}", next_row_ref.frame, self.frame_num);
                        let _skipped_row = self.my_iter_peekable.next().unwrap().unwrap();
                        continue;
                    }
                    if next_row_ref.frame == self.frame_num {
                        // still building current result
                        let next_row = self.my_iter_peekable.next().unwrap().unwrap();
                        self.accum.push(next_row);
                    } else {
                        // next frame not part of current result, return this result.
                        let rows = std::mem::take(&mut self.accum);
                        let result =
                            rows2result_no_video(&mut self.kests, &self.camns, &rows, &self.recon);
                        self.frame_num = next_row_ref.frame;
                        return Some(Ok(result));
                    }
                }
            }
        }
    }
}

struct BraidArchivePerCam<'a> {
    cam_name: String,
    frame_reader: Peek2<Box<dyn Iterator<Item = frame_source::Result<FrameData>>>>,
    cam_num: CamNum,
    cam_rows_peek_iter: std::iter::Peekable<std::slice::Iter<'a, Data2dDistortedRow>>,
    f0_time: DateTime<FixedOffset>,
}

pub(crate) fn as_ros_camid(raw_name: &str) -> String {
    let ros_name: String = raw_name.replace('-', "_");
    let ros_name: String = ros_name.replace(' ', "_");
    let ros_name: String = ros_name.replace('/', "_");
    ros_name
}

/// Iterate across multiple movies with a simultaneously recorded .braidz file
/// used to synchronize the frames.
pub(crate) struct BraidArchiveSyncVideoData<'a> {
    recon: Option<flydra_mvg::FlydraMultiCameraSystem<f64>>,
    kests: IndexedKEsts,
    per_cam: Vec<BraidArchivePerCam<'a>>,
    sync_threshold: chrono::Duration,
    cur_braidz_frame: i64,
    did_have_all: bool,
}

impl<'a> BraidArchiveSyncVideoData<'a> {
    pub(crate) fn new(
        archive: braidz_parser::BraidzArchive<std::io::BufReader<std::fs::File>>,
        data2d: &'a BTreeMap<CamNum, Vec<Data2dDistortedRow>>,
        camera_names: &[&str],
        frame_readers: Vec<Peek2<Box<dyn Iterator<Item = Result<FrameData, frame_source::Error>>>>>,
        sync_threshold: chrono::Duration,
        frame0_times: Vec<DateTime<FixedOffset>>,
    ) -> Result<Self> {
        assert_eq!(camera_names.len(), frame_readers.len());

        // The readers will all have the current read position at
        // `approx_start_time` when this is called.

        // Get earliest starting video
        let i = frame0_times.iter().argmin().unwrap();
        let earliest_start_cam_name = &camera_names[i];

        let camid2camn = &archive.cam_info.camid2camn;

        // Backwards compatibility with old ROS names.
        let as_camid = if camid2camn.contains_key(*earliest_start_cam_name) {
            // If the camid2camn table has the new, "raw" name, use it.
            str::to_string
        } else {
            // Otherwise, use the old ROS name.
            if !camid2camn.contains_key(&as_ros_camid(earliest_start_cam_name)) {
                anyhow::bail!(
                    "Braidz archive does not contain raw camera name, but it also does not contain a ROS name."
                );
            }
            as_ros_camid
        };

        let earliest_start = frame0_times[i];
        let earliest_start_cam_num = &camid2camn.get(&as_camid(earliest_start_cam_name)).unwrap();

        // Now get data2d row with this timestamp to find the synchronized frame number.
        let cam_rows = data2d.get(earliest_start_cam_num).ok_or_else(|| {
            anyhow::anyhow!(
                "No data2d in braidz file '{}' for camera '{}'.",
                archive.display(),
                earliest_start_cam_name,
            )
        })?;
        let mut found_frame = None;

        for row in cam_rows.iter() {
            let row_dt: DateTime<Utc> = (&row.cam_received_timestamp).into();
            if clocks_within(row_dt, earliest_start, sync_threshold) {
                if let Some(frame) = &found_frame {
                    assert_eq!(row.frame, *frame);
                } else {
                    found_frame = Some(row.frame);
                }
                break;
            }
        }
        let found_frame = found_frame.unwrap();

        let per_cam = camera_names
            .iter()
            .zip(frame_readers)
            .zip(frame0_times.iter())
            .map(|((cam_name, frame_reader), f0_time)| {
                let cam_num = *camid2camn.get(&as_camid(cam_name)).unwrap();

                let cam_rows = data2d.get(&cam_num).unwrap();
                for row in cam_rows.iter() {
                    if row.frame == found_frame {
                        break;
                    }
                }

                // Get the rows exclusively for this camera.
                let cam_rows = data2d.get(&cam_num).unwrap();
                let cam_rows_peek_iter = cam_rows.iter().peekable();

                BraidArchivePerCam {
                    cam_name: cam_name.to_string(),
                    frame_reader,
                    cam_num,
                    cam_rows_peek_iter,
                    f0_time: *f0_time,
                }
            })
            .collect();

        let recon = archive.calibration_info.as_ref().map(|x| {
            let CalibrationInfo { water, cameras } = x;
            flydra_mvg::FlydraMultiCameraSystem::from_system(cameras.clone(), *water)
        });
        let kests = IndexedKEsts::new(archive.kalman_estimates_table);

        Ok(Self {
            recon,
            kests,
            per_cam,
            cur_braidz_frame: found_frame,
            sync_threshold,
            did_have_all: false,
        })
    }
}

impl Iterator for BraidArchiveSyncVideoData<'_> {
    type Item = Result<crate::SyncedPictures>;
    fn next(&mut self) -> std::option::Option<Self::Item> {
        let sync_threshold = self.sync_threshold;

        loop {
            // braidz frame loop.
            let this_frame_num = self.cur_braidz_frame;
            self.cur_braidz_frame += 1;

            let mut n_cams_this_frame = 0;
            let mut n_cams_done = 0;

            let mut trigger_timestamp = None;

            // Iterate across all input mp4 cameras.
            let camera_pictures: Vec<frame_source::Result<crate::OutTimepointPerCamera>> = self
                .per_cam
                .iter_mut()
                .map(|this_cam| {
                    let cam_name = this_cam.cam_name.as_str();
                    // Get the rows exclusively for this camera.
                    let cam_rows_peek_iter = &mut this_cam.cam_rows_peek_iter;

                    let mut this_cam_this_frame: Vec<Data2dDistortedRow> = vec![];
                    while let Some(peek_row) = cam_rows_peek_iter.peek() {
                        let peek_row: Data2dDistortedRow = (*peek_row).clone(); // drop the original to free memory reference.
                        if peek_row.frame < this_frame_num {
                            // We are behind where we want to be. Skip this
                            // mp4 frame.
                            cam_rows_peek_iter.next().unwrap();
                            continue;
                        }
                        if peek_row.frame == this_frame_num {
                            // We have a frame.
                            let row = cam_rows_peek_iter.next().unwrap();
                            assert_eq!(row.camn, this_cam.cam_num);
                            this_cam_this_frame.push(row.clone());
                        }
                        if peek_row.frame > this_frame_num {
                            // This would be going too far.
                            break;
                        }
                    }

                    if this_cam_this_frame.is_empty() {
                        panic!(
                            "missing 2d data in braid archive for frame {}",
                            this_frame_num
                        );
                    }

                    let row0 = &this_cam_this_frame[0];
                    let row0_pts_chrono: DateTime<Utc> = (&row0.cam_received_timestamp).into();
                    // Get the timestamp we need.
                    let need_stamp = &row0.cam_received_timestamp;
                    if let Some(tt) = &row0.timestamp {
                        if let Some(_tt2) = &trigger_timestamp {
                            // // Hmm, why are the trigger timestamps for
                            // // multple cameras at the same frame number
                            // // identical?
                            // assert_eq!(tt,tt2);
                        } else {
                            trigger_timestamp = Some(tt.clone());
                        }
                    }
                    let need_chrono = need_stamp.into();

                    let mut found = false;

                    // Now get the next MP4 frame and ensure its timestamp is correct.
                    if let Some(peek1_frame) = this_cam.frame_reader.peek1() {
                        let frame_data = peek1_frame
                            .as_ref()
                            .unwrap();
                        let idx = frame_data.idx();
                        let p1_pts = frame_data
                            .timestamp().unwrap_duration();
                        let p1_pts_chrono = this_cam.f0_time + chrono::TimeDelta::from_std(p1_pts)?;

                        if clocks_within(need_chrono, p1_pts_chrono, sync_threshold) {
                            found = true;
                        } else if p1_pts_chrono > need_chrono {
                            // peek1 MP4 frame is after the time needed,
                            // so the frame is not in MP4. (Are we
                            // before first frame in MP4? Or is a frame
                            // skipped?)
                        } else {
                            panic!("Cam {cam_name}: frame {idx} ({p1_pts_chrono}) in video is missing from BRAIDZ.");
                        }
                    } else {
                        n_cams_done += 1;
                    }

                    let mp4_frame = if found {
                        n_cams_this_frame += 1;
                        // Take this MP4 frame image data.
                        this_cam.frame_reader.next()
                    } else {
                        None
                    };

                    let mp4_frame = match mp4_frame {
                        Some(Ok(f)) => f.take_decoded(),
                        Some(Err(e)) => {
                            return Err(e);
                        }
                        None => None,
                    };

                    Ok(crate::OutTimepointPerCamera::new(
                        row0_pts_chrono.into(),
                        mp4_frame,
                        this_cam_this_frame,
                    ))
                })
                .collect();

            // All mp4 files done. End.
            if n_cams_done == self.per_cam.len() {
                return None;
            }

            let kalman_estimates = self.kests.get_kest_rows(this_frame_num);

            let braidz_info = Some(crate::BraidzFrameInfo {
                frame_num: this_frame_num,
                trigger_timestamp,
                kalman_estimates,
            });

            let camera_pictures: frame_source::Result<Vec<crate::OutTimepointPerCamera>> =
                camera_pictures.into_iter().collect();

            let camera_pictures = match camera_pictures {
                Ok(cp) => cp,
                Err(e) => {
                    return Some(Err(e.into()));
                }
            };

            let timestamp = camera_pictures[0].timestamp;

            if self.did_have_all {
                return Some(Ok(SyncedPictures {
                    timestamp,
                    camera_pictures,
                    braidz_info,
                    recon: self.recon.clone(),
                }));
            } else {
                // If we haven't yet had a frame with all cameras, check if this
                // is the first such.
                self.did_have_all = n_cams_this_frame == self.per_cam.len();
                if self.did_have_all {
                    return Some(Ok(SyncedPictures {
                        timestamp,
                        camera_pictures,
                        braidz_info,
                        recon: self.recon.clone(),
                    }));
                }
            }
        }
    }
}
