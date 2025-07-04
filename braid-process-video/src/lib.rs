use std::{collections::BTreeMap, io::Write};

use chrono::{DateTime, FixedOffset, TimeDelta};
use eyre::{self as anyhow, Result, WrapErr};
use flydra_mvg::FlydraMultiCameraSystem;
use frame_source::{FrameData, FrameDataSource};
use futures::future::join_all;
use indicatif::{ProgressBar, ProgressStyle};
use ordered_float::NotNan;

use machine_vision_formats::{owned::OImage, pixel_format::Mono8};

use braid_types::{Data2dDistortedRow, KalmanEstimatesRow, RawCamName};

mod peek2;
use peek2::Peek2;

mod argmin;

use strand_dynamic_frame::{DynamicFrame, DynamicFrameOwned};

mod braidz_iter;
mod synced_iter;

mod config;
pub(crate) use config::FeatureDetectionMethod;
pub use config::{
    BraidRetrackVideoConfig, BraidzOutputConfig, OutputConfig, Valid, Validate, VideoOutputConfig,
    VideoSourceConfig,
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

pub(crate) const DEFAULT_REPROJECTED_RADIUS: &str = "12";
pub(crate) const DEFAULT_REPROJECTED_STYLE: &str = "fill: none; stroke: white; stroke-width: 3;";

#[derive(Debug)]
pub(crate) struct OutTimepointPerCamera {
    timestamp: DateTime<FixedOffset>,
    /// Camera image from MP4, MKV, or FMF file (if available).
    image: Option<DynamicFrameOwned>,
    /// Braidz data. Empty if no braidz data available.
    this_cam_this_frame: Vec<Data2dDistortedRow>,
}

impl OutTimepointPerCamera {
    pub(crate) fn new(
        timestamp: DateTime<FixedOffset>,
        image: Option<DynamicFrameOwned>,
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
    pub(crate) timestamp: DateTime<FixedOffset>,
    pub(crate) camera_pictures: Vec<OutTimepointPerCamera>,
    /// If a braidz file was used as synchronization source, more data is
    /// available.
    pub(crate) braidz_info: Option<BraidzFrameInfo>,
    pub(crate) recon: Option<FlydraMultiCameraSystem<f64>>,
}

impl SyncedPictures {
    fn project_kests(
        &self,
        cam: &CameraSource,
        recon: &Option<FlydraMultiCameraSystem<f64>>,
    ) -> Vec<(NotNan<f64>, NotNan<f64>)> {
        let recon = match recon {
            Some(recon) => recon,
            None => {
                return vec![];
            }
        };
        let cam_name = &cam.per_cam_render.raw_name;
        let cam = match recon.cam_by_name(cam_name.as_str()) {
            Some(cam) => cam,
            None => {
                return vec![];
            }
        };

        match &self.braidz_info {
            Some(braidz_info) => braidz_info
                .kalman_estimates
                .iter()
                .filter_map(|kest_row| {
                    let pt3d = braid_mvg::PointWorldFrame {
                        coords: nalgebra::Point3::new(kest_row.x, kest_row.y, kest_row.z),
                    };
                    let pix2d = cam.project_3d_to_distorted_pixel(&pt3d);
                    let x = pix2d.coords.x;
                    let y = pix2d.coords.y;
                    if x >= 0.0 && y >= 0.0 && x <= cam.width() as f64 && y <= cam.height() as f64 {
                        Some((NotNan::new(x).unwrap(), NotNan::new(y).unwrap()))
                    } else {
                        None
                    }
                })
                .collect(),
            None => {
                vec![]
            }
        }
    }
}

#[derive(Debug)]
pub(crate) struct BraidzFrameInfo {
    frame_num: i64,
    trigger_timestamp: Option<braid_types::FlydraFloatTimestampLocal<braid_types::Triggerbox>>,
    kalman_estimates: Vec<KalmanEstimatesRow>,
}

/// advance the iterators in `readers` so that they are all start around `approx_start_time`.
fn synchronize_readers_from(
    approx_start_time: DateTime<FixedOffset>,
    readers: &mut [Peek2<Box<dyn Iterator<Item = frame_source::Result<FrameData>>>>],
    frame0_times: &[chrono::DateTime<chrono::FixedOffset>],
) {
    // Advance each reader until upcoming frame is not before the start time.
    for (reader, frame0_time) in readers.iter_mut().zip(frame0_times) {
        // tracing::debug!("filename: {}", reader.as_ref().filename().display());

        // Get information for first frame
        let p1_pts = reader
            .peek1()
            .unwrap()
            .as_ref()
            .unwrap()
            .timestamp()
            .unwrap_duration();
        let p1_pts_chrono = *frame0_time + TimeDelta::from_std(p1_pts).unwrap();
        let p2_pts = reader
            .peek2()
            .unwrap()
            .as_ref()
            .unwrap()
            .timestamp()
            .unwrap_duration();
        let p2_pts_chrono = *frame0_time + TimeDelta::from_std(p2_pts).unwrap();
        let mut p1_delta = (p1_pts_chrono - approx_start_time)
            .num_nanoseconds()
            .unwrap()
            .abs();

        tracing::debug!("  p1_pts_chrono: {}", p1_pts_chrono);
        tracing::debug!("  p2_pts_chrono: {}", p2_pts_chrono);
        tracing::debug!("  p1_delta: {}", p1_delta);

        if p1_pts_chrono >= approx_start_time {
            // First frame is already after the start time, use it unconditionally.
            continue;
        } else {
            loop {
                // Get information for second frame
                if let Some(p2_frame) = reader.peek2() {
                    let p2_pts = p2_frame.as_ref().unwrap().timestamp().unwrap_duration();
                    let p2_pts_chrono = *frame0_time + TimeDelta::from_std(p2_pts).unwrap();
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

#[derive(Debug)]
struct PerCamRender {
    best_name: String,
    raw_name: RawCamName,
    frame0_png_buf: braid_types::PngImageData,
    width: usize,
    height: usize,
}

impl PerCamRender {
    fn from_reader(cam_id: &CameraIdentifier) -> Self {
        let best_name = cam_id.best_name();
        let raw_name = RawCamName::new(best_name.clone());

        let rdr = match &cam_id {
            CameraIdentifier::MovieOnly(m) | CameraIdentifier::Both((m, _)) => {
                m.reader.as_ref().unwrap()
            }
            _ => {
                panic!("")
            }
        };
        let frame_ref: &DynamicFrame = &rdr.peek1().unwrap().as_ref().unwrap().decoded().unwrap();

        let frame0_png_buf = frame_ref
            .to_encoded_buffer(convert_image::EncoderOptions::Png)
            .unwrap()
            .into();

        let width = frame_ref.width().try_into().unwrap();
        let height = frame_ref.height().try_into().unwrap();

        Self {
            best_name,
            raw_name,
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
        let (width, height) = image_sizes.get(&braidz_cam.cam_id_str).unwrap();
        let best_name = braidz_cam.cam_id_str.clone(); // this is the best we can do
        let raw_name = RawCamName::new(best_name.clone());

        // generate blank first image of the correct size.
        let image_data: Vec<u8> = vec![0; *width * *height];
        let frame = OImage::<Mono8>::new(
            (*width).try_into().unwrap(),
            (*height).try_into().unwrap(),
            *width,
            image_data,
        )
        .unwrap();
        let frame0_png_buf =
            convert_image::frame_to_encoded_buffer(&frame, convert_image::EncoderOptions::Png)
                .unwrap()
                .into();

        Self {
            best_name,
            raw_name,
            frame0_png_buf,
            width: *width,
            height: *height,
        }
    }

    fn new_render_data(&self, pts_chrono: DateTime<FixedOffset>) -> PerCamRenderFrame<'_> {
        PerCamRenderFrame {
            p: self,
            png_buf: None,
            points: vec![],
            reprojected_points: vec![],
            pts_chrono,
        }
    }
}

pub(crate) struct PerCamRenderFrame<'a> {
    pub(crate) p: &'a PerCamRender,
    pub(crate) png_buf: Option<Vec<u8>>,
    pub(crate) points: Vec<(NotNan<f64>, NotNan<f64>)>,
    pub(crate) reprojected_points: Vec<(NotNan<f64>, NotNan<f64>)>,
    pub(crate) pts_chrono: DateTime<FixedOffset>,
}

impl PerCamRenderFrame<'_> {
    pub(crate) fn set_original_image(&mut self, frame: &DynamicFrame) -> Result<()> {
        let png_buf = frame.to_encoded_buffer(convert_image::EncoderOptions::Png)?;
        self.png_buf = Some(png_buf);
        Ok(())
    }

    pub(crate) fn append_2d_point(&mut self, x: NotNan<f64>, y: NotNan<f64>) -> Result<()> {
        self.points.push((x, y));
        Ok(())
    }
}

#[derive(Debug)]
struct CameraSource {
    cam_id: CameraIdentifier,
    per_cam_render: PerCamRender,
}

impl CameraSource {
    fn take_reader(
        &mut self,
    ) -> Option<Peek2<Box<dyn Iterator<Item = frame_source::Result<FrameData>>>>> {
        match &mut self.cam_id {
            CameraIdentifier::MovieOnly(ref mut m) | CameraIdentifier::Both((ref mut m, _)) => {
                m.reader.take()
            }
            CameraIdentifier::BraidzOnly(_) => None,
        }
    }
    fn camsrc_framerate(
        &self,
        braidz_archive: Option<&braidz_parser::BraidzArchive<std::io::BufReader<std::fs::File>>>,
    ) -> Option<f64> {
        match &self.cam_id {
            CameraIdentifier::MovieOnly(ref m) => m.framerate(),
            CameraIdentifier::BraidzOnly(_) | CameraIdentifier::Both((_, _)) => {
                Some(braidz_archive.unwrap().expected_fps)
            }
        }
    }
}

#[derive(Debug, Hash, Eq, PartialEq)]
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
                // 2) camera name saved as title in file metadata
                // 3) cam_from_filename
                // 4) filename
                m.cfg_name.as_ref().cloned().unwrap_or_else(|| {
                    m.title.as_ref().cloned().unwrap_or_else(|| {
                        m.cam_from_filename
                            .as_ref()
                            .cloned()
                            .unwrap_or_else(|| m.filename.clone())
                    })
                })
            }
            CameraIdentifier::BraidzOnly(b) => b.cam_id_str.clone(),
        }
    }
    fn frame0_time(&self) -> chrono::DateTime<chrono::FixedOffset> {
        match self {
            CameraIdentifier::MovieOnly(m) | CameraIdentifier::Both((m, _)) => m.frame0_time,
            CameraIdentifier::BraidzOnly(_b) => {
                todo!()
            }
        }
    }
}

struct MovieCamId {
    /// Full path of the movie, including directory if given
    _full_path: std::path::PathBuf,
    /// The file reader
    reader: Option<Peek2<Box<dyn Iterator<Item = frame_source::Result<FrameData>>>>>,
    my_framerate: Option<f64>,
    /// File name of the movie (without directory path)
    filename: String,
    /// Source of timestamp data in the video file
    timestamp_source: String,
    /// Name of camera given in configuration file
    cfg_name: Option<String>,
    /// Title given in movie metadata
    title: Option<String>,
    /// Camera name extracted from filename
    cam_from_filename: Option<String>,
    frame0_time: chrono::DateTime<chrono::FixedOffset>,
}

impl PartialEq for MovieCamId {
    fn eq(&self, other: &Self) -> bool {
        self._full_path == other._full_path
            && self.filename == other.filename
            && self.timestamp_source == other.timestamp_source
    }
}

impl Eq for MovieCamId {}

impl std::hash::Hash for MovieCamId {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self._full_path.hash(state);
        self.filename.hash(state);
        self.timestamp_source.hash(state);
    }
}

impl MovieCamId {
    fn framerate(&self) -> Option<f64> {
        self.my_framerate
    }
    fn raw_name(&self) -> Option<String> {
        if let Some(title) = &self.title {
            return Some(title.clone());
        }
        if let Some(cam_from_filename) = &self.cam_from_filename {
            return Some(cam_from_filename.clone());
        }
        None
    }
}

impl std::fmt::Debug for MovieCamId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MovieCamId")
            .field("filename", &self.filename)
            .field("timestamp_source", &self.timestamp_source)
            .field("cfg_name", &self.cfg_name)
            .field("title", &self.title)
            .field("cam_from_filename", &self.cam_from_filename)
            .field("frame0_time", &self.frame0_time)
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Hash, Eq)]
struct BraidzCamId {
    cam_id_str: String,
    camn: braid_types::CamNum,
}

pub async fn run_config(
    cfg: &Valid<BraidRetrackVideoConfig>,
    show_progress: bool,
) -> Result<Vec<std::path::PathBuf>> {
    let cfg = cfg.valid();

    let mut braid_archive = if let Some(input_braidz) = cfg.input_braidz.as_ref() {
        tracing::debug!("parsing braidz file");
        Some(
            braidz_parser::braidz_parse_path(&input_braidz).with_context(|| {
                format!(
                    "opening braidz archive {}",
                    cfg.input_braidz.as_ref().unwrap()
                )
            })?,
        )
    } else {
        None
    };

    let braidz_summary = braid_archive.as_ref().map(|archive| {
        let path = archive.path();
        let attr = std::fs::metadata(path).unwrap();
        let filename = crate::config::path_to_string(path).unwrap();
        braidz_parser::summarize_braidz(archive, filename, attr.len())
    });

    let braidz_tracking_params = braid_archive.as_ref().and_then(|archive| {
        archive
            .kalman_estimates_info
            .as_ref()
            .map(|kei| kei.tracking_parameters.clone())
    });

    let braidz_calibration = braid_archive
        .as_ref()
        .and_then(|archive| archive.calibration_info.clone());

    let expected_framerate = braid_archive
        .as_ref()
        .map(|archive| archive.expected_fps as f32);

    tracing::debug!("Opening {} frame sources", cfg.input_video.len());
    let frame_sources: Vec<_> = cfg
        .input_video
        .iter()
        .map(|s| {
            tracing::debug!("Opening frame source {}", s.filename);
            frame_source::FrameSourceBuilder::new(&s.filename)
                .timestamp_source(s.timestamp_source.clone())
                .build_source()
        })
        .collect::<frame_source::Result<Vec<Box<dyn FrameDataSource>>>>()?;
    tracing::debug!("Frame sources opened");

    let frame_sources = Box::new(frame_sources);
    let frame_sources: &'static mut [Box<dyn FrameDataSource>] = frame_sources.leak();

    // Get `sources` from video inputs, parsing all camera names.
    let mut sources: Vec<CameraSource> = cfg
        .input_video
        .iter()
        .zip(frame_sources.iter_mut())
        .map(|(s, frame_source)| {
            let frame0_time = frame_source.frame0_time().ok_or_else(|| {
                eyre::eyre!(
                    "could not determine first frame timestamp for {}",
                    s.filename
                )
            })?;

            let frame0_time = if !cfg.disable_local_time {
                // convert to local timezone
                let frame0_local: DateTime<chrono::Local> = frame0_time.into();
                // convert back to fixed offset
                frame0_local.into()
            } else {
                frame0_time
            };

            let my_framerate = frame_source
                .average_framerate();
            let timestamp_source: String = frame_source.timestamp_source().into();

            let title: Option<String> = frame_source.camera_name().map(Into::into);

            let reader = Some(Peek2::new(frame_source.iter()));

            let full_path = std::path::PathBuf::from(&s.filename);

            let (filename, cam_from_filename) = braidz_types::camera_name_from_filename(&full_path);
            let av_fps_str = if let Some(av_fps) = &my_framerate {
                format!("{av_fps:.2}")
            } else {
                "<unknown>".to_string()
            };
            tracing::info!("Video source: {filename}, timestamp_source: {timestamp_source}, average framerate: {av_fps_str}");

            let cam_id = CameraIdentifier::MovieOnly(MovieCamId {
                _full_path: full_path,
                filename,
                timestamp_source,
                cfg_name: s.camera_name.clone(),
                title,
                cam_from_filename,
                frame0_time,
                reader,
                my_framerate,
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
                cam_id_str: cam_id.clone(),
                camn: *camn,
            })
            .collect()
    });

    // Update `sources` with info from braidz archive if they describe same camera.
    if let Some(braidz_sources) = braidz_sources.as_ref() {
        for braidz_cam_id in braidz_sources.iter() {
            let tmp = sources_ref
                .drain(..)
                .map(|source| {
                    let cam_id = source.cam_id;
                    let per_cam_render = source.per_cam_render;

                    let cam_id = match cam_id {
                        CameraIdentifier::MovieOnly(m) => {
                            if let Some(raw_name) = m.raw_name().as_ref() {
                                let ros_camid = crate::braidz_iter::as_ros_camid(raw_name);
                                if (&braidz_cam_id.cam_id_str == raw_name)
                                    || (braidz_cam_id.cam_id_str == ros_camid)
                                {
                                    CameraIdentifier::Both((m, braidz_cam_id.clone()))
                                } else {
                                    CameraIdentifier::MovieOnly(m)
                                }
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
    };

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
            tracing::info!("No sources given (either video files or braidz archive).");
            return Ok(vec![]);
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

    let camera_names: Vec<String> = sources
        .iter()
        .map(|s| match &s.cam_id {
            CameraIdentifier::MovieOnly(m) | CameraIdentifier::Both((m, _)) => m
                .raw_name()
                .ok_or_else(|| eyre::eyre!("no raw_name for {m:?}")),
            CameraIdentifier::BraidzOnly(b) => Ok(b.cam_id_str.clone()),
        })
        .collect::<Result<_>>()?;

    // Build iterator to iterate over output frames. This is equivalent to
    // iterating over synchronized input frames.
    let moment_iter: Box<dyn Iterator<Item = _>> = if braidz_only {
        let braid_archive = braid_archive.unwrap();
        let boxed = Box::new(braid_archive);
        let statik: &'static mut _ = Box::leak(boxed);

        let camns: Vec<braid_types::CamNum> = sources
            .iter()
            .map(|s| match &s.cam_id {
                CameraIdentifier::BraidzOnly(b) => b.camn,
                _ => panic!("impossible"),
            })
            .collect();

        let braid_archive = braidz_iter::BraidArchiveNoVideoData::new(statik, camns)?;
        Box::new(braid_archive)
    } else {
        let framerates = sources
            .iter()
            .map(|r| {
                r.camsrc_framerate(braid_archive.as_ref()).ok_or_else(|| {
                    eyre::eyre!("no framerate determined for {}", r.cam_id.best_name())
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let mut frame_readers: Vec<_> = sources
            .iter_mut()
            .map(|s| s.take_reader().unwrap())
            .collect();

        let frame0_times: Vec<chrono::DateTime<chrono::FixedOffset>> =
            sources.iter().map(|s| s.cam_id.frame0_time()).collect();

        // Determine which video started last and what time was the last start time.
        // This time is where we will start from.
        let approx_start_time: Option<DateTime<_>> = frame0_times.iter().max().copied();

        if let Some(approx_start_time) = &approx_start_time {
            tracing::info!("start time determined from videos: {}", approx_start_time);
        }

        let frame_duration = cfg
            .frame_duration_microsecs
            .map(|x| chrono::Duration::from_std(std::time::Duration::from_micros(x)).unwrap())
            .unwrap_or_else(|| {
                // Take average framerate of all cameras.
                let avg_fps = framerates.iter().sum::<f64>() / framerates.len() as f64;
                let dt = 1.0 / avg_fps;
                chrono::TimeDelta::from_std(std::time::Duration::from_secs_f64(dt)).unwrap()
            });
        tracing::debug!(
            "frame_duration: {} microseconds",
            frame_duration.num_microseconds().unwrap()
        );

        let sync_threshold = cfg
            .sync_threshold_microseconds
            .map(|x| chrono::TimeDelta::from_std(std::time::Duration::from_micros(x)).unwrap())
            .unwrap_or(frame_duration / 2);
        tracing::debug!(
            "sync_threshold: {} microseconds",
            sync_threshold.num_microseconds().unwrap()
        );

        if let Some(archive) = braid_archive {
            // In this path, we use the .braidz file as the source of
            // synchronization.

            let camera_names_ref: Vec<&str> = camera_names.iter().map(|x| x.as_str()).collect();

            Box::new(braidz_iter::BraidArchiveSyncVideoData::new(
                archive,
                &data2d,
                &camera_names_ref,
                frame_readers,
                sync_threshold,
                frame0_times,
            )?)
        } else if let Some(approx_start_time) = approx_start_time {
            // In this path, we use the timestamps in the saved videos as the source
            // of synchronization.
            synchronize_readers_from(approx_start_time, &mut frame_readers, &frame0_times);

            Box::new(synced_iter::SyncedIter::new(
                frame_readers,
                sync_threshold,
                frame_duration,
                frame0_times,
            )?)
        } else {
            anyhow::bail!(
                "Neither braidz archive nor input videos could be used as source of frame data."
            );
        }
    };

    let all_expected_cameras = camera_names
        .iter()
        .map(|x| RawCamName::new(x.clone()))
        .collect::<std::collections::BTreeSet<_>>();

    // Initialize outputs
    let output_storage: Vec<Result<OutputStorage, _>> =
        join_all(cfg.output.clone().into_iter().map(|output| async {
            // Create output dirs if needed.
            let output_filename = std::path::PathBuf::from(output.filename());
            if let Some(dest_dir) = output_filename.parent() {
                std::fs::create_dir_all(dest_dir)
                    .with_context(|| format!("while creating directory {}", dest_dir.display()))?;
            }

            match output {
                OutputConfig::Video(v) => Ok(OutputStorage::Video(Box::new(
                    output_video::VideoStorage::new(&v, &output_filename, &sources)?,
                ))),
                OutputConfig::DebugTxt(_) => Ok(OutputStorage::Debug(DebugStorage {
                    path: output_filename.clone(),
                    fd: std::fs::File::create(&output_filename)?,
                })),
                OutputConfig::Braidz(b) => {
                    let braidz_storage = output_braidz::BraidStorage::new(
                        cfg,
                        &b,
                        braidz_tracking_params.clone(),
                        &sources,
                        all_expected_cameras.clone(),
                        expected_framerate,
                        braidz_calibration.clone(),
                    )
                    .await?;

                    Ok(OutputStorage::Braid(braidz_storage))
                }
            }
        }))
        .await;

    let mut output_storage: Vec<_> = output_storage.into_iter().collect::<Result<Vec<_>>>()?;

    // Trim to maximum number of frames.
    let moment_iter = match cfg.max_num_frames {
        Some(max_num_frames) => Box::new(moment_iter.take(max_num_frames)),
        None => moment_iter,
    };

    let mut pb = if show_progress {
        match moment_iter.size_hint().1 {
            Some(n_expected) => {
                // Custom progress bar with space at right end to prevent obscuring last
                // digit with cursor.
                let style = ProgressStyle::with_template("{wide_bar} {pos}/{len} ETA: {eta} ")?;
                Some(ProgressBar::new(n_expected.try_into().unwrap()).with_style(style))
            }
            None => Some(ProgressBar::new_spinner()),
        }
    } else {
        None
    };

    let mut history_state = HistoryState::new();

    // Iterate over all output frames.
    for (out_fno, synced_data) in moment_iter.enumerate() {
        if let Some(pb) = pb.as_mut() {
            pb.set_position(out_fno.try_into().unwrap());
        }
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
            tracing::info!("frame {}", out_fno);
        }

        // --- Collect input data for this timepoint. -----
        let all_cam_render_data = gather_frame_data(
            out_fno,
            &synced_data,
            &sources,
            &mut output_storage,
            cfg,
            &mut history_state,
        )?;

        // --- Done collecting input data for this timepoint. -----
        for output in output_storage.iter_mut() {
            output
                .render_frame(out_fno, &synced_data, &all_cam_render_data)
                .await?;
        }
    }

    if let Some(pb) = pb.as_mut() {
        pb.finish_and_clear();
    }

    // collect output filenames
    let outputs = output_storage
        .iter()
        .map(|d| d.path().to_path_buf())
        .collect();

    for output in output_storage.into_iter() {
        output.close().await?;
    }

    Ok(outputs)
}

/// This stores all persistent state across all cameras.
struct HistoryState<'a> {
    flydra_feature_detectors: std::collections::HashMap<
        &'a CameraIdentifier,
        flydra_feature_detector::FlydraFeatureDetector,
    >,
}

impl<'a> HistoryState<'a> {
    fn new() -> Self {
        Self {
            flydra_feature_detectors: Default::default(),
        }
    }
}

fn gather_frame_data<'a>(
    out_fno: usize,
    synced_data: &SyncedPictures,
    sources: &'a [CameraSource],
    output_storage: &mut [OutputStorage],
    cfg: &BraidRetrackVideoConfig,
    history_state: &mut HistoryState<'a>,
) -> Result<Vec<PerCamRenderFrame<'a>>> {
    let synced_pics: &[OutTimepointPerCamera] = &synced_data.camera_pictures;

    let n_pics = synced_pics.len();
    let mut all_cam_render_data = Vec::with_capacity(n_pics);
    assert_eq!(n_pics, sources.len());
    for (per_cam, source) in synced_pics.iter().zip(sources.iter()) {
        let mut wrote_debug = false;

        // Copy the default information for this camera and then we will
        // start adding information relevant for this frame in time.
        let mut cam_render_data = source.per_cam_render.new_render_data(per_cam.timestamp);

        match &cfg.processing_config.feature_detection_method {
            FeatureDetectionMethod::CopyExisting => {
                // Copy existing data
                for row_data2d in per_cam.this_cam_this_frame.iter() {
                    // Debug output.
                    {
                        for output in output_storage.iter_mut() {
                            if let OutputStorage::Debug(d) = output {
                                writeln!(
                                    d.fd,
                                    "   Collect {}: {}, frame {}, {}, {}",
                                    source.cam_id.best_name(),
                                    per_cam.timestamp,
                                    row_data2d.frame,
                                    row_data2d.x,
                                    row_data2d.y,
                                )?;
                                wrote_debug = true;
                            }
                        }
                    }

                    // Camera render data
                    if let Ok(x) = NotNan::new(row_data2d.x) {
                        let y = NotNan::new(row_data2d.y).unwrap();
                        cam_render_data.append_2d_point(x, y)?;
                    }
                }
            }
            FeatureDetectionMethod::Flydra(per_cam_cfg) => {
                if let Some(pic) = &per_cam.image {
                    let entry = history_state
                        .flydra_feature_detectors
                        .entry(&source.cam_id)
                        .or_insert_with(|| {
                            // create new FlydraFeatureDetector
                            let best_name = source.cam_id.best_name();
                            let im_pt_cfg = per_cam_cfg
                                .get(&best_name)
                                .map(Clone::clone)
                                .unwrap_or_else(|| {
                                    tracing::info!("for {}: creating default flydra feature detector configuration",
                                        source.cam_id.best_name());
                                    flydra_pt_detect_cfg::default_absdiff()
                                });
                            let raw_cam_name = RawCamName::new(source.cam_id.best_name());
                            let pic_ref = pic.borrow();
                            flydra_feature_detector::FlydraFeatureDetector::new(
                                &raw_cam_name,
                                pic_ref.width(),
                                pic_ref.height(),
                                im_pt_cfg,
                                None,
                                None,
                            )
                            .unwrap()
                        });

                    tracing::warn!("converting image to MONO8");
                    let frame_ref = pic.borrow();
                    let mono8 = frame_ref
                        .into_pixel_format::<machine_vision_formats::pixel_format::Mono8>()?;
                    let dyn_mono8 = DynamicFrame::from_static_ref(&mono8);

                    let (detections, _) = entry.process_new_frame(
                        &dyn_mono8,
                        out_fno,
                        per_cam.timestamp.into(),
                        flydra_feature_detector::UfmfState::Stopped,
                        None,
                        None,
                        None,
                    )?;
                    for point in detections.points.into_iter() {
                        let x = NotNan::new(point.x0_abs)?;
                        let y = NotNan::new(point.y0_abs)?;
                        cam_render_data.append_2d_point(x, y)?;
                    }
                }
            }
        }

        // Did we get an image from the MP4 file?
        if let Some(pic) = &per_cam.image {
            cam_render_data.set_original_image(&pic.borrow())?;
        }

        cam_render_data.pts_chrono = per_cam.timestamp;

        cam_render_data
            .reprojected_points
            .extend(synced_data.project_kests(source, &synced_data.recon));

        if !wrote_debug {
            for output in output_storage.iter_mut() {
                if let OutputStorage::Debug(d) = output {
                    writeln!(
                        d.fd,
                        "   Collect {}: {} no points",
                        source.cam_id.best_name(),
                        per_cam.timestamp,
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
