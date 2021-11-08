use std::collections::BTreeMap;

use anyhow::Result;
use chrono::{DateTime, Utc};

use flydra_types::{CamNum, Data2dDistortedRow};

use crate::{argmin::Argmin, peek2, MovieReader};

fn clocks_within(a: &DateTime<Utc>, b: &DateTime<Utc>, dur: chrono::Duration) -> bool {
    let dist = a.signed_duration_since(*b);
    -dur < dist && dist < dur
}

struct BraidArchivePerCam<'a> {
    frame_reader: crate::peek2::Peek2<Box<dyn MovieReader>>,
    cam_num: CamNum,
    cam_rows_peek_iter: std::iter::Peekable<std::slice::Iter<'a, Data2dDistortedRow>>,
}

/// Iterate across multiple movies with a simultaneously recorded .braidz file
/// used to synchronize the frames.
pub struct BraidArchiveSyncData<'a> {
    per_cam: Vec<BraidArchivePerCam<'a>>,
    sync_threshold: chrono::Duration,
    cur_braidz_frame: i64,
    did_have_all: bool,
}

impl<'a> BraidArchiveSyncData<'a> {
    pub fn new(
        archive: &'a braidz_parser::BraidzArchive<std::io::BufReader<std::fs::File>>,
        data2d: &'a BTreeMap<CamNum, Vec<Data2dDistortedRow>>,
        camera_names: &[Option<String>],
        frame_readers: Vec<peek2::Peek2<Box<dyn MovieReader>>>,
        sync_threshold: chrono::Duration,
    ) -> Result<Self> {
        assert_eq!(camera_names.len(), frame_readers.len());

        let camera_names: Vec<Result<String>> = camera_names
            .iter()
            .zip(frame_readers.iter())
            .map(|(camera_name, rdr)| {
                Ok(camera_name
                    .as_ref()
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "Camera name for '{}' could not be guessed. Specify manually.",
                            rdr.as_ref().filename()
                        )
                    })?
                    .clone())
            })
            .collect();

        let camera_names: Result<Vec<String>> = camera_names.into_iter().collect();
        let camera_names: Vec<String> = camera_names?;

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
        let earliest_start_cam_name = &camera_names[i];
        let earliest_start = earliest_start_rdr
            .peek1()
            .unwrap()
            .as_ref()
            .unwrap()
            .pts_chrono;
        let earliest_start_cam_num = archive
            .cam_info
            .camid2camn
            .get(earliest_start_cam_name)
            .unwrap();

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
                let cam_num = *archive.cam_info.camid2camn.get(cam_name).unwrap();

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

impl<'a> Iterator for BraidArchiveSyncData<'a> {
    type Item = crate::OutFrameIterType;
    fn next(&mut self) -> std::option::Option<Self::Item> {
        let sync_threshold = self.sync_threshold;

        loop {
            // braidz frame loop.
            let this_frame_num = self.cur_braidz_frame;
            self.cur_braidz_frame += 1;

            let mut n_cams_this_frame = 0;
            let mut n_cams_done = 0;

            // Iterate across all input mkv cameras.
            let result = Some(
                self.per_cam
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
                                debug_assert!(row.camn == this_cam.cam_num);
                                this_cam_this_frame.push(row.clone());
                            }
                            if peek_row.frame > this_frame_num {
                                // This would be going to far.
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
                        // Get the timestamp we need.
                        let need_stamp = &row0.cam_received_timestamp;
                        let need_chrono = need_stamp.into();

                        let mut found = false;

                        // Now get the next MKV frame and ensure its timestamp is correct.
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

                        crate::OutFramePerCamInput::new(mkv_frame, this_cam_this_frame)
                    })
                    .collect(),
            );

            // All mkv files done. End.
            if n_cams_done == self.per_cam.len() {
                return None;
            }

            if self.did_have_all {
                return result;
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
