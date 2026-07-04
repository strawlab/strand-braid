// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A [ci2] camera backend that renders synthetic images of simulated insects.
//!
//! This backend lets the *entire* live Braid pipeline (acquisition, background
//! subtraction, feature detection, UDP transport, the mainbrain, the Kalman
//! filter, data association, `.braidz` saving) run against a known ground truth
//! with no camera hardware. It is the image-level injection path of the
//! simulation test harness; see the `braid-sim` crate for the shared core.
//!
//! The scenario (a `sim.toml` parsed by [`braid_sim::Scenario`]) is provided via
//! the `STRAND_CAM_SIM_SPEC` environment variable; the camera to render is
//! selected by the `--camera-name` Strand Camera already receives (one of the
//! [`braid_sim::Scenario::camera_name`] values). Each camera independently
//! evaluates the deterministic world for its current frame and projects it with
//! its own calibration, so multiple sim cameras need no coordination.

extern crate machine_vision_formats as formats;

use std::time::{Duration, Instant};

use ci2::{
    AcquisitionMode, AutoMode, DynamicFrameWithInfo, HostTimingInfo, TriggerMode, TriggerSelector,
};
use flydra_mvg::FlydraMultiCameraSystem;
use formats::PixFmt;
use strand_dynamic_frame::DynamicFrameOwned;

use braid_sim::Scenario;
use braid_sim::scenario::BlobParams;
use braid_sim::world::World;

/// The environment variable naming the `sim.toml` scenario file.
pub const SIM_SPEC_ENV: &str = "STRAND_CAM_SIM_SPEC";

/// Pixel formats the sim backend can render. The single deterministic gray
/// scene is carried in each: RGB8 replicates the gray into three channels,
/// YUV422 carries it as luma with neutral chroma, and the Bayer variants label
/// the mono mosaic (1 byte/pixel) with the requested color-filter array. This
/// lets the color recording paths be exercised without camera hardware.
const SUPPORTED_PIXEL_FORMATS: [PixFmt; 7] = [
    PixFmt::Mono8,
    PixFmt::RGB8,
    PixFmt::YUV422,
    PixFmt::BayerRG8,
    PixFmt::BayerGR8,
    PixFmt::BayerGB8,
    PixFmt::BayerBG8,
];

/// Load the scenario named by [`SIM_SPEC_ENV`].
fn load_scenario() -> ci2::Result<Scenario> {
    let path = std::env::var_os(SIM_SPEC_ENV).ok_or_else(|| {
        ci2::Error::from(format!(
            "the sim camera backend requires the {SIM_SPEC_ENV} environment variable \
             to point at a sim.toml scenario file"
        ))
    })?;
    let text = std::fs::read_to_string(&path)
        .map_err(|e| ci2::Error::from(format!("reading {SIM_SPEC_ENV} ({path:?}): {e}")))?;
    Scenario::from_toml_str(&text)
        .map_err(|e| ci2::Error::from(format!("parsing {SIM_SPEC_ENV} ({path:?}): {e}")))
}

pub struct WrappedModule {}

pub fn new_module() -> ci2::Result<WrappedModule> {
    Ok(WrappedModule {})
}

/// The sim backend keeps no global state that needs tearing down; the guard is a
/// no-op that exists to match the shape of the other ci2 backends.
pub struct SimTerminateGuard {}

pub fn make_singleton_guard(
    _module: &dyn ci2::CameraModule<CameraType = WrappedCamera, Guard = SimTerminateGuard>,
) -> ci2::Result<SimTerminateGuard> {
    Ok(SimTerminateGuard {})
}

impl<'a> ci2::CameraModule for &'a WrappedModule {
    type CameraType = WrappedCamera;
    type Guard = SimTerminateGuard;

    fn name(self: &&'a WrappedModule) -> &'static str {
        "sim"
    }

    fn camera_infos(self: &&'a WrappedModule) -> ci2::Result<Vec<Box<dyn ci2::CameraInfo>>> {
        let scenario = load_scenario()?;
        let infos = (0..scenario.cameras.count)
            .map(|k| {
                let ci: Box<dyn ci2::CameraInfo> = Box::new(SimCameraInfo::new(k));
                ci
            })
            .collect();
        Ok(infos)
    }

    fn camera(self: &mut &'a WrappedModule, name: &str) -> ci2::Result<Self::CameraType> {
        WrappedCamera::new(name)
    }

    fn settings_file_extension(&self) -> &str {
        // Sim cameras have no node map, but a value is required by the trait.
        "toml"
    }
}

#[derive(Debug, Clone)]
struct SimCameraInfo {
    name: String,
    serial: String,
}

impl SimCameraInfo {
    fn new(k: usize) -> Self {
        Self {
            name: Scenario::camera_name(k),
            serial: format!("{k}"),
        }
    }
}

impl ci2::CameraInfo for SimCameraInfo {
    fn name(&self) -> &str {
        &self.name
    }
    fn serial(&self) -> &str {
        &self.serial
    }
    fn model(&self) -> &str {
        "sim"
    }
    fn vendor(&self) -> &str {
        "braid-sim"
    }
}

pub struct WrappedCamera {
    info: SimCameraInfo,
    /// This camera's name, used to project with its own calibration.
    cam_name: String,
    /// This camera's index (the `k` in `simcam{k}`), for timing perturbation.
    cam_index: usize,
    /// Scenario RNG seed, for deterministic timing jitter.
    seed: u64,
    /// Per-camera frame-arrival timing perturbation.
    timing: braid_sim::scenario::TimingModel,
    /// The full multi-camera calibration (this camera projects with its entry).
    system: FlydraMultiCameraSystem<f64>,
    /// The deterministic ground-truth world.
    world: World,
    image_width: usize,
    image_height: usize,
    blob: BlobParams,
    /// If set, report host timestamps as if frames arrived at this rate (instead
    /// of wall-clock `now()`), to reproduce the wrong-measured-fps bug. The
    /// reference instant for frame 0 is `start`.
    reported_fps: Option<f64>,
    /// Number of insect-free frames rendered first so the background model
    /// settles before insects appear.
    bg_warmup_frames: u32,
    /// Frame rate, frames per second. Used both to evaluate the world at the
    /// right logical time and to pace acquisition. Defaults to the scenario
    /// `fps`; Braid overrides it via the software frame-rate-limit path (under
    /// FakeSync it sends the scenario frame rate, so this is consistent).
    fps: f64,
    /// Whether acquisition is paced to `fps`. Braid's software frame-rate-limit
    /// enables this; it is on by default.
    frame_rate_enabled: bool,
    /// The pixel format frames are rendered in. Defaults to Mono8; can be set
    /// to RGB8 (e.g. via `--pixel-format RGB8`) to exercise the color
    /// recording path.
    pixel_format: PixFmt,
    /// When acquisition started (for pacing); `None` until `acquisition_start`.
    start: Option<Instant>,
    /// Wall-clock datetime captured at `acquisition_start`, used as the time of
    /// frame 0 when `reported_fps` synthesizes timestamps.
    start_datetime: Option<chrono::DateTime<chrono::Utc>>,
    /// Next frame number to emit.
    next_fno: usize,
}

fn _test_camera_is_send() {
    // Compile-time check: ci2-async drives the camera from a worker thread.
    fn implements<T: Send>() {}
    implements::<WrappedCamera>();
}

impl WrappedCamera {
    fn new(name: &str) -> ci2::Result<Self> {
        let scenario = load_scenario()?;

        // The requested name must be one of the scenario's cameras.
        let valid = (0..scenario.cameras.count).any(|k| Scenario::camera_name(k) == name);
        if !valid {
            return Err(ci2::Error::from(format!(
                "unknown sim camera \"{name}\"; expected one of simcam0..simcam{}",
                scenario.cameras.count.saturating_sub(1)
            )));
        }

        let system = braid_sim::calibration::build_calibration(&scenario)
            .map_err(|e| ci2::Error::from(format!("building sim calibration: {e}")))?;

        let cam_index = Scenario::camera_index(name).ok_or_else(|| {
            ci2::Error::from(format!("cannot parse camera index from \"{name}\""))
        })?;
        let info = SimCameraInfo {
            name: name.to_string(),
            serial: name.trim_start_matches("simcam").to_string(),
        };

        Ok(Self {
            info,
            cam_name: name.to_string(),
            cam_index,
            seed: scenario.seed,
            timing: scenario.timing.clone(),
            image_width: scenario.cameras.image_width,
            image_height: scenario.cameras.image_height,
            blob: scenario.blob.clone(),
            reported_fps: scenario.reported_fps,
            bg_warmup_frames: scenario.bg_warmup_frames,
            fps: scenario.fps,
            frame_rate_enabled: true,
            pixel_format: PixFmt::Mono8,
            start: None,
            start_datetime: None,
            next_fno: 0,
            world: World::new(scenario),
            system,
        })
    }

    /// Wall-clock interval between frames.
    fn frame_period(&self) -> Duration {
        Duration::from_secs_f64(1.0 / self.fps)
    }

    /// Pixel centers of all insects visible to this camera at frame `fno`,
    /// after applying the scenario's observation-model imperfections (detection
    /// noise, dropout, clutter). Empty during the background-warmup phase.
    fn blobs_for_frame(&self, fno: usize) -> Vec<(f64, f64)> {
        if (fno as u32) < self.bg_warmup_frames {
            return Vec::new();
        }
        // Logical world time: t = 0 at the first post-warmup frame.
        let t = (fno as u32 - self.bg_warmup_frames) as f64 / self.fps;
        let obs = &self.world.scenario().observation;
        let mut blobs: Vec<(f64, f64)> = self
            .world
            .state_at(t)
            .iter()
            .filter(|insect| !obs.is_suppressed(self.seed, self.cam_index, fno, insect.id))
            .filter_map(|insect| {
                braid_sim::projection::project_pixel(
                    &self.system,
                    &self.cam_name,
                    self.image_width,
                    self.image_height,
                    &insect.pos,
                )
                .map(|(x, y)| obs.jitter_pixel(self.seed, self.cam_index, fno, insect.id, x, y))
            })
            .collect();
        // Spurious clutter detections (false positives).
        blobs.extend(obs.clutter(
            self.seed,
            self.cam_index,
            fno,
            self.image_width,
            self.image_height,
        ));
        blobs
    }
}

impl ci2::CameraInfo for WrappedCamera {
    fn name(&self) -> &str {
        &self.info.name
    }
    fn serial(&self) -> &str {
        &self.info.serial
    }
    fn model(&self) -> &str {
        "sim"
    }
    fn vendor(&self) -> &str {
        "braid-sim"
    }
}

impl ci2::Camera for WrappedCamera {
    // ----- Sim cameras have no GenICam feature tree. -----
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
        Ok(self.image_width as u32)
    }
    fn height(&self) -> ci2::Result<u32> {
        Ok(self.image_height as u32)
    }

    fn pixel_format(&self) -> ci2::Result<PixFmt> {
        Ok(self.pixel_format)
    }
    fn possible_pixel_formats(&self) -> ci2::Result<Vec<PixFmt>> {
        Ok(SUPPORTED_PIXEL_FORMATS.to_vec())
    }
    fn set_pixel_format(&mut self, pixel_format: PixFmt) -> ci2::Result<()> {
        if SUPPORTED_PIXEL_FORMATS.contains(&pixel_format) {
            self.pixel_format = pixel_format;
            Ok(())
        } else {
            Err(ci2::Error::from(format!(
                "sim backend does not support pixel format {pixel_format}; \
                 supported: {SUPPORTED_PIXEL_FORMATS:?}"
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

    // Frame-rate control is supported: the sim camera paces its frames to this
    // rate. Under FakeSync, Braid drives this via the software frame-rate-limit
    // path, sending the scenario frame rate (consistent with the sim's own fps).
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
        self.start_datetime = Some(chrono::Utc::now());
        Ok(())
    }
    fn acquisition_stop(&mut self) -> ci2::Result<()> {
        self.start = None;
        Ok(())
    }

    fn next_frame(&mut self) -> ci2::Result<DynamicFrameWithInfo> {
        let fno = self.next_fno;
        self.next_fno += 1;

        // Pace to the frame rate (when enabled) so the pipeline runs at a
        // realistic rate. The optional per-camera timing perturbation
        // delays delivery of this frame so its 2D detections reach the mainbrain
        // late (and may be dropped from live bundling). Only delivery is delayed;
        // the frame content is unchanged.
        if let (Some(start), true) = (self.start, self.frame_rate_enabled) {
            let extra = self.timing.extra_delay_sec(self.seed, self.cam_index, fno);
            let target = start + self.frame_period() * fno as u32 + Duration::from_secs_f64(extra);
            let now = Instant::now();
            if target > now {
                std::thread::sleep(target - now);
            }
        }

        let blobs = self.blobs_for_frame(fno);
        // Render in the selected pixel format, all carrying the same gray scene
        // to exercise the various recording paths. RGB8 replicates the gray into
        // three channels; YUV422 carries it as luma with neutral chroma; the
        // Bayer variants use the mono mosaic (1 byte/pixel) labeled with the CFA.
        let bg = self.blob.background;
        let peak = self.blob.peak as f64;
        let sigma = self.blob.sigma;
        let (w, h) = (self.image_width, self.image_height);
        let (buf, stride) = match self.pixel_format {
            PixFmt::RGB8 => (
                braid_sim::render::render_rgb8(w, h, bg, &blobs, peak, sigma),
                w * 3,
            ),
            PixFmt::YUV422 => (
                braid_sim::render::render_yuv422_uyvy(w, h, bg, &blobs, peak, sigma),
                w * 2,
            ),
            // Mono8 and all Bayer variants are a single byte per pixel; the
            // Bayer mosaic is simply the mono scene labeled with a CFA.
            _ => (
                braid_sim::render::render_mono8(w, h, bg, &blobs, peak, sigma),
                w,
            ),
        };

        let image = DynamicFrameOwned::from_buf(
            self.image_width as u32,
            self.image_height as u32,
            stride,
            buf,
            self.pixel_format,
        )
        .ok_or_else(|| ci2::Error::SingleFrameError("sim frame had invalid layout".into()))?;

        // Host grab time. Normally the true wall-clock now(); with `reported_fps`
        // set, a synthetic time advancing at that rate, modeling a host clock
        // that is *bunched* relative to the true frame cadence (as happens under
        // load when the driver delivers buffered frames in bursts). The fps
        // estimator must not be fooled by this when a hardware timestamp exists.
        let datetime = match (self.reported_fps, self.start_datetime) {
            (Some(rfps), Some(base)) if rfps > 0.0 => {
                base + chrono::Duration::nanoseconds((fno as f64 / rfps * 1e9) as i64)
            }
            _ => chrono::Utc::now(),
        };

        // Hardware (device) timestamp at the TRUE frame cadence, in nanoseconds.
        // The sim emulates a camera that provides a reliable hardware clock; this
        // is what a correct fps estimator should use, and it is unaffected by the
        // `reported_fps` host-clock bunching above.
        let device_timestamp = (fno as f64 / self.fps * 1e9) as u64;
        let backend_data: Option<Box<dyn ci2::BackendData>> =
            Some(Box::new(ci2_pylon_types::PylonExtra {
                block_id: fno as u64,
                device_timestamp,
            }));

        Ok(DynamicFrameWithInfo {
            image: std::sync::Arc::new(image),
            host_timing: HostTimingInfo { fno, datetime },
            backend_data,
        })
    }
}
