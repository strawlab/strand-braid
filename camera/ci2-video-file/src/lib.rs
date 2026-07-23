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
//! By default the file plays back once and then loops from the beginning,
//! indefinitely -- there is no live-camera equivalent of "the movie ended".
//! Setting [`VIDEO_FILE_LOOP_ENV`] to `"false"` disables this, instead
//! playing through exactly once and holding on the last frame (see "Playing
//! once, then holding on the last frame" below). Decoding happens on a
//! dedicated background thread (see [`decode_loop`]), because
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
//! way (`acquisition_frame_rate_enable`/`set_acquisition_frame_rate`), or by
//! setting [`VIDEO_FILE_LIMIT_FRAMERATE_ENV`] at open time (e.g. because a
//! downstream processing pipeline -- checkerboard detection, ... -- can't
//! keep up with the source's native rate on a given machine). Either way
//! this only changes how fast frames are served: every decoded frame is
//! still served, in the same order, so a lower rate plays back in slow
//! motion rather than skipping frames, and holding on the first/last frame
//! (see below) is unaffected.
//!
//! # Pixel formats
//!
//! Frames are exposed in whatever pixel format [`frame_source`] decodes them
//! to (`RGB8` for MP4/H.264); no format conversion is offered, matching
//! `ci2-webcam`'s approach.
//!
//! # Holding on the first frame
//!
//! By default (matching every prior release of this backend) playback begins
//! the instant the camera opens. Setting [`VIDEO_FILE_AUTOSTART_ENV`] to
//! `"false"` instead holds on the very first decoded frame -- repeated
//! indefinitely, still paced to the normal frame rate (so a caller
//! configuring settings during a hold doesn't spin the downstream
//! processing pipeline as fast as it possibly can on a static image) --
//! until a `"StartPlayback"` [`ci2::Camera::command_execute`] command
//! arrives (reachable from outside the process via
//! `CamArg::ExecuteCommand("StartPlayback".into())`, e.g. a plain `POST
//! /callback` to strand-cam's BUI). Real playback then begins from that
//! moment, with pacing reset to start fresh rather than resuming from
//! whenever the camera was originally opened. This exists so a caller (e.g.
//! a recording/test harness) can finish configuring everything else before
//! letting the source video actually start moving.
//!
//! # Playing once, then holding on the last frame
//!
//! Setting [`VIDEO_FILE_LOOP_ENV`] to `"false"` disables the default
//! loop-forever behavior: the file plays through exactly once, then
//! `next_frame` freezes on the last decoded frame -- still paced to the
//! normal frame rate, same as the pre-start hold above, rather than a busy
//! loop -- instead of restarting from the beginning. The decoder thread
//! logs "reached end of ..., holding on last frame" (as opposed to "...
//! looping playback") the moment this happens, so an external watcher (e.g.
//! a recording harness polling the terminal log) can tell when the source
//! has genuinely finished, rather than guessing from the file's own known
//! duration.
//!
//! # Signaling end of playback
//!
//! The log line above works for a human watching the terminal, but is
//! unreliable for an automated caller polling a *rendered* terminal (e.g. a
//! recording harness driving a browser-bridged terminal via `ttyd`/CDP):
//! most terminal front-ends, including `ttyd`'s DOM renderer, only ever
//! materialize the currently visible viewport as DOM nodes, so a busy log
//! (e.g. checkerboard corner-detection running every ~500ms) can scroll the
//! line out of view -- and thus permanently out of reach of a DOM query --
//! well before a poll happens to check for it. Setting
//! [`VIDEO_FILE_DONE_MARKER_ENV`] to a file path has `decode_loop` create
//! that file (empty contents) at the exact same moment it logs "holding on
//! last frame". A plain filesystem existence check is immune to the
//! scrolling problem above, since a file's existence doesn't scroll away.
//! Unset by default (no marker file is written unless a caller opts in);
//! only meaningful together with [`VIDEO_FILE_LOOP_ENV`]`=false`, since
//! looping playback never reaches "the end" at all.

extern crate machine_vision_formats as formats;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender};
use std::time::{Duration, Instant};

use ci2::{
    AcquisitionMode, AutoMode, DynamicFrameWithInfo, HostTimingInfo, TriggerMode, TriggerSelector,
};
use formats::PixFmt;
use strand_dynamic_frame::DynamicFrameOwned;

/// The environment variable naming the video file to play back.
pub const VIDEO_FILE_ENV: &str = "STRAND_CAM_VIDEO_FILE";

/// The environment variable controlling whether playback begins immediately
/// (`"true"`/unset, the default -- unchanged from every prior release) or
/// holds on the first frame until a `"StartPlayback"` `command_execute` call
/// arrives (`"false"`). See the module-level "Holding on the first frame"
/// docs above.
pub const VIDEO_FILE_AUTOSTART_ENV: &str = "STRAND_CAM_VIDEO_FILE_AUTOSTART";

/// The [`ci2::Camera::command_execute`] command name that ends the hold
/// started by [`VIDEO_FILE_AUTOSTART_ENV`]`=false` and begins real playback.
pub const START_PLAYBACK_COMMAND: &str = "StartPlayback";

/// The environment variable controlling whether the file loops forever
/// (`"true"`/unset, the default -- unchanged from every prior release) or
/// plays through exactly once and then holds on its last frame (`"false"`).
/// See the module-level "Playing once, then holding on the last frame"
/// docs above.
pub const VIDEO_FILE_LOOP_ENV: &str = "STRAND_CAM_VIDEO_FILE_LOOP";

/// The environment variable naming a marker file to create the moment
/// playback reaches the end of the source (only reachable when
/// [`VIDEO_FILE_LOOP_ENV`]`=false`). Unset by default -- no marker file is
/// written unless a caller opts in. See the module-level "Signaling end of
/// playback" docs above.
pub const VIDEO_FILE_DONE_MARKER_ENV: &str = "STRAND_CAM_VIDEO_FILE_DONE_MARKER";

/// The environment variable overriding playback pacing to a fixed frame
/// rate, instead of the source's native rate (see the module-level
/// "Pacing" docs). Unset, or set to the literal string `"None"` -- both the
/// default -- keep native-rate pacing exactly as before. Set to a positive
/// number (e.g. `"5"`) to pace at that fixed rate instead: every decoded
/// frame is still served, in the same order, so a lower value plays back in
/// slow motion rather than skipping frames.
pub const VIDEO_FILE_LIMIT_FRAMERATE_ENV: &str = "STRAND_CAM_VIDEO_FILE_LIMIT_FRAMERATE";

/// Parses [`VIDEO_FILE_LIMIT_FRAMERATE_ENV`]. Unset or `"None"` -> `Ok(None)`
/// (no override). A positive number -> `Ok(Some(fps))`. Anything else is a
/// clear error -- fail loudly on a typo rather than silently keeping
/// native-rate pacing, since a caller who set this expects it to take
/// effect.
fn limit_framerate_override() -> ci2::Result<Option<f64>> {
    let Some(value) = std::env::var_os(VIDEO_FILE_LIMIT_FRAMERATE_ENV) else {
        return Ok(None);
    };
    let value = value.to_string_lossy();
    if value == "None" {
        return Ok(None);
    }
    let fps: f64 = value.parse().map_err(|_| {
        ci2::Error::from(format!(
            "{VIDEO_FILE_LIMIT_FRAMERATE_ENV}={value:?} is not a number or \"None\""
        ))
    })?;
    if fps.is_nan() || fps <= 0.0 {
        return Err(ci2::Error::from(format!(
            "{VIDEO_FILE_LIMIT_FRAMERATE_ENV}={value:?} must be a positive number"
        )));
    }
    Ok(Some(fps))
}

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

/// A message from [`decode_loop`] to [`WrappedCamera`]: either a real
/// decoded frame, or (only when `decode_loop` was told not to loop) a
/// one-time marker that the file has been played through exactly once and
/// no more frames are coming.
enum DecodeMsg {
    Frame(DecodedFrame),
    /// Sent exactly once, as the last message before the decoder thread
    /// exits, when `decode_loop` was run with `loop_playback: false`.
    EndOfStream,
}

/// Decode `path` on a dedicated thread, sending each decoded frame to `tx`.
///
/// When `loop_playback` is true (the default -- see [`VIDEO_FILE_LOOP_ENV`]),
/// loops back to the beginning whenever the source is exhausted, indefinitely.
/// When false, plays the file through exactly once, sends one
/// [`DecodeMsg::EndOfStream`], creates `done_marker` (if given -- see
/// [`VIDEO_FILE_DONE_MARKER_ENV`]), and returns.
///
/// Otherwise runs until `tx.send` fails (the receiver -- and thus the
/// [`WrappedCamera`] -- has been dropped), at which point the thread exits.
fn decode_loop(
    path: PathBuf,
    tx: SyncSender<ci2::Result<DecodeMsg>>,
    loop_playback: bool,
    done_marker: Option<PathBuf>,
) {
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
                .send(Ok(DecodeMsg::Frame(DecodedFrame {
                    image,
                    pts,
                    average_framerate,
                })))
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
        if !loop_playback {
            tracing::info!("video-file backend: reached end of {path:?}, holding on last frame");
            if let Some(marker) = &done_marker {
                // Best-effort: a failure here shouldn't take down playback,
                // just leave whoever's waiting on the marker to time out and
                // report it themselves.
                if let Err(e) = std::fs::write(marker, "") {
                    tracing::warn!(
                        "video-file backend: failed to write done-marker {marker:?}: {e}"
                    );
                }
            }
            let _ = tx.send(Ok(DecodeMsg::EndOfStream));
            return;
        }
    }
}

pub struct WrappedCamera {
    info: VideoFileCameraInfo,
    rx: Receiver<ci2::Result<DecodeMsg>>,
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
    /// Whether real playback has been allowed to begin -- always `true`
    /// unless `VIDEO_FILE_AUTOSTART_ENV=false`, in which case `next_frame`
    /// holds on `held_frame` until a `"StartPlayback"` `command_execute`
    /// call (see `command_execute` below) flips this. `Arc` since
    /// `command_execute` only gets `&self`.
    started_manually: Arc<AtomicBool>,
    /// Whether `next_frame` has already reset `start`/`next_fno` for the
    /// held -> started transition. Initialized equal to the initial value
    /// of `started_manually`, so the default (autostart) path never enters
    /// that branch at all -- zero behavior change when
    /// `VIDEO_FILE_AUTOSTART_ENV` is unset.
    has_reset_pacing: bool,
    /// A clone of the very first decoded frame, repeatedly served (never
    /// consumed) while held before playback starts.
    held_frame: Arc<DynamicFrameOwned>,
    /// The most recently served real (decoded, non-held) frame -- kept
    /// up to date on every real frame so it's ready to freeze on once
    /// `DecodeMsg::EndOfStream` arrives (only possible when
    /// `VIDEO_FILE_LOOP_ENV=false`; see `next_frame`).
    last_real_frame: Option<Arc<DynamicFrameOwned>>,
    /// Whether the source has finished playing through once and
    /// `decode_loop` sent `DecodeMsg::EndOfStream` -- only ever becomes
    /// true when not looping. Once true, `next_frame` freezes on
    /// `last_real_frame` instead of reading `rx`/`pending_frames` again.
    ended: bool,
}

fn _test_camera_is_send() {
    // Compile-time check: ci2-async drives the camera from a worker thread.
    fn implements<T: Send>() {}
    implements::<WrappedCamera>();
}

fn recv_decoded(rx: &Receiver<ci2::Result<DecodeMsg>>) -> ci2::Result<DecodeMsg> {
    match rx.recv_timeout(DECODE_TIMEOUT) {
        Ok(Ok(msg)) => Ok(msg),
        Ok(Err(e)) => Err(e),
        Err(RecvTimeoutError::Timeout) => Err(ci2::Error::Timeout),
        Err(RecvTimeoutError::Disconnected) => Err(ci2::Error::from(
            "video-file decoder thread exited unexpectedly",
        )),
    }
}

/// Like [`recv_decoded`], but for call sites (only [`WrappedCamera::new`])
/// that expect a real frame and treat an immediate `EndOfStream` (a source
/// with fewer frames than the caller needed) as an error.
fn recv_decoded_frame(rx: &Receiver<ci2::Result<DecodeMsg>>) -> ci2::Result<DecodedFrame> {
    match recv_decoded(rx)? {
        DecodeMsg::Frame(frame) => Ok(frame),
        DecodeMsg::EndOfStream => Err(ci2::Error::from(
            "video file ended before producing the expected frame",
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

        let loop_playback = std::env::var_os(VIDEO_FILE_LOOP_ENV)
            .map(|v| v != "false")
            .unwrap_or(true);
        let done_marker = std::env::var_os(VIDEO_FILE_DONE_MARKER_ENV).map(PathBuf::from);

        let (tx, rx) = std::sync::mpsc::sync_channel(DECODE_QUEUE_DEPTH);
        let decode_path = path.clone();
        std::thread::Builder::new()
            .name("ci2-video-file-decode".to_string())
            .spawn(move || decode_loop(decode_path, tx, loop_playback, done_marker))
            .map_err(|e| ci2::Error::BackendError(anyhow::Error::new(e)))?;

        // Block until the first frame decodes, to negotiate width/height/
        // pixel-format synchronously (as real camera backends do at open
        // time) and to fail loudly, right here, on a bad source file.
        let first = recv_decoded_frame(&rx)?;
        let width = first.image.borrow().width();
        let height = first.image.borrow().height();
        let pixel_format = first.image.borrow().pixel_format();
        // Cloned before `first.image` is moved into `pending_frames` below --
        // this is the frame `next_frame` repeatedly serves while held (see
        // the module-level "Holding on the first frame" docs), kept
        // completely separate from the real playback queue.
        let held_frame = Arc::new(first.image.clone());

        let autostart = std::env::var_os(VIDEO_FILE_AUTOSTART_ENV)
            .map(|v| v != "false")
            .unwrap_or(true);

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
                && let Ok(DecodeMsg::Frame(second)) = recv_decoded(&rx)
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

        // Optional override: pace at a fixed rate instead of the native one
        // just computed above, e.g. because a downstream processing
        // pipeline can't keep up with the source's native rate on this
        // machine. Doesn't change which frames are decoded/held/looped --
        // only how fast `next_frame` serves them (see the module-level
        // "Pacing" docs).
        let fps = match limit_framerate_override()? {
            Some(limited) => {
                tracing::info!(
                    "video-file backend: pacing playback at {limited} fps \
                     ({VIDEO_FILE_LIMIT_FRAMERATE_ENV}) instead of the source's native {fps} fps"
                );
                limited
            }
            None => fps,
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
            started_manually: Arc::new(AtomicBool::new(autostart)),
            has_reset_pacing: autostart,
            last_real_frame: Some(held_frame.clone()),
            held_frame,
            ended: false,
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
    // ----- Video files have no GenICam feature tree, except one command. -----
    fn command_execute(&self, name: &str, _verify: bool) -> ci2::Result<()> {
        match name {
            START_PLAYBACK_COMMAND => {
                // See the module-level "Holding on the first frame" docs and
                // `next_frame` below. A no-op (not an error) if playback was
                // never held in the first place.
                self.started_manually.store(true, Ordering::SeqCst);
                Ok(())
            }
            _ => Err(ci2::Error::FeatureNotPresent()),
        }
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
        let held = !self.started_manually.load(Ordering::SeqCst);
        if !held && !self.has_reset_pacing {
            // Just transitioned held -> started: begin pacing from *now*,
            // not from whenever acquisition_start() originally fired (which,
            // if we were held, may have been long before this moment).
            self.start = Some(Instant::now());
            self.next_fno = 0;
            self.has_reset_pacing = true;
        }

        let fno = self.next_fno;
        self.next_fno += 1;

        // Held (pre-start): repeatedly serve the same clone of the first
        // frame (see the module-level "Holding on the first frame" docs),
        // never touching `pending_frames`/`rx` -- the real playback queue
        // sits untouched behind this until `command_execute("StartPlayback")`
        // flips the flag. Ended (post-completion, only possible when
        // VIDEO_FILE_LOOP_ENV=false): repeatedly serve the last real frame
        // instead of erroring on the now-disconnected decode channel (see
        // the module-level "Playing once, then holding on the last frame"
        // docs). Otherwise, pull the next real frame as usual, tracking it
        // as `last_real_frame` in case the very next call turns out to be
        // the last one.
        let image: std::sync::Arc<DynamicFrameOwned> = if held {
            self.held_frame.clone()
        } else if self.ended {
            self.last_real_frame
                .clone()
                .expect("a real frame is always served before `ended` can become true")
        } else {
            let decoded = match self.pending_frames.pop_front() {
                Some(image) => Some(image),
                None => match recv_decoded(&self.rx)? {
                    DecodeMsg::Frame(frame) => Some(frame.image),
                    DecodeMsg::EndOfStream => None,
                },
            };
            match decoded {
                Some(decoded) => {
                    let image = std::sync::Arc::new(decoded);
                    self.last_real_frame = Some(image.clone());
                    image
                }
                None => {
                    self.ended = true;
                    self.last_real_frame
                        .clone()
                        .expect("a real frame is always served before EndOfStream can arrive")
                }
            }
        };

        // Pace to the frame rate (when enabled), the same Instant-based
        // sleep-until-target idiom ci2-sim uses -- applied while held too
        // (repeating the same image), so a caller configuring settings
        // during a hold doesn't spin the downstream pipeline (encoding,
        // detection, ...) as fast as it possibly can on a static frame.
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
            image,
            host_timing: HostTimingInfo {
                fno,
                datetime: chrono::Utc::now(),
            },
            backend_data: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A single test, not several, deliberately: `limit_framerate_override`
    // reads real process-global environment state, and running independent
    // #[test] fns that each set/unset the same variable could race under
    // cargo's default parallel test execution. One sequential test avoids
    // that entirely.
    #[test]
    fn limit_framerate_override_parses_expected_values() {
        // SAFETY: this test does not run any other threads/tests
        // concurrently with these env var mutations (see the comment
        // above) -- the sequential ordering here is what makes it sound.
        unsafe {
            std::env::remove_var(VIDEO_FILE_LIMIT_FRAMERATE_ENV);
        }
        assert_eq!(limit_framerate_override().unwrap(), None);

        unsafe {
            std::env::set_var(VIDEO_FILE_LIMIT_FRAMERATE_ENV, "None");
        }
        assert_eq!(limit_framerate_override().unwrap(), None);

        unsafe {
            std::env::set_var(VIDEO_FILE_LIMIT_FRAMERATE_ENV, "5");
        }
        assert_eq!(limit_framerate_override().unwrap(), Some(5.0));

        unsafe {
            std::env::set_var(VIDEO_FILE_LIMIT_FRAMERATE_ENV, "2.5");
        }
        assert_eq!(limit_framerate_override().unwrap(), Some(2.5));

        unsafe {
            std::env::set_var(VIDEO_FILE_LIMIT_FRAMERATE_ENV, "0");
        }
        assert!(limit_framerate_override().is_err());

        unsafe {
            std::env::set_var(VIDEO_FILE_LIMIT_FRAMERATE_ENV, "-5");
        }
        assert!(limit_framerate_override().is_err());

        unsafe {
            std::env::set_var(VIDEO_FILE_LIMIT_FRAMERATE_ENV, "not-a-number");
        }
        assert!(limit_framerate_override().is_err());

        unsafe {
            std::env::remove_var(VIDEO_FILE_LIMIT_FRAMERATE_ENV);
        }
    }
}
