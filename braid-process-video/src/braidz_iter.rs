use std::{collections::BTreeMap, iter::Peekable};

use chrono::{DateTime, Utc};
use color_eyre::{
    eyre::{self as anyhow},
    Result,
};

use flydra_types::{CamNum, Data2dDistortedRow};
use frame_source::FrameData;
use timestamped_frame::ExtraTimeData;

use crate::{argmin::Argmin, peek2::Peek2, SyncedPictures};

fn clocks_within(a: &DateTime<Utc>, b: &DateTime<Utc>, dur: chrono::Duration) -> bool {
    let dist = a.signed_duration_since(*b);
    -dur < dist && dist < dur
}

/// Iterate across timepoints where no movie sources were given, relying only on
/// data in the archive.
pub(crate) struct BraidArchiveNoVideoData {
    my_iter_peekable: Peekable<Box<dyn Iterator<Item = Result<Data2dDistortedRow, csv::Error>>>>,
    frame_num: i64,
    accum: Vec<Data2dDistortedRow>,
    camns: Vec<CamNum>,
}

impl BraidArchiveNoVideoData {
    pub(crate) fn new(
        archive: &'static mut braidz_parser::BraidzArchive<std::io::BufReader<std::fs::File>>,
        camns: Vec<CamNum>,
    ) -> Result<Self> {
        let my_iter = Box::new(archive.iter_data2d_distorted()?);
        let my_iter: Box<dyn Iterator<Item = Result<Data2dDistortedRow, csv::Error>>> = my_iter;
        let mut my_iter_peekable = my_iter.peekable();
        let row0 = my_iter_peekable.peek().unwrap();
        let frame_num = row0.as_ref().unwrap().frame;
        Ok(Self {
            camns,
            my_iter_peekable,
            frame_num,
            accum: vec![],
        })
    }
}

fn rows2result(camns: &[CamNum], rows: &[Data2dDistortedRow]) -> SyncedPictures {
    assert!(!rows.is_empty());
    // let frame_num = rows[0].frame;
    let cam_received_timestamp = rows[0].cam_received_timestamp.clone();
    let timestamp: DateTime<Utc> = cam_received_timestamp.into();
    let mut camera_pictures = Vec::new();

    for camn in camns {
        let mut this_cam_this_frame = vec![];
        for row in rows {
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

    let braidz_info = Some(crate::BraidzFrameInfo {
        // frame_num,
        trigger_timestamp: None,
    });

    SyncedPictures {
        timestamp,
        braidz_info,
        camera_pictures,
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
                        return Some(Ok(rows2result(&self.camns, &rows)));
                    }
                }
                Some(Err(_)) => {
                    todo!()
                }
                Some(Ok(next_row_ref)) => {
                    if next_row_ref.frame < self.frame_num {
                        // Unexpected data from the past.
                        // TODO: could use `AscendingGroupIter` to handle this case.
                        log::error!("skipping data from the past (received data from frame {} while processing {}", next_row_ref.frame, self.frame_num);
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
                        let result = rows2result(&self.camns, &rows);
                        self.frame_num = next_row_ref.frame;
                        return Some(Ok(result));
                    }
                }
            }
        }
    }
}

struct BraidArchivePerCam<'a> {
    frame_reader: Peek2<Box<dyn Iterator<Item = Result<FrameData>>>>,
    cam_num: CamNum,
    cam_rows_peek_iter: std::iter::Peekable<std::slice::Iter<'a, Data2dDistortedRow>>,
}

fn as_ros_camid(raw_name: &str) -> String {
    let ros_name: String = raw_name.replace('-', "_");
    let ros_name: String = ros_name.replace(' ', "_");
    let ros_name: String = ros_name.replace('/', "_");
    ros_name
}

/// Iterate across multiple movies with a simultaneously recorded .braidz file
/// used to synchronize the frames.
pub(crate) struct BraidArchiveSyncVideoData<'a> {
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
        frame_readers: Vec<Peek2<Box<dyn Iterator<Item = Result<FrameData>>>>>,
        sync_threshold: chrono::Duration,
    ) -> Result<Self> {
        assert_eq!(camera_names.len(), frame_readers.len());

        // The readers will all have the current read position at
        // `approx_start_time` when this is called.

        // Get time of first frame for each reader.
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

        // Get earliest starting video
        let i = t0.iter().argmin().unwrap();
        let earliest_start_rdr = &frame_readers[i];
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

        let earliest_start = earliest_start_rdr
            .peek1()
            .unwrap()
            .as_ref()
            .unwrap()
            .decoded()
            .unwrap()
            .extra()
            .host_timestamp();
        let earliest_start_cam_num = &camid2camn.get(&as_camid(*earliest_start_cam_name)).unwrap();

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
            if clocks_within(
                &(&row.cam_received_timestamp).into(),
                &earliest_start,
                sync_threshold,
            ) {
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
            .zip(frame_readers.into_iter())
            .map(|(cam_name, frame_reader)| {
                let cam_num = *camid2camn.get(&as_camid(*cam_name)).unwrap();

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
                    frame_reader,
                    cam_num,
                    cam_rows_peek_iter,
                }
            })
            .collect();

        Ok(Self {
            per_cam,
            cur_braidz_frame: found_frame,
            sync_threshold,
            did_have_all: false,
        })
    }
}

impl<'a> Iterator for BraidArchiveSyncVideoData<'a> {
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

            // Iterate across all input mkv cameras.
            let camera_pictures: Vec<Result<crate::OutTimepointPerCamera>> = self
                .per_cam
                .iter_mut()
                .map(|this_cam| {
                    // Get the rows exclusively for this camera.
                    let cam_rows_peek_iter = &mut this_cam.cam_rows_peek_iter;

                    let mut this_cam_this_frame: Vec<Data2dDistortedRow> = vec![];
                    while let Some(peek_row) = cam_rows_peek_iter.peek() {
                        let peek_row: Data2dDistortedRow = (*peek_row).clone(); // drop the original to free memory reference.
                        if peek_row.frame < this_frame_num {
                            // We are behind where we want to be. Skip this
                            // mkv frame.
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
                    let row0_pts_chrono = (&row0.cam_received_timestamp).into();
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

                    // Now get the next MKV frame and ensure its timestamp is correct.
                    if let Some(peek1_frame) = this_cam.frame_reader.peek1() {
                        let p1_pts_chrono = peek1_frame
                            .as_ref()
                            .unwrap()
                            .decoded()
                            .unwrap()
                            .extra()
                            .host_timestamp();

                        if clocks_within(&need_chrono, &p1_pts_chrono, sync_threshold) {
                            found = true;
                        } else if p1_pts_chrono > need_chrono {
                            // peek1 MKV frame is after the time needed,
                            // so the frame is not in MKV. (Are we
                            // before first frame in MKV? Or is a frame
                            // skipped?)
                        } else {
                            panic!("Frame number in MKV is missing from BRAIDZ.");
                        }
                    } else {
                        n_cams_done += 1;
                    }

                    let mkv_frame = if found {
                        n_cams_this_frame += 1;
                        // Take this MKV frame image data.
                        this_cam.frame_reader.next()
                    } else {
                        None
                    };

                    let mkv_frame = match mkv_frame {
                        Some(Ok(f)) => f.take_decoded(),
                        Some(Err(e)) => {
                            return Err(e);
                        }
                        None => None,
                    };

                    Ok(crate::OutTimepointPerCamera::new(
                        row0_pts_chrono,
                        mkv_frame,
                        this_cam_this_frame,
                    ))
                })
                .collect();

            // All mkv files done. End.
            if n_cams_done == self.per_cam.len() {
                return None;
            }

            let braidz_info = Some(crate::BraidzFrameInfo {
                // frame_num: this_frame_num,
                trigger_timestamp,
            });

            let camera_pictures: Result<Vec<crate::OutTimepointPerCamera>> =
                camera_pictures.into_iter().collect();

            let camera_pictures = match camera_pictures {
                Ok(cp) => cp,
                Err(e) => {
                    return Some(Err(e));
                }
            };

            let timestamp = camera_pictures[0].timestamp;

            if self.did_have_all {
                return Some(Ok(SyncedPictures {
                    timestamp,
                    camera_pictures,
                    braidz_info,
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
                    }));
                }
            }
        }
    }
}
