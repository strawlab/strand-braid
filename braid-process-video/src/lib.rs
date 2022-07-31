use std::{collections::BTreeMap, io::Write};

use anyhow::{Context as ContextTrait, Result};
use chrono::{DateTime, Utc};
use ordered_float::NotNan;

#[cfg(feature = "read-mkv")]
use ffmpeg_next as ffmpeg;

use machine_vision_formats::{pixel_format::RGB8, ImageData, ImageStride};

use ci2_remote_control::MkvRecordingConfig;
use flydra_types::{Data2dDistortedRow, RawCamName};

mod peek2;

mod argmin;

#[cfg(feature = "read-mkv")]
mod ffmpeg_frame_reader;
#[cfg(feature = "read-mkv")]
use ffmpeg_frame_reader::FfmpegFrameReader;

mod fmf_frame_reader;
use fmf_frame_reader::FmfFrameReader;

mod frame;
pub use frame::Frame;

mod braidz_iter;
mod synced_iter;

mod config;
pub use config::{
    BraidRetrackVideoConfig, OutputConfig, OutputVideoConfig, Valid, VideoSourceConfig,
};

mod auto_config_generator;
pub use auto_config_generator::auto_config;

mod tiny_skia_frame;

pub struct OutFramePerCamInput {
    /// Camera image from MKV file, if available.
    mkv_frame: Option<Result<Frame>>,
    /// Braidz data. Empty if no braidz data available.
    this_cam_this_frame: Vec<Data2dDistortedRow>,
}

impl OutFramePerCamInput {
    pub(crate) fn new(
        mkv_frame: Option<Result<Frame>>,
        this_cam_this_frame: Vec<Data2dDistortedRow>,
    ) -> Self {
        Self {
            mkv_frame,
            this_cam_this_frame,
        }
    }
}

/// An ordered `Vec` with one entry per camera.
pub type OutFrameIterType = Vec<OutFramePerCamInput>;

fn synchronize_readers_from(
    approx_start_time: DateTime<Utc>,
    readers: Vec<peek2::Peek2<Box<dyn MovieReader>>>,
) -> Vec<peek2::Peek2<Box<dyn MovieReader>>> {
    // Advance each reader until upcoming frame is not before the start time.
    readers
        .into_iter()
        .map(|mut reader| {
            log::debug!("filename: {}", reader.as_ref().filename());

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

#[derive(Clone)]
struct PerCamRender {
    width: usize,
    height: usize,
    cam_name: Option<String>,
    cam_num: Option<flydra_types::CamNum>,
    png_buf: Option<Vec<u8>>,
    points: Vec<(NotNan<f64>, NotNan<f64>)>,
}

impl PerCamRender {
    fn new(rdr: &peek2::Peek2<Box<dyn MovieReader>>) -> Self {
        let peek1 = rdr.peek1().unwrap().as_ref().unwrap();
        let width = peek1.width() as usize;
        let height = peek1.height() as usize;
        Self {
            width,
            height,
            cam_name: None,
            cam_num: None,
            png_buf: None,
            points: vec![],
        }
    }

    fn set_original_image(&mut self, image: &dyn ImageStride<RGB8>) -> Result<()> {
        self.png_buf = Some(convert_image::frame_to_image(
            image,
            convert_image::ImageOptions::Png,
        )?);
        Ok(())
    }

    fn append_2d_point(&mut self, x: NotNan<f64>, y: NotNan<f64>) -> Result<()> {
        self.points.push((x, y));
        Ok(())
    }
}

pub fn run_config(cfg: &Valid<BraidRetrackVideoConfig>) -> Result<()> {
    let cfg = cfg.valid();

    #[cfg(feature = "read-mkv")]
    ffmpeg::init().unwrap();

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

    // let braidz_summary = cfg.input_braidz.as_ref().map(|braidz_fname| {
    let braidz_summary = braid_archive.as_ref().map(|archive| {
        let path = archive.path();
        let attr = std::fs::metadata(&path).unwrap();
        let filename = crate::config::path_to_string(&path).unwrap();
        braidz_parser::summarize_braidz(archive, filename, attr.len())
    });

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
        .map(|f| open_movie(f))
        .collect::<Result<Vec<_>>>()?;

    let movie_re = regex::Regex::new(r"^movie\d{8}_\d{6}_(.*)$").unwrap();

    for (cam_name, reader) in camera_names.iter_mut().zip(readers.iter()) {
        if cam_name.is_none() {
            // Camera name was not specified manually in the config.

            // First, try to read from metadata embedded in the movie.
            if let Some(title) = &reader.title() {
                // The title of the video segment defaults to the camera name,
                // so here we read the title. Braidz files save the camera name
                // as the "ROS" version, so we have to convert to that form.
                let raw = RawCamName::new(title.to_string());
                let ros = raw.to_ros();
                let ros_cam_name = ros.as_str();
                log::debug!(
                    "In video {}, camera name from video title: {}",
                    reader.filename(),
                    ros_cam_name
                );
                *cam_name = Some(ros_cam_name.to_string());
            } else if let Some(braidz_summary) = braidz_summary.as_ref() {
                // If we could not read from video metadata, see if we can read
                // from braidz file. However, the braidz file records camera
                // names as the ROS variant in which some characters are
                // replaced with underscore. Therefore, we attempt to parse
                // known video filenames saved by braid to extract the original
                // camera name from them.

                let full_path = std::path::PathBuf::from(reader.filename());
                let fname = full_path
                    .file_name()
                    .unwrap()
                    .to_os_string()
                    .to_str()
                    .unwrap()
                    .to_string();
                // example: fname = "movie20211108_084523_Basler-22445994.fmf.gz"

                let stem = fname.as_str().split(".").next().unwrap();
                // example: stem = "movie20211108_084523_Basler-22445994"

                if let Some(caps) = movie_re.captures(stem) {
                    // get the raw camera name
                    let raw = caps.get(1).unwrap().as_str();
                    // example: raw = "Basler-22445994"

                    let ros = RawCamName::new(raw.to_string())
                        .to_ros()
                        .as_str()
                        .to_string();
                    // example: ros = "Basler_22445994"

                    for test_ros_cam_name in braidz_summary.cam_info.camid2camn.keys() {
                        if &ros == test_ros_cam_name {
                            *cam_name = Some(ros);
                            break;
                        }
                    }
                }
            } else {
                // Theoretically, could attempt to parse filenames as above even
                // without a braidz archive. But for no we do not.
            }
        }

        if cam_name.is_none() {
            // No camera name given and no braidz file.
            anyhow::bail!(
                "For video file {}, could not determine camera name",
                reader.filename()
            );
        }
    }

    // Determine which video started last and what time was the last start time.
    // This time is where we will start from.
    let approx_start_time = readers
        .iter()
        .map(|rdr| rdr.creation_time())
        .max()
        .ok_or_else(|| anyhow::anyhow!("Zero file inputs. Cannot find start."))?
        .clone();

    log::info!("start time: {}", approx_start_time);

    let mut data2d = BTreeMap::new();
    if let Some(ref mut braidz) = braid_archive.as_mut() {
        for row in braidz.iter_data2d_distorted()? {
            let row = row?;
            let cam_entry = &mut data2d.entry(row.camn).or_insert_with(Vec::new);
            cam_entry.push(row);
        }
    }

    let frame_readers: Vec<_> = readers.into_iter().map(crate::peek2::Peek2::new).collect();

    let mut per_cams: Vec<PerCamRender> = frame_readers.iter().map(PerCamRender::new).collect();

    let cum_width: usize = per_cams.iter().map(|x| x.width).sum();
    let cum_height: usize = per_cams.iter().map(|x| x.height).max().unwrap();

    // Advance each reader until upcoming frame is not before the start time.
    let frame_duration_approx = frame_readers
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

    // Build iterator to iterate over output frames. This is equivalent to
    // iterating over synchronized input frames.
    let frame_iter: Box<dyn Iterator<Item = _>> = if let Some(ref archive) = braid_archive {
        // In this path, we use the .braidz file as the source of
        // synchronization.
        Box::new(braidz_iter::BraidArchiveSyncData::new(
            archive,
            &data2d,
            &camera_names,
            frame_readers,
            sync_threshold,
        )?)
    } else {
        // In this path, we use the timestamps in the saved videos as the source
        // of synchronization.
        let frame_readers = synchronize_readers_from(approx_start_time, frame_readers);
        Box::new(synced_iter::SyncedIter::new(
            frame_readers,
            sync_threshold,
            frame_duration,
        )?)
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

    camera_names
        .iter()
        .zip(per_cams.iter_mut())
        .for_each(|(opt_cam_name, per_cam_mut)| {
            let cam_num = opt_cam_name
                .as_ref()
                .map(|cam_name| {
                    braid_archive
                        .as_ref()
                        .map(|a| a.cam_info.camid2camn.get(cam_name))
                        .flatten()
                })
                .flatten()
                .copied();
            per_cam_mut.cam_num = cam_num;
            per_cam_mut.cam_name = opt_cam_name.clone();
        });

    // For now, we can only have a single video output.
    let output = &cfg.output[0];
    let default_video_options = OutputVideoConfig::default();
    let video_options = &output
        .video_options
        .as_ref()
        .unwrap_or(&default_video_options);

    // Create output dir if needed.
    let output_filename = std::path::PathBuf::from(&output.filename);
    if let Some(dest_dir) = output_filename.parent() {
        std::fs::create_dir_all(dest_dir)?;
    }
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

    let mut my_mkv_writer = mkv_writer::MkvWriter::new(out_fd, mkv_cfg, None)?;
    let mut composite_timestamp;
    let mut first_timestamp = None;

    let mut usvg_opt = usvg::Options::default();
    // Get file's absolute directory.
    // usvg_opt.resources_dir = std::fs::canonicalize(&args[1]).ok().and_then(|p| p.parent().map(|p| p.to_path_buf()));
    usvg_opt.fontdb.load_system_fonts();

    let composite_margin_pixels = video_options.composite_margin_pixels.unwrap_or(5);

    let feature_radius = video_options
        .feature_radius
        .as_ref()
        .map(Clone::clone)
        .unwrap_or_else(|| "10".to_string());
    let feature_style = video_options
        .feature_style
        .as_ref()
        .map(Clone::clone)
        .unwrap_or_else(|| "fill:none;stroke:deepskyblue;stroke-width:3".to_string());

    let debug_output: Option<&config::OutputConfig> =
        cfg.output.iter().find(|x| x.type_ == "debug_txt");

    let mut debug_fd = debug_output
        .map(|x| std::fs::File::create(&x.filename))
        .transpose()?;

    for (out_fno, synced_frames) in frame_iter.enumerate() {
        let synced_frames: OutFrameIterType = synced_frames;

        if let Some(start_frame) = cfg.start_frame {
            if out_fno < start_frame {
                continue;
            }
        }

        if let Some(ref mut fd) = &mut debug_fd {
            writeln!(fd, "frame {} ----------", out_fno)?;
        }

        if out_fno % cfg.log_interval_frames.unwrap_or(100) == 0 {
            log::info!("frame {}", out_fno);
        }

        // Number of individual input frames from this timepoint to be
        // compisited into final output.
        let n_frames = synced_frames.len();

        let mut per_cam_data = Vec::with_capacity(n_frames);

        composite_timestamp = None;
        for ((filename, out_frame_per_cam_input), per_cam_ref) in
            filenames.iter().zip(synced_frames).zip(&per_cams)
        {
            // Copy the default information for this camera and then we will
            // start adding information relevant for this frame in time.
            let mut per_cam = per_cam_ref.clone();

            // Did we get an image from the MKV file?
            if let Some(frame) = out_frame_per_cam_input.mkv_frame {
                let frame = frame?;
                // Update the timestamp for this frame to whatever timestamp
                // came from the last MKV frame.
                composite_timestamp = Some(frame.pts_chrono);

                per_cam.set_original_image(&frame)?;

                let mut wrote_debug = false;

                for row_data2d in out_frame_per_cam_input.this_cam_this_frame.iter() {
                    if let Some(ref mut fd) = &mut debug_fd {
                        let row_dt: DateTime<Utc> = (&row_data2d.cam_received_timestamp).into();
                        writeln!(
                            fd,
                            "   {}: {} ({}), {} ({})",
                            filename,
                            frame.pts_chrono,
                            datetime_conversion::datetime_to_f64(&frame.pts_chrono),
                            row_dt,
                            row_data2d.cam_received_timestamp.as_f64(),
                        )?;
                        wrote_debug = true;
                    }

                    if let Ok(x) = NotNan::new(row_data2d.x) {
                        let y = NotNan::new(row_data2d.y).unwrap();
                        per_cam.append_2d_point(x, y)?;
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

            per_cam_data.push(per_cam);
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

            // Draw SVG
            let mut wtr = tagger::new(tagger::upgrade_write(Vec::<u8>::new()));
            let svg_width = cum_width + n_frames * 2 * composite_margin_pixels;
            let svg_height = cum_height + 2 * composite_margin_pixels;
            wtr.elem("svg", |d| {
                d.attr("xmlns", "http://www.w3.org/2000/svg")?;
                d.attr("xmlns:xlink", "http://www.w3.org/1999/xlink")?;
                d.attr("viewBox", format_args!("0 0 {} {}", svg_width, svg_height))
            })?
            .build(|w| {
                // write a background white rectangle.
                w.single("rect", |d| {
                    d.attr("x", 0)?;
                    d.attr("y", 0)?;
                    d.attr("width", svg_width)?;
                    d.attr("height", svg_height)?;
                    d.attr("style", "fill:white")
                })?;

                w.elem("g", |_| Ok(()))?.build(|w| {
                    let mut curx = 0;
                    for (cam_idx, per_cam) in per_cam_data.into_iter().enumerate() {
                        curx += composite_margin_pixels;

                        // Clip to the camera image size.
                        w.elem("clipPath", |d| {
                            d.attr("id", format!("clip-path-{}", cam_idx))
                        })?
                        .build(|w| {
                            w.single("rect", |d| {
                                d.attr("x", 0)?;
                                d.attr("y", 0)?;
                                d.attr("width", per_cam.width)?;
                                d.attr("height", per_cam.height)
                            })?;
                            Ok(())
                        })?;

                        w.elem("g", |d| {
                            d.attr(
                                "transform",
                                format!("translate({},{})", curx, composite_margin_pixels),
                            )?;
                            d.attr("clip-path", format!("url(#clip-path-{})", cam_idx))
                        })?
                        .build(|w| {
                            if let Some(ref bytes) = per_cam.png_buf {
                                let png_base64_buf = base64::encode(&bytes);
                                let data_url = format!("data:image/png;base64,{}", png_base64_buf);
                                w.single("image", |d| {
                                    d.attr("x", 0)?;
                                    d.attr("y", 0)?;
                                    d.attr("width", per_cam.width)?;
                                    d.attr("height", per_cam.height)?;
                                    d.attr("xlink:href", data_url)
                                })?;
                            }

                            for xy in per_cam.points.iter() {
                                w.single("circle", |d| {
                                    d.attr("cx", xy.0.as_ref())?;
                                    d.attr("cy", xy.1.as_ref())?;
                                    d.attr("r", &feature_radius)?;
                                    d.attr("style", &feature_style)
                                })?;
                            }
                            Ok(())
                        })?;

                        curx += per_cam.width + composite_margin_pixels;
                    }
                    Ok(())
                })?;
                Ok(())
            })?;
            // Get the SVG file contents.
            let fmt_wtr = wtr.into_writer();
            let svg_buf = {
                let _ = fmt_wtr.error?;
                fmt_wtr.inner
            };

            // // Write composited SVG to disk.
            // let mut debug_svg_fd = std::fs::File::create(format!("frame{:05}.svg", out_fno))?;
            // debug_svg_fd.write_all(&svg_buf)?;

            // Now parse the SVG file.
            let rtree = usvg::Tree::from_data(&svg_buf, &usvg_opt.to_ref())?;
            // Now render the SVG file to a pixmap.
            let pixmap_size = rtree.svg_node().size.to_screen_size();
            let mut pixmap =
                tiny_skia::Pixmap::new(pixmap_size.width(), pixmap_size.height()).unwrap();
            resvg::render(&rtree, usvg::FitTo::Original, pixmap.as_mut()).unwrap();

            // Save the pixmap into the MVG file being saved.
            my_mkv_writer.write(&tiny_skia_frame::Frame::new(pixmap)?, save_ts)?;
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

pub trait MovieReader {
    fn title(&self) -> Option<&str>;
    fn filename(&self) -> &str;
    fn creation_time(&self) -> &DateTime<Utc>;
    fn next_frame(&mut self) -> Option<Result<Frame>>;
}

fn open_movie(filename: &str) -> Result<Box<dyn MovieReader>> {
    if filename.to_lowercase().ends_with(".fmf") {
        Ok(Box::new(FmfFrameReader::new(filename)?))
    } else if filename.to_lowercase().ends_with(".fmf.gz") {
        Ok(Box::new(FmfFrameReader::new(filename)?))
    } else {
        #[cfg(feature = "read-mkv")]
        {
            Ok(Box::new(FfmpegFrameReader::new(filename)?))
        }

        #[cfg(not(feature = "read-mkv"))]
        {
            anyhow::bail!("File not .fmf or .fmf.gz but not compiled 'read-mkv' feature.")
        }
    }
}

impl Iterator for dyn MovieReader {
    type Item = Result<Frame>;
    fn next(&mut self) -> std::option::Option<<Self as Iterator>::Item> {
        self.next_frame()
    }
}