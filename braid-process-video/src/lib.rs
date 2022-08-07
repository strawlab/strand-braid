use std::{collections::BTreeMap, io::Write};

use anyhow::{Context as ContextTrait, Result};
use chrono::{DateTime, Utc};
use futures::future::join_all;
use ordered_float::NotNan;

#[cfg(feature = "read-mkv")]
use ffmpeg_next as ffmpeg;

use machine_vision_formats::ImageData;

use flydra_types::{Data2dDistortedRow, RawCamName, RosCamName};

mod peek2;
use peek2::Peek2;

mod argmin;

#[cfg(feature = "read-mkv")]
mod ffmpeg_frame_reader;
#[cfg(feature = "read-mkv")]
use ffmpeg_frame_reader::FfmpegFrameReader;

mod fmf_frame_reader;
use fmf_frame_reader::FmfFrameReader;

mod frame;
pub(crate) use frame::Frame;

mod braidz_iter;
mod synced_iter;

mod config;
pub(crate) use config::FeatureDetectionMethod;
pub use config::{
    BraidRetrackVideoConfig, OutputConfig, Valid, Validate, VideoOutputConfig, VideoSourceConfig,
};

mod auto_config_generator;
pub use auto_config_generator::auto_config;

mod tiny_skia_frame;

mod output_types;
use output_types::*;

mod output_braidz;

mod output_video;

pub(crate) const DEFAULT_COMPOSITE_MARGIN_PIXELS: usize = 5;
pub(crate) const DEFAULT_FEATURE_RADIUS: &str = "10";
pub(crate) const DEFAULT_FEATURE_STYLE: &str = "fill: none; stroke: deepskyblue; stroke-width: 3;";
pub(crate) const DEFAULT_CAMERA_TEXT_STYLE: &str =
    "font-family: Arial; font-size: 40px; fill: deepskyblue;";

#[derive(Debug)]
pub(crate) struct OutTimepointPerCamera {
    timestamp: DateTime<Utc>,
    /// Camera image from MKV or FMF file, if available.
    image: Option<Frame>,
    /// Braidz data. Empty if no braidz data available.
    this_cam_this_frame: Vec<Data2dDistortedRow>,
}

impl OutTimepointPerCamera {
    pub(crate) fn new(
        timestamp: DateTime<Utc>,
        image: Option<Frame>,
        this_cam_this_frame: Vec<Data2dDistortedRow>,
    ) -> Self {
        Self {
            timestamp,
            image,
            this_cam_this_frame,
        }
    }
}

/// An ordered `Vec` with one entry per camera.
#[derive(Debug)]
pub(crate) struct SyncedPictures {
    pub(crate) timestamp: DateTime<Utc>,
    pub(crate) camera_pictures: Vec<OutTimepointPerCamera>,
    /// If a braidz file was used as synchronization source, more data is
    /// available.
    pub(crate) braidz_info: Option<BraidzFrameInfo>,
}

#[derive(Debug)]
pub(crate) struct BraidzFrameInfo {
    // frame_num: i64,
    trigger_timestamp: Option<flydra_types::FlydraFloatTimestampLocal<flydra_types::Triggerbox>>,
}

fn synchronize_readers_from(
    approx_start_time: DateTime<Utc>,
    readers: &mut Vec<Peek2<Box<dyn MovieReader>>>,
) {
    // Advance each reader until upcoming frame is not before the start time.
    for reader in readers.iter_mut() {
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
            continue;
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
        }
    }
}

struct PerCamRender {
    best_name: String,
    ros_name: Option<RosCamName>,
    frame0_png_buf: flydra_types::PngImageData,
    width: usize,
    height: usize,
}

impl PerCamRender {
    fn from_reader(cam_id: &CameraIdentifier) -> Self {
        let best_name = cam_id.best_name();
        let ros_name = cam_id.ros_name().map(RosCamName::new);

        let rdr = match &cam_id {
            CameraIdentifier::MovieOnly(m) | CameraIdentifier::Both((m, _)) => {
                m.reader.as_ref().unwrap()
            }
            _ => {
                panic!("")
            }
        };
        let peek1 = rdr.peek1().unwrap().as_ref().unwrap();

        // Copy first frame for later use in braidz output if needed.
        let frame0_png_buf =
            convert_image::frame_to_image(&peek1, convert_image::ImageOptions::Png)
                .unwrap()
                .into();

        let width = peek1.width() as usize;
        let height = peek1.height() as usize;

        Self {
            best_name,
            ros_name,
            frame0_png_buf,
            width,
            height,
        }
    }

    fn from_braidz(
        braid_archive: &braidz_parser::BraidzArchive<std::io::BufReader<std::fs::File>>,
        braidz_cam: &BraidzCamId,
    ) -> Self {
        let image_sizes = braid_archive.image_sizes.as_ref().unwrap();
        let (width, height) = image_sizes.get(&braidz_cam.ros_cam_name).unwrap();
        let best_name = braidz_cam.ros_cam_name.clone(); // this is the best we can do
        let ros_name = Some(RosCamName::new(braidz_cam.ros_cam_name.clone()));

        // generate blank first image of the correct size.
        let image_data: Vec<u8> = vec![0; *width * *height];
        let frame = simple_frame::SimpleFrame::<machine_vision_formats::pixel_format::Mono8> {
            width: (*width).try_into().unwrap(),
            stride: (*width).try_into().unwrap(),
            height: (*height).try_into().unwrap(),
            image_data,
            fmt: std::marker::PhantomData,
        };
        let frame0_png_buf =
            convert_image::frame_to_image(&frame, convert_image::ImageOptions::Png)
                .unwrap()
                .into();

        Self {
            best_name,
            ros_name,
            frame0_png_buf,
            width: *width,
            height: *height,
        }
    }

    fn new_render_data(&self, pts_chrono: DateTime<Utc>) -> PerCamRenderFrame<'_> {
        PerCamRenderFrame {
            p: self,
            png_buf: None,
            points: vec![],
            pts_chrono,
        }
    }
}

pub(crate) struct PerCamRenderFrame<'a> {
    pub(crate) p: &'a PerCamRender,
    pub(crate) png_buf: Option<Vec<u8>>,
    pub(crate) points: Vec<(NotNan<f64>, NotNan<f64>)>,
    pub(crate) pts_chrono: DateTime<Utc>,
}

impl<'a> PerCamRenderFrame<'a> {
    pub(crate) fn set_original_image(&mut self, frame: &Frame) -> Result<()> {
        let png_buf = convert_image::frame_to_image(&frame, convert_image::ImageOptions::Png)?;
        self.png_buf = Some(png_buf);
        Ok(())
    }

    pub(crate) fn append_2d_point(&mut self, x: NotNan<f64>, y: NotNan<f64>) -> Result<()> {
        self.points.push((x, y));
        Ok(())
    }
}

struct CameraSource {
    cam_id: CameraIdentifier,
    per_cam_render: PerCamRender,
}

impl CameraSource {
    fn take_reader(&mut self) -> Option<Peek2<Box<dyn MovieReader>>> {
        match &mut self.cam_id {
            CameraIdentifier::MovieOnly(ref mut m) | CameraIdentifier::Both((ref mut m, _)) => {
                m.reader.take()
            }
            CameraIdentifier::BraidzOnly(_) => None,
        }
    }
}

enum CameraIdentifier {
    MovieOnly(MovieCamId),
    BraidzOnly(BraidzCamId),
    Both((MovieCamId, BraidzCamId)),
}

impl CameraIdentifier {
    fn best_name(&self) -> String {
        match self {
            CameraIdentifier::MovieOnly(m) | CameraIdentifier::Both((m, _)) => {
                // Prefer:
                // 1) configured name
                // 2) camera name saved in file metadata
                // 3) filename
                m.cfg_name.as_ref().cloned().unwrap_or_else(|| {
                    m.title
                        .as_ref()
                        .cloned()
                        .unwrap_or_else(|| m.filename.clone())
                })
            }
            CameraIdentifier::BraidzOnly(b) => b.ros_cam_name.clone(),
        }
    }
    fn ros_name(&self) -> Option<String> {
        match self {
            CameraIdentifier::MovieOnly(m) => m.ros_name(),
            CameraIdentifier::BraidzOnly(b) | CameraIdentifier::Both((_, b)) => {
                Some(b.ros_cam_name.clone())
            }
        }
    }
}

struct MovieCamId {
    /// Full path of the movie, including directory if given
    _full_path: std::path::PathBuf,
    /// The file reader
    reader: Option<Peek2<Box<dyn MovieReader>>>,
    /// File name of the movie (without directory path)
    filename: String,
    /// Name of camera given in configuration file
    cfg_name: Option<String>,
    /// Title given in movie metadata
    title: Option<String>,
    /// Title converted to ROS name (`-` replaced with `_`)
    title_as_ros: Option<String>,
    /// Filename converted to ROS name (`-` replaced with `_`)
    filename_as_ros: Option<String>,
}

impl MovieCamId {
    fn ros_name(&self) -> Option<String> {
        if let Some(title_as_ros) = &self.title_as_ros {
            return Some(title_as_ros.clone());
        }
        if let Some(filename_as_ros) = &self.filename_as_ros {
            return Some(filename_as_ros.clone());
        }
        None
    }
}

#[derive(Clone, Debug, PartialEq)]
struct BraidzCamId {
    ros_cam_name: String,
    camn: flydra_types::CamNum,
}

pub async fn run_config(cfg: &Valid<BraidRetrackVideoConfig>) -> Result<()> {
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

    let braidz_summary = braid_archive.as_ref().map(|archive| {
        let path = archive.path();
        let attr = std::fs::metadata(&path).unwrap();
        let filename = crate::config::path_to_string(&path).unwrap();
        braidz_parser::summarize_braidz(archive, filename, attr.len())
    });

    let tracking_parameters = braid_archive.as_ref().and_then(|archive| {
        archive
            .kalman_estimates_info
            .as_ref()
            .map(|kei| kei.tracking_parameters.clone())
    });

    let expected_framerate = braid_archive
        .as_ref()
        .map(|archive| archive.expected_fps as f32);

    let movie_re = regex::Regex::new(r"^movie\d{8}_\d{6}_(.*)$").unwrap();

    // Get `sources` from video inputs, parsing all camera names.
    let mut sources: Vec<CameraSource> = cfg
        .input_video
        .iter()
        .map(|s| {
            let reader = open_movie(&s.filename)?;
            let title: Option<String> = reader.title().map(Into::into);

            let reader = Some(Peek2::new(reader));

            let title_as_ros: Option<String> = title
                .as_ref()
                .map(|title| RawCamName::new(title.clone()).to_ros().as_str().to_string());

            let full_path = std::path::PathBuf::from(&s.filename);
            let filename = full_path
                .file_name()
                .unwrap()
                .to_os_string()
                .to_str()
                .unwrap()
                .to_string();

            let stem = filename.as_str().split('.').next().unwrap();
            // example: stem = "movie20211108_084523_Basler-22445994"

            let filename_as_ros = movie_re.captures(stem).map(|caps| {
                // get the raw camera name
                let raw = caps.get(1).unwrap().as_str();
                // example: raw = "Basler-22445994"

                let ros = RawCamName::new(raw.to_string())
                    .to_ros()
                    .as_str()
                    .to_string();
                // example: ros = "Basler_22445994"

                ros
            });

            let cam_id = CameraIdentifier::MovieOnly(MovieCamId {
                _full_path: full_path,
                filename,
                cfg_name: s.camera_name.clone(),
                title,
                title_as_ros,
                filename_as_ros,
                reader,
            });

            let per_cam_render = PerCamRender::from_reader(&cam_id);

            Ok(CameraSource {
                cam_id,
                per_cam_render,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let sources_ref = &mut sources;

    // Get `braidz_sources` from braidz input.
    let braidz_sources: Option<Vec<BraidzCamId>> = braidz_summary.map(|summary| {
        summary
            .cam_info
            .camid2camn
            .iter()
            .map(|(cam_id, camn)| BraidzCamId {
                ros_cam_name: cam_id.clone(),
                camn: *camn,
            })
            .collect()
    });

    // Update `sources` with info from braidz archive if they describe same camera.
    braidz_sources.as_ref().map(|braidz_sources| {
        for braidz_cam_id in braidz_sources.iter() {
            let tmp = sources_ref
                .drain(..)
                .map(|source| {
                    let cam_id = source.cam_id;
                    let per_cam_render = source.per_cam_render;

                    let cam_id = match cam_id {
                        CameraIdentifier::MovieOnly(m) => {
                            if Some(braidz_cam_id.ros_cam_name.clone()) == m.ros_name() {
                                CameraIdentifier::Both((m, braidz_cam_id.clone()))
                            } else {
                                CameraIdentifier::MovieOnly(m)
                            }
                        }
                        other => other,
                    };

                    CameraSource {
                        cam_id,
                        per_cam_render,
                    }
                })
                .collect::<Vec<CameraSource>>();

            *sources_ref = tmp;
        }
    });

    // If we have no manually specified video sources but do have a braidz file, use that.
    let braidz_only = if sources.is_empty() {
        if let Some(braidz_sources) = braidz_sources {
            let mut cam_ids = braidz_sources
                .into_iter()
                .map(|bs| {
                    let per_cam_render =
                        PerCamRender::from_braidz(braid_archive.as_ref().unwrap(), &bs);
                    CameraSource {
                        cam_id: CameraIdentifier::BraidzOnly(bs),
                        per_cam_render,
                    }
                })
                .collect::<Vec<_>>();
            sources.append(&mut cam_ids);
            true
        } else {
            log::info!("No sources given (either video files or braidz archive).");
            return Ok(());
        }
    } else {
        false
    };

    let mut data2d = BTreeMap::new();
    if let Some(ref mut braidz) = braid_archive.as_mut() {
        for row in braidz.iter_data2d_distorted()? {
            let row = row?;
            let cam_entry = &mut data2d.entry(row.camn).or_insert_with(Vec::new);
            cam_entry.push(row);
        }
    }

    let ros_camera_names: Vec<String> = sources
        .iter()
        .map(|s| match &s.cam_id {
            CameraIdentifier::MovieOnly(m) | CameraIdentifier::Both((m, _)) => {
                m.ros_name().unwrap()
            }
            CameraIdentifier::BraidzOnly(b) => b.ros_cam_name.clone(),
        })
        .collect();

    // Build iterator to iterate over output frames. This is equivalent to
    // iterating over synchronized input frames.
    let moment_iter: Box<dyn Iterator<Item = _>> = if braidz_only {
        let braid_archive = braid_archive.unwrap();
        let boxed = Box::new(braid_archive);
        let statik: &'static mut _ = Box::leak(boxed);

        let camns: Vec<flydra_types::CamNum> = sources
            .iter()
            .map(|s| match &s.cam_id {
                CameraIdentifier::BraidzOnly(b) => b.camn,
                _ => panic!("impossible"),
            })
            .collect();

        Box::new(braidz_iter::BraidArchiveNoVideoData::new(statik, camns)?)
    } else {
        let mut frame_readers: Vec<_> = sources
            .iter_mut()
            .map(|s| s.take_reader().unwrap())
            .collect();

        // Determine which video started last and what time was the last start time.
        // This time is where we will start from.
        let approx_start_time: Option<DateTime<Utc>> = frame_readers
            .iter()
            .map(|reader| reader.as_ref().creation_time())
            .max()
            .map(Clone::clone);

        if let Some(approx_start_time) = &approx_start_time {
            log::info!("start time determined from videos: {}", approx_start_time);
        }

        let frame_duration = cfg
            .frame_duration_microsecs
            .map(|x| chrono::Duration::from_std(std::time::Duration::from_micros(x)).unwrap())
            .unwrap_or_else(|| {
                frame_readers
                    .iter()
                    .map(|reader| {
                        let p1_pts_chrono = reader.peek1().unwrap().as_ref().unwrap().pts_chrono;
                        let p2_pts_chrono = reader.peek2().unwrap().as_ref().unwrap().pts_chrono;
                        p2_pts_chrono - p1_pts_chrono
                    })
                    .min()
                    .unwrap()
            });

        let sync_threshold = cfg
            .sync_threshold_microseconds
            .map(|x| chrono::Duration::from_std(std::time::Duration::from_micros(x)).unwrap())
            .unwrap_or(frame_duration / 2);

        log::info!(
            "sync_threshold: {} microseconds",
            sync_threshold.num_microseconds().unwrap()
        );

        if let Some(archive) = braid_archive {
            // In this path, we use the .braidz file as the source of
            // synchronization.

            let ros_camera_names_ref: Vec<&str> =
                ros_camera_names.iter().map(|x| x.as_str()).collect();

            Box::new(braidz_iter::BraidArchiveSyncVideoData::new(
                archive,
                &data2d,
                &ros_camera_names_ref,
                frame_readers,
                sync_threshold,
            )?)
        } else if let Some(approx_start_time) = approx_start_time {
            // In this path, we use the timestamps in the saved videos as the source
            // of synchronization.
            synchronize_readers_from(approx_start_time, &mut frame_readers);

            Box::new(synced_iter::SyncedIter::new(
                frame_readers,
                sync_threshold,
                frame_duration,
            )?)
        } else {
            anyhow::bail!(
                "Neither braidz archive nor input videos could be used as source of frame data."
            );
        }
    };

    let all_expected_cameras = ros_camera_names
        .iter()
        .map(|x| RosCamName::new(x.clone()))
        .collect::<std::collections::BTreeSet<RosCamName>>();

    // Initialize outputs
    let output_storage: Vec<Result<OutputStorage, _>> =
        join_all(cfg.output.clone().into_iter().map(|output| async {
            // Create output dirs if needed.
            let output_filename = std::path::PathBuf::from(output.filename());
            if let Some(dest_dir) = output_filename.parent() {
                std::fs::create_dir_all(dest_dir)?;
            }

            match output {
                OutputConfig::Video(v) => Ok(OutputStorage::Video(
                    output_video::VideoStorage::new(&v, &output_filename, &sources)?,
                )),
                OutputConfig::DebugTxt(_) => Ok(OutputStorage::Debug(DebugStorage {
                    fd: std::fs::File::create(&output_filename)?,
                })),
                OutputConfig::Braidz(b) => Ok(OutputStorage::Braid(
                    output_braidz::BraidStorage::new(
                        cfg,
                        &b,
                        tracking_parameters.clone(),
                        &sources,
                        all_expected_cameras.clone(),
                        expected_framerate,
                    )
                    .await?,
                )),
            }
        }))
        .await;

    let mut output_storage: Vec<_> = output_storage.into_iter().collect::<Result<Vec<_>>>()?;

    // Trim to maximum number of frames.
    let moment_iter = match cfg.max_num_frames {
        Some(max_num_frames) => Box::new(moment_iter.take(max_num_frames)),
        None => moment_iter,
    };

    // Iterate over all output frames.
    for (out_fno, synced_data) in moment_iter.enumerate() {
        let synced_data = synced_data?;

        if let Some(start_frame) = cfg.skip_n_first_output_frames {
            if out_fno < start_frame {
                continue;
            }
        }

        for output in output_storage.iter_mut() {
            if let OutputStorage::Debug(d) = output {
                writeln!(d.fd, "output frame {} ----------", out_fno)?;
            }
        }

        if out_fno % cfg.log_interval_frames.unwrap_or(100) == 0 {
            log::info!("frame {}", out_fno);
        }

        // --- Collect input data for this timepoint. -----
        let all_cam_render_data =
            gather_frame_data(&synced_data, &sources, &mut output_storage, cfg)?;

        // --- Done collecting input data for this timepoint. -----
        for output in output_storage.iter_mut() {
            output
                .render_frame(out_fno, &synced_data, &all_cam_render_data)
                .await?;
        }
    }

    Ok(())
}

pub(crate) trait MovieReader {
    fn title(&self) -> Option<&str>;
    fn filename(&self) -> &str;
    fn creation_time(&self) -> &DateTime<Utc>;
    fn next_frame(&mut self) -> Option<Result<Frame>>;
}

fn open_movie(filename: &str) -> Result<Box<dyn MovieReader>> {
    if filename.to_lowercase().ends_with(".fmf") || filename.to_lowercase().ends_with(".fmf.gz") {
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

fn gather_frame_data<'a>(
    synced_data: &SyncedPictures,
    sources: &'a [CameraSource],
    output_storage: &mut Vec<OutputStorage>,
    cfg: &BraidRetrackVideoConfig,
) -> Result<Vec<PerCamRenderFrame<'a>>> {
    let synced_pics: &[OutTimepointPerCamera] = &synced_data.camera_pictures;

    let n_pics = synced_pics.len();
    let mut all_cam_render_data = Vec::with_capacity(n_pics);
    assert_eq!(n_pics, sources.len());
    for (per_cam, source) in synced_pics.iter().zip(sources.iter()) {
        // Copy the default information for this camera and then we will
        // start adding information relevant for this frame in time.
        let mut cam_render_data = source.per_cam_render.new_render_data(per_cam.timestamp);

        // Did we get an image from the MKV file?
        if let Some(pic) = &per_cam.image {
            cam_render_data.set_original_image(pic)?;
        }
        let mut wrote_debug = false;

        cam_render_data.pts_chrono = per_cam.timestamp;

        for (rowi, row_data2d) in per_cam.this_cam_this_frame.iter().enumerate() {
            {
                let row_dt: DateTime<Utc> = (&row_data2d.cam_received_timestamp).into();
                for output in output_storage.iter_mut() {
                    if let OutputStorage::Debug(d) = output {
                        writeln!(
                            d.fd,
                            "   Collect {}: {} ({}), rowi {}, {} ({}), {}, {}",
                            source.cam_id.best_name(),
                            per_cam.timestamp,
                            datetime_conversion::datetime_to_f64(&per_cam.timestamp),
                            rowi,
                            row_dt,
                            row_data2d.cam_received_timestamp.as_f64(),
                            row_data2d.x,
                            row_data2d.y,
                        )?;
                        wrote_debug = true;
                    }
                }
            }

            match &cfg.processing_config.feature_detection_method {
                FeatureDetectionMethod::CopyExisting => {
                    if let Ok(x) = NotNan::new(row_data2d.x) {
                        let y = NotNan::new(row_data2d.y).unwrap();
                        cam_render_data.append_2d_point(x, y)?;
                    }
                }
            }
        }

        if !wrote_debug {
            for output in output_storage.iter_mut() {
                if let OutputStorage::Debug(d) = output {
                    writeln!(
                        d.fd,
                        "   Collect {}: {} ({}) no points",
                        source.cam_id.best_name(),
                        per_cam.timestamp,
                        datetime_conversion::datetime_to_f64(&per_cam.timestamp),
                    )?;
                    #[allow(unused_assignments)]
                    {
                        wrote_debug = true;
                    }
                }
            }
        }

        all_cam_render_data.push(cam_render_data);
    }
    Ok(all_cam_render_data)
}
