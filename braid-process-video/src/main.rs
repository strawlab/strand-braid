use std::{collections::BTreeMap, io::Write};

use anyhow::{Context as ContextTrait, Result};
use chrono::{DateTime, Utc};
use structopt::StructOpt;

use ffmpeg_next as ffmpeg;

use ci2_remote_control::MkvRecordingConfig;
use flydra_types::{CamNum, Data2dDistortedRow, FlydraFloatTimestampLocal, RawCamName, Triggerbox};
use simple_frame::SimpleFrame;

mod peek2;

mod argmin;
use argmin::Argmin;

mod frame_reader;
use frame_reader::FrameReader;

mod frame;
pub use frame::Frame;

mod synced_iter;
use synced_iter::SyncedIter;

use machine_vision_formats::pixel_format::RGB8;

mod config;
use config::{BraidRetrackVideoConfig, OutputVideoConfig, Validate};

#[derive(Debug, StructOpt)]
#[structopt(about = "process videos within the Braid multi-camera framework")]
struct BraidProcessVideoCliArgs {
    /// Input configuration TOML file
    #[structopt(long, parse(from_os_str))]
    config: std::path::PathBuf,
}

#[derive(Debug, Clone)]
struct Rgba(pub [u8; 4]);

fn put_pixel(self_: &mut SimpleFrame<RGB8>, x: u32, y: u32, incoming: &Rgba) {
    if x >= self_.width {
        return;
    }
    if y >= self_.height {
        return;
    }
    let row_start = self_.stride as usize * y as usize;
    let pix_start = row_start + x as usize * 3;

    let alpha = incoming.0[3] as f64 / 255.0;
    let p = 1.0 - alpha;
    let q = alpha;

    use std::convert::TryInto;
    let old: [u8; 3] = self_.image_data[pix_start..pix_start + 3]
        .try_into()
        .unwrap();
    let new: [u8; 3] = [
        (old[0] as f64 * p + incoming.0[0] as f64 * q).round() as u8,
        (old[1] as f64 * p + incoming.0[1] as f64 * q).round() as u8,
        (old[2] as f64 * p + incoming.0[2] as f64 * q).round() as u8,
    ];

    self_.image_data[pix_start] = new[0];
    self_.image_data[pix_start + 1] = new[1];
    self_.image_data[pix_start + 2] = new[2];
}

fn synchronize_readers_from(
    approx_start_time: DateTime<Utc>,
    readers: Vec<peek2::Peek2<FrameReader>>,
) -> Vec<peek2::Peek2<FrameReader>> {
    // Advance each reader until upcoming frame is not before the start time.
    readers
        .into_iter()
        .map(|mut reader| {
            log::debug!("filename: {}", reader.as_ref().filename);

            // Get information for first frame
            let p1_pts_chrono = reader.peek1().unwrap().as_ref().unwrap().pts_chrono;
            let p2_pts_chrono = reader.peek2().unwrap().as_ref().unwrap().pts_chrono;
            let mut p1_delta = (p1_pts_chrono - approx_start_time)
                .num_nanoseconds()
                .unwrap()
                .abs();

            log::debug!("  p1_pts_chrono: {}", p1_pts_chrono);
            log::debug!("  p2_pts_chrono: {}", p2_pts_chrono);
            log::debug!("  p1_delta: {}", p1_delta);

            if p1_pts_chrono >= approx_start_time {
                // First frame is already after the start time, use it unconditionally.
                reader
            } else {
                loop {
                    // Get information for second frame
                    if let Some(p2_frame) = reader.peek2() {
                        let p2_pts_chrono = p2_frame.as_ref().unwrap().pts_chrono;
                        let p2_delta = (p2_pts_chrono - approx_start_time)
                            .num_nanoseconds()
                            .unwrap()
                            .abs();

                        if p2_pts_chrono >= approx_start_time {
                            // Second frame is after start time. Use closest match.
                            if p1_delta <= p2_delta {
                                // p1 frame is closet to start frame.
                            } else {
                                // p2 frame is closest to start frame. Advance so it is now p1.
                                reader.next();
                            }
                            break;
                        }

                        // Not yet at start time, advance.
                        reader.next();
                        p1_delta = p2_delta;
                    } else {
                        // No p2 frame.
                        if reader.peek1().is_some() {
                            // If there is a single frame remaining, skip it.
                            // (This is the alternative to checking all corner
                            // cases for single frame files.)
                            reader.next();
                        }
                        break;
                    }
                }
                reader
            }
        })
        .collect()
}

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
struct BraidArchiveSyncData<'a> {
    per_cam: Vec<BraidArchivePerCam>,
    // archive: &'a braidz_parser::BraidzArchive<std::io::BufReader<std::fs::File>>,
    data2d: &'a BTreeMap<CamNum, Vec<Data2dDistortedRow>>,
    braidz_frame0: i64,
    sync_threshold: chrono::Duration,
}

impl<'a> BraidArchiveSyncData<'a> {
    fn new(
        archive: &'a braidz_parser::BraidzArchive<std::io::BufReader<std::fs::File>>,
        data2d: &'a BTreeMap<CamNum, Vec<Data2dDistortedRow>>,
        camera_names: &[Option<String>],
        frame_readers: Vec<peek2::Peek2<FrameReader>>,
        sync_threshold: chrono::Duration,
    ) -> Result<Self> {
        assert_eq!(camera_names.len(), frame_readers.len());

        use crate::synced_iter::Timestamped;
        // The readers will all have the current read position at
        // `approx_start_time` when this is called.

        // Get time of first frame for each reader.
        let t0: Vec<DateTime<Utc>> = frame_readers
            .iter()
            .map(|x| x.peek1().unwrap().timestamp())
            .collect();

        // Get earliest starting video
        let i = t0.iter().argmin().unwrap();
        let earliest_start_rdr = &frame_readers[i];
        let earliest_start_cam_name = &camera_names[i].as_ref().unwrap();
        let earliest_start = earliest_start_rdr.peek1().unwrap().timestamp();
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
            // archive,
            data2d,
            braidz_frame0: found_frame,
            sync_threshold,
        })
    }

    fn into_iter(self) -> BraidArchiveIter<'a> {
        let braidz_frame = self.braidz_frame0;
        BraidArchiveIter {
            src: self,
            braidz_frame,
            did_have_all: false,
        }
    }
}

struct BraidArchiveIter<'a> {
    src: BraidArchiveSyncData<'a>,
    braidz_frame: i64,
    did_have_all: bool,
}

impl<'a> Iterator for BraidArchiveIter<'a> {
    type Item = Vec<Option<Result<Frame>>>;
    fn next(&mut self) -> std::option::Option<Self::Item> {
        let data2d = &self.src.data2d;
        let sync_threshold = self.src.sync_threshold;

        loop {
            // braidz frame loop.
            let this_frame_num = self.braidz_frame;
            self.braidz_frame += 1;

            let mut n_cams_this_frame = 0;

            // Iterate across all input mkv cameras.
            let result = Some(
                self.src
                    .per_cam
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
                self.did_have_all = n_cams_this_frame == self.src.per_cam.len();
                if self.did_have_all {
                    return result;
                }
            }
        }
    }
}

fn run_config(cfg: &BraidRetrackVideoConfig) -> Result<()> {
    ffmpeg::init().unwrap();

    // Get sources.
    let filenames: Vec<String> = cfg.input_video.iter().map(|s| s.filename.clone()).collect();

    // Read camera names from configuration.
    let mut camera_names: Vec<Option<String>> = cfg
        .input_video
        .iter()
        .map(|s| {
            s.camera_name
                .as_ref()
                .map(|s| RawCamName::new(s.clone()).to_ros().as_str().to_string())
        })
        .collect();

    // Open a frame reader for each source.
    let readers = filenames
        .iter()
        .map(|f| FrameReader::new(f))
        .collect::<Result<Vec<_>>>()?;

    for (cam_name, reader) in camera_names.iter_mut().zip(readers.iter()) {
        if cam_name.is_none() {
            if let Some(title) = &reader.title {
                // The title of the video segment defaults to the camera name,
                // so here we read the title. Braidz files save the camera name
                // as the "ROS" version, so we have to convert to that form.
                let raw = RawCamName::new(title.clone());
                let ros = raw.to_ros();
                let ros_cam_name = ros.as_str();
                log::info!(
                    "In video {}, camera name from title: {}",
                    reader.filename,
                    ros_cam_name
                );
                *cam_name = Some(ros_cam_name.to_string());
            }
        }
    }

    // Determine which video started last and what time was the last start time.
    // This time is where we will start from.
    let approx_start_time = readers
        .iter()
        .map(|rdr| rdr.creation_time)
        .max()
        .ok_or_else(|| anyhow::anyhow!("Zero file inputs. Cannot find start."))?;

    log::info!("start time: {}", approx_start_time);

    let mut braid_archive = cfg
        .input_braidz
        .as_ref()
        .map(braidz_parser::braidz_parse_path)
        .transpose()
        .with_context(|| {
            format!(
                "opening braidz archive {}",
                cfg.input_braidz.as_ref().unwrap()
            )
        })?;

    let mut data2d = BTreeMap::new();
    if let Some(ref mut braidz) = braid_archive.as_mut() {
        for row in braidz.iter_data2d_distorted()? {
            let row = row?;
            let cam_entry = &mut data2d.entry(row.camn).or_insert_with(Vec::new);
            cam_entry.push(row);
        }
    }

    let readers: Vec<_> = readers.into_iter().map(crate::peek2::Peek2::new).collect();

    let widths: Vec<usize> = readers
        .iter()
        .map(|x| x.peek1().unwrap().as_ref().unwrap().width() as usize)
        .collect();
    let cum_width: usize = widths.iter().sum();
    let cum_height = readers
        .iter()
        .map(|x| x.peek1().unwrap().as_ref().unwrap().height() as usize)
        .max()
        .unwrap();

    // Advance each reader until upcoming frame is not before the start time.
    let frame_duration_approx = readers
        .iter()
        .map(|reader| {
            let p1_pts_chrono = reader.peek1().unwrap().as_ref().unwrap().pts_chrono;
            let p2_pts_chrono = reader.peek2().unwrap().as_ref().unwrap().pts_chrono;
            p2_pts_chrono - p1_pts_chrono
        })
        .min()
        .unwrap();

    let frame_duration = cfg
        .frame_duration_microsecs
        .map(|x| chrono::Duration::from_std(std::time::Duration::from_micros(x)).unwrap())
        .unwrap_or(frame_duration_approx);

    log::info!(
        "frame_duration: {} microseconds",
        frame_duration.num_microseconds().unwrap()
    );

    let sync_threshold = cfg
        .sync_threshold_microseconds
        .map(|x| chrono::Duration::from_std(std::time::Duration::from_micros(x)).unwrap())
        .unwrap_or(frame_duration / 2);

    log::info!(
        "sync_threshold: {} microseconds",
        sync_threshold.num_microseconds().unwrap()
    );

    let synced_iter: Box<dyn Iterator<Item = _>> = if let Some(ref archive) = braid_archive {
        let sync_data =
            BraidArchiveSyncData::new(archive, &data2d, &camera_names, readers, sync_threshold)?;
        Box::new(sync_data.into_iter())
    } else {
        let readers = synchronize_readers_from(approx_start_time, readers);
        Box::new(SyncedIter::new(readers, sync_threshold, frame_duration)?)
    };

    let ros_cam_ids: Option<Vec<String>> = braid_archive
        .as_ref()
        .map(|a| -> Vec<_> { a.cam_info.camid2camn.keys().map(Clone::clone).collect() });
    if let Some(rcis) = ros_cam_ids {
        log::info!(
            "cameras in braid archive \"{}\": {:?}",
            cfg.input_braidz.as_ref().unwrap(),
            rcis
        );
    }

    let cam_nums = camera_names
        .iter()
        .map(|opt_cam_name| {
            opt_cam_name
                .as_ref()
                .map(|cam_name| {
                    braid_archive
                        .as_ref()
                        .map(|a| a.cam_info.camid2camn.get(cam_name))
                        .flatten()
                })
                .flatten()
                .copied()
        })
        .collect::<Vec<_>>();

    // For now, we can only have a single video output.
    let output = &cfg.output[0];
    let default_video_options = OutputVideoConfig::default();
    let video_options = &output
        .video_options
        .as_ref()
        .unwrap_or(&default_video_options);

    let out_fd = std::fs::File::create(&output.filename)?;

    let opts = ci2_remote_control::VP9Options { bitrate: 10000 };
    let codec = ci2_remote_control::MkvCodec::VP9(opts);

    let mkv_cfg = MkvRecordingConfig {
        codec,
        max_framerate: ci2_remote_control::RecordingFrameRate::Unlimited,
        save_creation_time: video_options.time_dilation_factor.is_none(),
        title: video_options.title.clone(),
        ..Default::default()
    };

    let green = Rgba([128, 255, 128, 255]);

    let mut my_mkv_writer = mkv_writer::MkvWriter::new(out_fd, mkv_cfg, None)?;
    let mut composite_timestamp;
    let mut first_timestamp = None;

    let debug_output: Option<&config::OutputConfig> =
        cfg.output.iter().find(|x| x.type_ == "debug_txt");

    let mut debug_fd = debug_output
        .map(|x| std::fs::File::create(&x.filename))
        .transpose()?;

    for (out_fno, synced_frames) in synced_iter.enumerate() {
        let synced_frames: Vec<Option<Result<Frame>>> = synced_frames;

        if let Some(ref mut fd) = &mut debug_fd {
            writeln!(fd, "frame {} ----------", out_fno)?;
        }
        if out_fno % 100 == 0 {
            log::info!("frame {}", out_fno);
        }

        let n_frames = synced_frames.len();

        let width = cum_width + n_frames * 2 * video_options.composite_margin_pixels;
        let height = cum_height + 2 * video_options.composite_margin_pixels;

        let stride = width as usize * 3;
        let image_data = vec![255; stride as usize * height as usize];
        let mut composited = SimpleFrame::<RGB8> {
            width: width as u32,
            height: height as u32,
            stride: stride as u32,
            image_data,
            fmt: std::marker::PhantomData,
        };

        // Convert from total pixels to half width/height.
        let feature_size_pixels = (video_options.feature_size_pixels.unwrap_or(10) / 2) as i32;

        let mut cur_x: i32 = 0;
        let cur_y = video_options.composite_margin_pixels as i32;
        composite_timestamp = None;
        for (filename, ((cam_num, frame), frame_width)) in filenames
            .iter()
            .zip(cam_nums.iter().zip(synced_frames).zip(&widths))
        {
            cur_x += video_options.composite_margin_pixels as i32;

            if let Some(frame) = frame {
                let frame = frame?;
                composite_timestamp = Some(frame.pts_chrono);
                let frame_triggerbox: FlydraFloatTimestampLocal<Triggerbox> =
                    frame.pts_chrono.into();
                let frame_f64 = frame_triggerbox.as_f64();

                // Draw frame at (cur_x, cur_y) of same frame.width() frame.height() into composited.
                let src_stride = frame.stride();
                let copy_width = frame.width() as usize * 3;
                for src_row in 0..frame.height() as usize {
                    let start = src_row * src_stride;
                    let src_row_data = &frame.bytes()[start..][..copy_width];

                    let dest_start_y = cur_y as usize + src_row;
                    let dest_start = dest_start_y * stride + cur_x as usize * 3;
                    let dest = &mut composited.image_data[dest_start..][..copy_width];
                    dest.copy_from_slice(src_row_data);
                }

                let mut wrote_debug = false;

                if let Some(cam_num) = cam_num {
                    if let Some(data2d_rows) = data2d.get(cam_num) {
                        // TODO: major optimization by indexing. This is
                        // probably SLOW - it iterates over all timestamps
                        // for each frame.
                        let time_dist = data2d_rows
                            .iter()
                            .map(|row| (row.cam_received_timestamp.as_f64() - frame_f64).abs())
                            // .map(|row| (row.timestamp.as_ref().unwrap().as_f64() - frame_f64).abs())
                            .collect::<Vec<f64>>();

                        if let Some(best_idx) = time_dist.iter().argmin() {
                            let best_row = &data2d_rows[best_idx];
                            let best_timestamp = &best_row.cam_received_timestamp;
                            // let best_timestamp = best_row.timestamp.as_ref().unwrap();
                            let offset_secs = (frame_f64 - best_timestamp.as_f64()).abs();
                            let offset_secs_chrono = chrono::Duration::from_std(
                                std::time::Duration::from_secs_f64(offset_secs),
                            )
                            .unwrap();

                            if offset_secs_chrono < sync_threshold {
                                let best_dt: chrono::DateTime<chrono::Utc> = best_timestamp.into();

                                if let Some(ref mut fd) = &mut debug_fd {
                                    writeln!(
                                        fd,
                                        "   {}: {} ({}), {} ({})",
                                        filename,
                                        frame.pts_chrono,
                                        datetime_conversion::datetime_to_f64(&frame.pts_chrono),
                                        best_dt,
                                        best_timestamp.as_f64(),
                                    )?;
                                    wrote_debug = true;
                                }

                                let x = best_row.x;
                                let y = best_row.y;

                                if !x.is_nan() {
                                    for xo in &[-feature_size_pixels, feature_size_pixels] {
                                        for yo in -feature_size_pixels..=feature_size_pixels {
                                            put_pixel(
                                                &mut composited,
                                                (cur_x + x as i32 + xo) as u32,
                                                (cur_y + y as i32 + yo) as u32,
                                                &green,
                                            );
                                        }
                                    }
                                    for xo in -feature_size_pixels..=feature_size_pixels {
                                        for yo in &[-feature_size_pixels, feature_size_pixels] {
                                            put_pixel(
                                                &mut composited,
                                                (cur_x + x as i32 + xo) as u32,
                                                (cur_y + y as i32 + yo) as u32,
                                                &green,
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                if !wrote_debug {
                    if let Some(ref mut fd) = &mut debug_fd {
                        writeln!(
                            fd,
                            "   {}: {} ({})",
                            filename,
                            frame.pts_chrono,
                            datetime_conversion::datetime_to_f64(&frame.pts_chrono),
                        )?;
                    }
                }
            }

            cur_x += *frame_width as i32;
            cur_x += video_options.composite_margin_pixels as i32;
        }

        // If there is no new data, we do not write a frame.
        if let Some(ts) = &composite_timestamp {
            let save_ts = if let Some(time_dilation_factor) = video_options.time_dilation_factor {
                if first_timestamp.is_none() {
                    first_timestamp = Some(*ts);
                }

                let actual_time_delta =
                    ts.signed_duration_since(*first_timestamp.as_ref().unwrap());
                let actual_time_delta_micros = actual_time_delta.num_microseconds().unwrap();
                let saved_time_delta =
                    (actual_time_delta_micros as f64 * time_dilation_factor as f64).round() as i64;
                let saved_time_delta = chrono::Duration::microseconds(saved_time_delta);
                *ts + saved_time_delta
            } else {
                *ts
            };
            my_mkv_writer.write(&composited, save_ts)?;
        }

        // let png_buf = convert_image::frame_to_image(&composited, convert_image::ImageOptions::Png)?;
        // std::fs::write(format!("frame{}.png", out_fno), png_buf)?;

        if let Some(max_num_frames) = &cfg.max_num_frames {
            if out_fno >= *max_num_frames {
                break;
            }
        }
    }

    my_mkv_writer.finish()?;
    Ok(())
}

fn main() -> Result<()> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "info");
    }

    env_logger::init();

    let args = BraidProcessVideoCliArgs::from_args();

    let cfg_fname = match args.config.to_str() {
        None => {
            panic!("Configuration file name not utf-8.");
        }
        Some(cfg_fname) => cfg_fname.to_string(),
    };

    let get_usage = || {
        let default_buf = toml::to_string_pretty(&BraidRetrackVideoConfig::default()).unwrap();
        format!(
            "Parsing TOML config file '{}' into BraidRetrackVideoConfig.\n\n\
            Example of a valid TOML configuration:\n\n```\n{}```",
            &cfg_fname, default_buf
        )
    };

    let cfg_str = std::fs::read_to_string(&cfg_fname)
        .with_context(|| format!("Reading config file '{}'", &cfg_fname))?;

    let mut cfg: BraidRetrackVideoConfig = toml::from_str(&cfg_str).with_context(get_usage)?;
    cfg.validate().with_context(get_usage)?;

    let cfg_as_string = toml::to_string_pretty(&cfg).unwrap();
    log::info!(
        "Generating output using the following configuration:\n\n```\n{}```\n",
        cfg_as_string
    );

    run_config(&cfg)
}
