use std::collections::BTreeMap;

use anyhow::Result;
use chrono::{DateTime, Utc};

use flydra_types::{CamNum, Data2dDistortedRow};

use crate::peek2;

use crate::argmin::Argmin;

use crate::frame_reader::FrameReader;

use crate::frame::Frame;

fn clocks_within(a: &DateTime<Utc>, b: &DateTime<Utc>, dur: chrono::Duration) -> bool {
    let dist = a.signed_duration_since(*b);
    -dur < dist && dist < dur
}

struct BraidArchivePerCam {
    frame_reader: crate::peek2::Peek2<FrameReader>,
    data2d_start_row_idx: usize,
    cam_num: CamNum,
    cur_offset: usize,
}

// Iterate across multiple movies with a simultaneously recorded .braidz file
// used to synchronize the frames.
pub struct BraidArchiveSyncData<'a> {
    per_cam: Vec<BraidArchivePerCam>,
    data2d: &'a BTreeMap<CamNum, Vec<Data2dDistortedRow>>,
    sync_threshold: chrono::Duration,
    cur_braidz_frame: i64,
    did_have_all: bool,
}

impl<'a> BraidArchiveSyncData<'a> {
    pub fn new(
        archive: &'a braidz_parser::BraidzArchive<std::io::BufReader<std::fs::File>>,
        data2d: &'a BTreeMap<CamNum, Vec<Data2dDistortedRow>>,
        camera_names: &[Option<String>],
        frame_readers: Vec<peek2::Peek2<FrameReader>>,
        sync_threshold: chrono::Duration,
    ) -> Result<Self> {
        assert_eq!(camera_names.len(), frame_readers.len());

        // The readers will all have the current read position at
        // `approx_start_time` when this is called.

        // Get time of first frame for each reader.
        let t0: Vec<DateTime<Utc>> = frame_readers
            .iter()
            .map(|x| x.peek1().unwrap().as_ref().unwrap().pts_chrono)
            .collect();

        // Get earliest starting video
        let i = t0.iter().argmin().unwrap();
        let earliest_start_rdr = &frame_readers[i];
        let earliest_start_cam_name = &camera_names[i].as_ref().unwrap();
        let earliest_start = earliest_start_rdr
            .peek1()
            .unwrap()
            .as_ref()
            .unwrap()
            .pts_chrono;
        let earliest_start_cam_num = archive
            .cam_info
            .camid2camn
            .get(*earliest_start_cam_name)
            .unwrap();

        // Now get data2d row with this timestamp to find the synchronized frame number.
        let cam_rows = data2d.get(earliest_start_cam_num).unwrap();
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

        // let cam_nums: Vec<CamNum> = camera_names
        let per_cam = camera_names
            .iter()
            .zip(frame_readers.into_iter())
            .map(|(cam_name, frame_reader)| {
                let cam_num = *archive
                    .cam_info
                    .camid2camn
                    .get(cam_name.as_ref().unwrap())
                    .unwrap();

                let cam_rows = data2d.get(&cam_num).unwrap();
                let mut found_row = None;
                for (i, row) in cam_rows.iter().enumerate() {
                    if row.frame == found_frame {
                        found_row = Some(i);
                        break;
                    }
                }
                let data2d_start_row_idx = found_row.unwrap();

                BraidArchivePerCam {
                    data2d_start_row_idx,
                    frame_reader,
                    cam_num,
                    cur_offset: 0,
                }
            })
            .collect();

        Ok(Self {
            per_cam,
            data2d,
            cur_braidz_frame: found_frame,
            sync_threshold,
            did_have_all: false,
        })
    }
}

impl<'a> Iterator for BraidArchiveSyncData<'a> {
    type Item = Vec<Option<Result<Frame>>>;
    fn next(&mut self) -> std::option::Option<Self::Item> {
        let data2d = &self.data2d;
        let sync_threshold = self.sync_threshold;

        loop {
            // braidz frame loop.
            let this_frame_num = self.cur_braidz_frame;
            self.cur_braidz_frame += 1;

            let mut n_cams_this_frame = 0;

            // Iterate across all input mkv cameras.
            let result = Some(
                self.per_cam
                    .iter_mut()
                    .map(|this_cam| -> Option<Result<Frame>> {
                        let cam_rows = data2d.get(&this_cam.cam_num).unwrap();

                        let mut row = None;
                        while row.is_none() {
                            // data2d loop in case there are multiple points per frame in braidz file.
                            let xrow =
                                &cam_rows[this_cam.data2d_start_row_idx + this_cam.cur_offset];

                            this_cam.cur_offset += 1;
                            assert!(
                                !(xrow.frame > this_frame_num),
                                "missing 2d data in braid archive for frame {}",
                                this_frame_num
                            );
                            if xrow.frame == this_frame_num {
                                row = Some(xrow);
                            } else {
                                debug_assert!(xrow.frame < this_frame_num);
                            }
                        }
                        let row = row.unwrap();
                        debug_assert!(row.frame == this_frame_num);
                        debug_assert!(row.camn == this_cam.cam_num);

                        // Get the timestamp we need.
                        let need_stamp = &row.cam_received_timestamp;
                        let need_chrono = need_stamp.into();

                        let mut found = false;

                        // Now get the next frame and ensure its timestamp is correct.
                        if let Some(peek1_frame) = this_cam.frame_reader.peek1() {
                            let p1_pts_chrono = peek1_frame.as_ref().unwrap().pts_chrono;

                            if clocks_within(&need_chrono, &p1_pts_chrono, sync_threshold) {
                                found = true;
                            } else if p1_pts_chrono > need_chrono {
                                // peek1 MKV frame is after the time needed,
                                // so the frame is not in MKV. (Are we
                                // before first frame in MKV? Or is a frame
                                // skipped?)
                            } else {
                                todo!("frame missing from BRAIDZ?!");
                            }
                        }

                        if found {
                            n_cams_this_frame += 1;
                            // Take this MKV frame image data.
                            this_cam.frame_reader.next()
                        } else {
                            None
                        }
                    })
                    .collect(),
            );

            if self.did_have_all {
                // If we have already had a crame with all cameras, return
                // whatever cameras we do have data for, if any.
                if n_cams_this_frame > 0 {
                    return result;
                } else {
                    // TODO: handle case where all cameras failed to save a
                    // frame to MKV but future camera data will come.
                    return None;
                }
            } else {
                // If we haven't yet had a frame with all cameras, check if this
                // is the first such.
                self.did_have_all = n_cams_this_frame == self.per_cam.len();
                if self.did_have_all {
                    return result;
                }
            }
        }
    }
}
