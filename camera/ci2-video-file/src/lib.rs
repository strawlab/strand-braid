// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A [ci2] camera backend that plays back a pre-recorded video file (MP4,
//! FMF, MKV, ...) as if it were a live camera feed.
//!
//! This exists so Strand Camera's live acquisition/processing pipeline
//! (background subtraction, checkerboard calibration, ...) can be exercised
//! against real recorded footage with no camera hardware. Unlike the
//! `webcam` backend (`ci2-webcam`), it decodes the file directly via
//! [`frame_source`] -- there is no virtual V4L2/UVC device involved at all.
//!
//! The video file is named by the [`VIDEO_FILE_ENV`] environment variable
//! (mirroring `ci2-sim`'s `STRAND_CAM_SIM_SPEC`); `--camera-name` is a
//! caller-chosen label, not the path itself, so it stays safe to use in
//! `{CAMNAME}` filename templates and Braid camera identity. There is
//! exactly one "camera": the file named by that environment variable.
//!
//! # Playback
//!
//! The file plays back once and then loops from the beginning, indefinitely
//! -- there is no live-camera equivalent of "the movie ended". Decoding
//! happens on a dedicated background thread (see [`decode_loop`]), because
//! [`frame_source::FrameDataSource`]'s frame iterator borrows the source for
//! its own lifetime and so can't be stored alongside it as a struct field
//! across independent [`ci2::Camera::next_frame`] calls; the decoder thread
//! owns both locally and hands decoded frames across a small bounded
//! channel, giving the usual producer/consumer backpressure.
//!
//! # Pacing
//!
//! Frames are paced to the source's native frame rate -- from
//! [`frame_source::FrameDataSource::average_framerate`] when available (only
//! for a video recorded by strand-cam itself; see [`DecodedFrame`]'s docs),
//! else estimated from two real consecutive frames' own timestamps, else
//! [`DEFAULT_FPS`] -- using the same `Instant`-based sleep-until-target
//! idiom `ci2-sim` uses (with a resync guard so a delayed caller doesn't
//! trigger a burst of frames to "catch up"), and can be overridden the same
//! way (`acquisition_frame_rate_enable`/`set_acquisition_frame_rate`).
//!
//! # Pixel formats
//!
//! Frames are exposed in whatever pixel format [`frame_source`] decodes them
//! to (`RGB8` for MP4/H.264); no format conversion is offered, matching
//! `ci2-webcam`'s approach.

extern crate machine_vision_formats as formats;

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender};
use std::time::{Duration, Instant};

use ci2::{
    AcquisitionMode, AutoMode, DynamicFrameWithInfo, HostTimingInfo, TriggerMode, TriggerSelector,
};
use formats::PixFmt;
use strand_dynamic_frame::DynamicFrameOwned;

/// The environment variable naming the video file to play back.
pub const VIDEO_FILE_ENV: &str = "STRAND_CAM_VIDEO_FILE";

/// Frame rate used when the source has no known average frame rate (e.g. FMF).
const DEFAULT_FPS: f64 = 30.0;

/// How many frames the decoder thread may decode ahead of consumption. Small
/// on purpose: this backend's whole point is realistic live-feed pacing, not
/// decoding as fast as possible.
const DECODE_QUEUE_DEPTH: usize = 2;

/// How long [`WrappedCamera::next_frame`] (and camera open) wait for the
/// decoder thread before giving up. The decoder should never actually be
/// this slow; this only guards against a stuck/dead thread or a truly
/// pathological source file.
const DECODE_TIMEOUT: Duration = Duration::from_secs(30);

fn video_file_path() -> ci2::Result<PathBuf> {
    let path = std::env::var_os(VIDEO_FILE_ENV).ok_or_else(|| {
        ci2::Error::from(format!(
            "the video-file camera backend requires the {VIDEO_FILE_ENV} \
             environment variable to point at a video file"
        ))
    })?;
    let path = PathBuf::from(path);
    if !path.try_exists().unwrap_or(false) {
        return Err(ci2::Error::from(format!(
            "{VIDEO_FILE_ENV} names {path:?}, which does not exist"
        )));
    }
    Ok(path)
}

pub struct WrappedModule {}

pub fn new_module() -> ci2::Result<WrappedModule> {
    Ok(WrappedModule {})
}

/// The video-file backend keeps no global state that needs tearing down; the
/// guard is a no-op that exists to match the shape of the other ci2 backends.
pub struct VideoFileTerminateGuard {}

pub fn make_singleton_guard(
    _module: &dyn ci2::CameraModule<CameraType = WrappedCamera, Guard = VideoFileTerminateGuard>,
) -> ci2::Result<VideoFileTerminateGuard> {
    Ok(VideoFileTerminateGuard {})
}

impl<'a> ci2::CameraModule for &'a WrappedModule {
    type CameraType = WrappedCamera;
    type Guard = VideoFileTerminateGuard;

    fn name(self: &&'a WrappedModule) -> &'static str {
        "video-file"
    }

    fn camera_infos(self: &&'a WrappedModule) -> ci2::Result<Vec<Box<dyn ci2::CameraInfo>>> {
        let path = video_file_path()?;
        let info: Box<dyn ci2::CameraInfo> = Box::new(VideoFileCameraInfo::new(&path));
        Ok(vec![info])
    }

    fn camera(self: &mut &'a WrappedModule, name: &str) -> ci2::Result<Self::CameraType> {
        WrappedCamera::new(name)
    }

    fn settings_file_extension(&self) -> &str {
        // Video-file cameras have no node map, but a value is required by the trait.
        "toml"
    }
}

#[derive(Debug, Clone)]
struct VideoFileCameraInfo {
    name: String,
}

impl VideoFileCameraInfo {
    /// The camera name derived from the video file: its stem (e.g.
    /// `/path/to/checkerboard.mp4` -> `checkerboard`).
    fn default_name(path: &Path) -> String {
        path.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "video-file".to_string())
    }

    fn new(path: &Path) -> Self {
        Self {
            name: Self::default_name(path),
        }
    }
}

impl ci2::CameraInfo for VideoFileCameraInfo {
    fn name(&self) -> &str {
        &self.name
    }
    fn serial(&self) -> &str {
        "0"
    }
    fn model(&self) -> &str {
        "video-file"
    }
    fn vendor(&self) -> &str {
        "frame-source"
    }
}

/// One decoded frame from the source, plus its presentation timestamp
/// (elapsed time since the start of the file) when the source has one, and
/// the source's own average frame rate (re-reported on every frame for
/// simplicity; it is constant for a given file).
///
/// [`frame_source::FrameDataSource::average_framerate`] is populated only
/// from strand-cam-specific SEI timing metadata (see `h264_source.rs`'s
/// `calc_avg_fps`) -- reliable, and averaged across the *whole* file, for a
/// video recorded by strand-cam itself, but `None` for an ordinary MP4 from
/// anywhere else (e.g. this backend's own test footage). [`WrappedCamera::new`]
/// prefers it when available and falls back to timing two real consecutive
/// frames itself otherwise.
struct DecodedFrame {
    image: DynamicFrameOwned,
    pts: Option<Duration>,
    average_framerate: Option<f64>,
}

/// Decode `path` on a dedicated thread, looping back to the beginning
/// whenever the source is exhausted, sending each decoded frame to `tx`.
///
/// Runs until `tx.send` fails (the receiver -- and thus the [`WrappedCamera`]
/// -- has been dropped), at which point the thread exits.
fn decode_loop(path: PathBuf, tx: SyncSender<ci2::Result<DecodedFrame>>) {
    let mut first_loop = true;
    loop {
        let mut source = match frame_source::FrameSourceBuilder::new(&path).build_source() {
            Ok(source) => source,
            Err(e) => {
                let _ = tx.send(Err(ci2::Error::from(format!(
                    "opening video file {path:?}: {e}"
                ))));
                return;
            }
        };
        let average_framerate = source.average_framerate();
        let iter = match source.presentation_order_iter() {
            Ok(iter) => iter,
            Err(e) => {
                let _ = tx.send(Err(ci2::Error::from(format!(
                    "reading frame order from {path:?}: {e}"
                ))));
                return;
            }
        };

        if !first_loop {
            tracing::info!("video-file backend: reached end of {path:?}, looping playback");
        }
        first_loop = false;

        let mut sent_any = false;
        for frame in iter {
            let decoded = match frame {
                Ok(frame) => frame,
                Err(e) => {
                    if tx
                        .send(Err(ci2::Error::from(format!("decoding {path:?}: {e}"))))
                        .is_err()
                    {
                        return;
                    }
                    continue;
                }
            };
            let pts = match decoded.timestamp() {
                frame_source::Timestamp::Duration(d) => Some(d),
                frame_source::Timestamp::Fraction(_) => None,
            };
            let Some(image) = decoded.take_decoded() else {
                // Not a decoded pixel frame (e.g. raw undecoded H.264, which
                // would require the `openh264` feature) -- skip rather than
                // silently sending garbage.
                continue;
            };
            sent_any = true;
            if tx
                .send(Ok(DecodedFrame {
                    image,
                    pts,
                    average_framerate,
                }))
                .is_err()
            {
                return;
            }
        }
        if !sent_any {
            // The whole file produced no decodable frame; looping would spin
            // forever doing nothing, so report an error instead.
            let _ = tx.send(Err(ci2::Error::from(format!(
                "{path:?} produced no decodable video frames"
            ))));
            return;
        }
    }
}

pub struct WrappedCamera {
    info: VideoFileCameraInfo,
    rx: Receiver<ci2::Result<DecodedFrame>>,
    /// Frames already pulled off `rx` during [`WrappedCamera::new`] (to
    /// determine `width`/`height`/`pixel_format`/`fps` up front, mirroring
    /// how e.g. `ci2-webcam` negotiates format at open time), waiting to be
    /// returned by the first call(s) to `next_frame`.
    pending_frames: std::collections::VecDeque<DynamicFrameOwned>,
    width: u32,
    height: u32,
    pixel_format: PixFmt,
    /// Frame rate used to pace acquisition, determined at open time (see
    /// [`WrappedCamera::new`] for the priority order).
    fps: f64,
    frame_rate_enabled: bool,
    /// When acquisition started (for pacing); `None` until `acquisition_start`.
    start: Option<Instant>,
    /// Next frame number to emit.
    next_fno: usize,
}

fn _test_camera_is_send() {
    // Compile-time check: ci2-async drives the camera from a worker thread.
    fn implements<T: Send>() {}
    implements::<WrappedCamera>();
}

fn recv_decoded(rx: &Receiver<ci2::Result<DecodedFrame>>) -> ci2::Result<DecodedFrame> {
    match rx.recv_timeout(DECODE_TIMEOUT) {
        Ok(Ok(frame)) => Ok(frame),
        Ok(Err(e)) => Err(e),
        Err(RecvTimeoutError::Timeout) => Err(ci2::Error::Timeout),
        Err(RecvTimeoutError::Disconnected) => Err(ci2::Error::from(
            "video-file decoder thread exited unexpectedly",
        )),
    }
}

impl WrappedCamera {
    fn new(name: &str) -> ci2::Result<Self> {
        let path = video_file_path()?;

        let expected_name = VideoFileCameraInfo::default_name(&path);
        if name != expected_name {
            return Err(ci2::Error::from(format!(
                "unknown video-file camera \"{name}\"; expected \"{expected_name}\" \
                 (derived from {VIDEO_FILE_ENV}={path:?})"
            )));
        }

        let (tx, rx) = std::sync::mpsc::sync_channel(DECODE_QUEUE_DEPTH);
        let decode_path = path.clone();
        std::thread::Builder::new()
            .name("ci2-video-file-decode".to_string())
            .spawn(move || decode_loop(decode_path, tx))
            .map_err(|e| ci2::Error::BackendError(anyhow::Error::new(e)))?;

        // Block until the first frame decodes, to negotiate width/height/
        // pixel-format synchronously (as real camera backends do at open
        // time) and to fail loudly, right here, on a bad source file.
        let first = recv_decoded(&rx)?;
        let width = first.image.borrow().width();
        let height = first.image.borrow().height();
        let pixel_format = first.image.borrow().pixel_format();

        // Native frame rate, in priority order:
        // 1. `average_framerate()`, whole-file-averaged from SEI timing
        //    metadata -- reliable, but only present for a video recorded by
        //    strand-cam itself (see `DecodedFrame`'s docs).
        // 2. Otherwise, best-effort: the gap between two real consecutive
        //    frames' own presentation timestamps (assumes a roughly
        //    constant frame rate).
        // 3. Otherwise, `DEFAULT_FPS` (e.g. a single-frame file, or a raw
        //    stream with no reconstructible presentation order).
        let mut pending_frames = std::collections::VecDeque::with_capacity(2);
        let fps = if let Some(fps) = first.average_framerate {
            pending_frames.push_back(first.image);
            fps
        } else {
            let mut fps = DEFAULT_FPS;
            pending_frames.push_back(first.image);
            if let Some(pts0) = first.pts
                && let Ok(second) = recv_decoded(&rx)
            {
                if let Some(pts1) = second.pts {
                    let dt = pts1.saturating_sub(pts0).as_secs_f64();
                    if dt > 0.0 {
                        fps = 1.0 / dt;
                    }
                }
                pending_frames.push_back(second.image);
            }
            fps
        };

        Ok(Self {
            info: VideoFileCameraInfo::new(&path),
            width,
            height,
            pixel_format,
            fps,
            pending_frames,
            rx,
            frame_rate_enabled: true,
            start: None,
            next_fno: 0,
        })
    }

    /// Wall-clock interval between frames.
    fn frame_period(&self) -> Duration {
        Duration::from_secs_f64(1.0 / self.fps)
    }
}

impl ci2::CameraInfo for WrappedCamera {
    fn name(&self) -> &str {
        &self.info.name
    }
    fn serial(&self) -> &str {
        self.info.serial()
    }
    fn model(&self) -> &str {
        self.info.model()
    }
    fn vendor(&self) -> &str {
        self.info.vendor()
    }
}

impl ci2::Camera for WrappedCamera {
    // ----- Video files have no GenICam feature tree. -----
    fn command_execute(&self, _name: &str, _verify: bool) -> ci2::Result<()> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn feature_bool(&self, _name: &str) -> ci2::Result<bool> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn feature_bool_set(&self, _name: &str, _value: bool) -> ci2::Result<()> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn feature_enum(&self, _name: &str) -> ci2::Result<String> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn feature_enum_set(&self, _name: &str, _value: &str) -> ci2::Result<()> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn feature_float(&self, _name: &str) -> ci2::Result<f64> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn feature_float_set(&self, _name: &str, _value: f64) -> ci2::Result<()> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn feature_int(&self, _name: &str) -> ci2::Result<i64> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn feature_int_set(&self, _name: &str, _value: i64) -> ci2::Result<()> {
        Err(ci2::Error::FeatureNotPresent())
    }

    fn node_map_load(&self, _settings: &str) -> ci2::Result<()> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn node_map_save(&self) -> ci2::Result<String> {
        Err(ci2::Error::FeatureNotPresent())
    }

    fn width(&self) -> ci2::Result<u32> {
        Ok(self.width)
    }
    fn height(&self) -> ci2::Result<u32> {
        Ok(self.height)
    }

    fn pixel_format(&self) -> ci2::Result<PixFmt> {
        Ok(self.pixel_format)
    }
    fn possible_pixel_formats(&self) -> ci2::Result<Vec<PixFmt>> {
        Ok(vec![self.pixel_format])
    }
    fn set_pixel_format(&mut self, pixel_format: PixFmt) -> ci2::Result<()> {
        if pixel_format == self.pixel_format {
            Ok(())
        } else {
            Err(ci2::Error::from(format!(
                "video-file backend cannot change pixel format; the source decodes to {:?}",
                self.pixel_format
            )))
        }
    }

    fn exposure_time(&self) -> ci2::Result<f64> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn exposure_time_range(&self) -> ci2::Result<(f64, f64)> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn set_exposure_time(&mut self, _: f64) -> ci2::Result<()> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn exposure_auto(&self) -> ci2::Result<AutoMode> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn set_exposure_auto(&mut self, _: AutoMode) -> ci2::Result<()> {
        Err(ci2::Error::FeatureNotPresent())
    }

    fn gain(&self) -> ci2::Result<f64> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn gain_range(&self) -> ci2::Result<(f64, f64)> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn set_gain(&mut self, _: f64) -> ci2::Result<()> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn gain_auto(&self) -> ci2::Result<AutoMode> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn set_gain_auto(&mut self, _: AutoMode) -> ci2::Result<()> {
        Err(ci2::Error::FeatureNotPresent())
    }

    fn trigger_mode(&self) -> ci2::Result<TriggerMode> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn set_trigger_mode(&mut self, _: TriggerMode) -> ci2::Result<()> {
        Err(ci2::Error::FeatureNotPresent())
    }

    // Frame-rate control is supported: playback is paced to this rate, the
    // same way ci2-sim paces its synthetic frames.
    fn acquisition_frame_rate_enable(&self) -> ci2::Result<bool> {
        Ok(self.frame_rate_enabled)
    }
    fn set_acquisition_frame_rate_enable(&mut self, value: bool) -> ci2::Result<()> {
        self.frame_rate_enabled = value;
        Ok(())
    }
    fn acquisition_frame_rate(&self) -> ci2::Result<f64> {
        Ok(self.fps)
    }
    fn acquisition_frame_rate_range(&self) -> ci2::Result<(f64, f64)> {
        Ok((1.0, 1000.0))
    }
    fn set_acquisition_frame_rate(&mut self, value: f64) -> ci2::Result<()> {
        if value <= 0.0 {
            return Err(ci2::Error::from("frame rate must be positive"));
        }
        self.fps = value;
        Ok(())
    }

    fn trigger_selector(&self) -> ci2::Result<TriggerSelector> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn set_trigger_selector(&mut self, _: TriggerSelector) -> ci2::Result<()> {
        Err(ci2::Error::FeatureNotPresent())
    }

    fn acquisition_mode(&self) -> ci2::Result<AcquisitionMode> {
        Err(ci2::Error::FeatureNotPresent())
    }
    fn set_acquisition_mode(&mut self, _: AcquisitionMode) -> ci2::Result<()> {
        Err(ci2::Error::FeatureNotPresent())
    }

    fn acquisition_start(&mut self) -> ci2::Result<()> {
        self.next_fno = 0;
        self.start = Some(Instant::now());
        Ok(())
    }
    fn acquisition_stop(&mut self) -> ci2::Result<()> {
        self.start = None;
        Ok(())
    }

    fn next_frame(&mut self) -> ci2::Result<DynamicFrameWithInfo> {
        let fno = self.next_fno;
        self.next_fno += 1;

        let image = match self.pending_frames.pop_front() {
            Some(image) => image,
            None => recv_decoded(&self.rx)?.image,
        };

        // Pace to the frame rate (when enabled), the same Instant-based
        // sleep-until-target idiom ci2-sim uses.
        if let (Some(start), true) = (self.start, self.frame_rate_enabled) {
            let period = self.frame_period();
            let target = start + period * fno as u32;
            let now = Instant::now();
            if target > now {
                std::thread::sleep(target - now);
            } else if now.duration_since(target) > period {
                // Fallen behind by more than one frame period (e.g. this
                // call was delayed by a downstream stall) -- resync the
                // schedule to "now" rather than blasting through the whole
                // backlog to catch up, which would defeat pacing a "live"
                // feed at all. This mirrors how a real camera's small
                // hardware buffer would just drop frames it couldn't
                // deliver in time, rather than deliver them all at once
                // later.
                self.start = Some(now - period * fno as u32);
            }
        }

        Ok(DynamicFrameWithInfo {
            image: std::sync::Arc::new(image),
            host_timing: HostTimingInfo {
                fno,
                datetime: chrono::Utc::now(),
            },
            backend_data: None,
        })
    }
}
