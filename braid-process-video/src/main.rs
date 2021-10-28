use std::{collections::BTreeMap, io::Write};

use anyhow::{Context as ContextTrait, Result};
use chrono::{DateTime, Utc};
use ordered_float::NotNan;
use structopt::StructOpt;

use ffmpeg_next as ffmpeg;

use machine_vision_formats::{pixel_format::RGB8, ImageData, ImageStride};

use ci2_remote_control::MkvRecordingConfig;
use flydra_types::{FlydraFloatTimestampLocal, RawCamName, Triggerbox};

mod peek2;

mod argmin;
use argmin::Argmin;

mod frame_reader;
use frame_reader::FrameReader;

mod frame;
pub use frame::Frame;

mod braidz_iter;
mod synced_iter;

mod config;
use config::{BraidRetrackVideoConfig, OutputVideoConfig, Validate};

mod tiny_skia_frame;

#[derive(Debug, StructOpt)]
#[structopt(about = "process videos within the Braid multi-camera framework")]
struct BraidProcessVideoCliArgs {
    /// Input configuration TOML file
    #[structopt(long, parse(from_os_str))]
    config: std::path::PathBuf,
}

#[derive(Debug, Clone)]
struct Rgba(pub [u8; 4]);

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
    fn new(rdr: &peek2::Peek2<FrameReader>) -> Self {
        let peek1 = rdr.peek1().unwrap().as_ref().unwrap();
        let width = peek1.width() as usize;
        let height = peek1.height() as usize;
        Self {
            width,
            height,
            cam_name: None, // TODO
            cam_num: None,  // TODO
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

    let frame_readers: Vec<_> = readers.into_iter().map(crate::peek2::Peek2::new).collect();

    let per_cams: Vec<PerCamRender> = frame_readers.iter().map(|x| PerCamRender::new(x)).collect();

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

    let mut my_mkv_writer = mkv_writer::MkvWriter::new(out_fd, mkv_cfg, None)?;
    let mut composite_timestamp;
    let mut first_timestamp = None;

    let debug_output: Option<&config::OutputConfig> =
        cfg.output.iter().find(|x| x.type_ == "debug_txt");

    let mut debug_fd = debug_output
        .map(|x| std::fs::File::create(&x.filename))
        .transpose()?;

    for (out_fno, synced_frames) in frame_iter.enumerate() {
        let synced_frames: Vec<Option<Result<Frame>>> = synced_frames;

        if let Some(start_frame) = cfg.start_frame {
            if out_fno < start_frame {
                continue;
            }
        }

        if let Some(ref mut fd) = &mut debug_fd {
            writeln!(fd, "frame {} ----------", out_fno)?;
        }
        if out_fno % 100 == 0 {
            log::info!("frame {}", out_fno);
        }

        // Number of individual input frames from this timepoint to be
        // compisited into final output.
        let n_frames = synced_frames.len();

        let mut per_cam_data = Vec::with_capacity(n_frames);

        let mut usvg_opt = usvg::Options::default();
        // Get file's absolute directory.
        // usvg_opt.resources_dir = std::fs::canonicalize(&args[1]).ok().and_then(|p| p.parent().map(|p| p.to_path_buf()));
        usvg_opt.fontdb.load_system_fonts();

        // Convert from total pixels to half width/height.
        let feature_size_pixels = (video_options.feature_size_pixels.unwrap_or(10) / 2) as i32;

        composite_timestamp = None;
        for (filename, ((cam_num, frame), per_cam_ref)) in filenames
            .iter()
            .zip(cam_nums.iter().zip(synced_frames).zip(&per_cams))
        {
            let mut per_cam = per_cam_ref.clone();

            if let Some(frame) = frame {
                let frame = frame?;
                composite_timestamp = Some(frame.pts_chrono);
                // Get timestamp from MKV file.
                let frame_triggerbox: FlydraFloatTimestampLocal<Triggerbox> =
                    frame.pts_chrono.into();
                // Get timestamp from MKV file also as f64 number.
                let frame_f64 = frame_triggerbox.as_f64();

                per_cam.set_original_image(&frame)?;

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
                            // TODO: Potentially there are multiple rows in
                            // braidz file for this framenumber. Handle them all.
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

                                if let Ok(x) = NotNan::new(best_row.x) {
                                    if let Ok(y) = NotNan::new(best_row.y) {
                                        println!("frame {}: append point", out_fno);
                                        per_cam.append_2d_point(x, y)?;
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

            wtr.elem("svg", |d| {
                let width = cum_width + n_frames * 2 * video_options.composite_margin_pixels;
                let height = cum_height + 2 * video_options.composite_margin_pixels;

                d.attr("xmlns", "http://www.w3.org/2000/svg")
                    .attr("xmlns:xlink", "http://www.w3.org/1999/xlink")
                    .attr("viewBox", format_args!("0 0 {} {}", width, height));
            })
            .build(|w| {
                w.elem("g", |d| {
                    d.attr("id", "frames");
                })
                .build(|w| {
                    // TODO: put in image coordinate system
                    let mut curx = 0;
                    for per_cam in per_cam_data.into_iter() {
                        curx += video_options.composite_margin_pixels;
                        if let Some(ref bytes) = per_cam.png_buf {
                            let png_base64_buf = base64::encode(&bytes);
                            let data_url = format!("data:image/png;base64,{}", png_base64_buf);
                            w.single("image", |d| {
                                d.attr("x", curx)
                                    .attr("y", video_options.composite_margin_pixels)
                                    .attr("width", per_cam.width)
                                    .attr("height", per_cam.height)
                                    .attr("xlink:href", data_url);
                            });
                        } else {
                            w.single("rect", |d| {
                                d.attr("x", curx)
                                    .attr("y", video_options.composite_margin_pixels)
                                    .attr("width", per_cam.width)
                                    .attr("height", per_cam.height)
                                    .attr("style", "fill:blue");
                            });
                        }

                        for xy in per_cam.points.iter() {
                            w.single("circle", |d| {
                                d.attr("cx", curx as f64 + xy.0.as_ref())
                                    .attr(
                                        "cy",
                                        video_options.composite_margin_pixels as f64
                                            + xy.1.as_ref(),
                                    )
                                    .attr("r", format!("{}", feature_size_pixels))
                                    .attr("style", "fill:none;stroke:green;stroke-width:3");
                            });
                        }

                        curx += per_cam.width + video_options.composite_margin_pixels;
                    }
                });
            });
            // Get the SVG file contents.
            let fmt_wtr = wtr.into_writer();
            let svg_buf = {
                let _ = fmt_wtr.error?;
                fmt_wtr.inner
            };

            let mut debug_svg_fd = std::fs::File::create(format!("frame{:05}.svg", out_fno))?;
            debug_svg_fd.write_all(&svg_buf)?;

            // Now parse the SVG file.
            let rtree = usvg::Tree::from_data(&svg_buf, &usvg_opt.to_ref())?;
            // Now render the SVG file to a pixmap.
            let pixmap_size = rtree.svg_node().size.to_screen_size();
            let mut pixmap =
                tiny_skia::Pixmap::new(pixmap_size.width(), pixmap_size.height()).unwrap();
            resvg::render(&rtree, usvg::FitTo::Original, pixmap.as_mut()).unwrap();

            my_mkv_writer.write(&tiny_skia_frame::Frame::new(pixmap), save_ts)?;
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
