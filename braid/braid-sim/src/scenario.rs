// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The `sim.toml` scenario schema: the single source of truth for a simulated
//! run (arena, cameras, insects, blob rendering, frame rate).

use serde::{Deserialize, Serialize};

/// Axis-aligned bounding box of the tracking volume, in meters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Arena {
    /// Minimum (x, y, z) corner, meters.
    pub min: [f64; 3],
    /// Maximum (x, y, z) corner, meters.
    pub max: [f64; 3],
}

impl Arena {
    /// Center of the arena, meters.
    pub fn center(&self) -> [f64; 3] {
        [
            0.5 * (self.min[0] + self.max[0]),
            0.5 * (self.min[1] + self.max[1]),
            0.5 * (self.min[2] + self.max[2]),
        ]
    }
    /// Half-extent of the arena along each axis, meters.
    pub fn half_extent(&self) -> [f64; 3] {
        [
            0.5 * (self.max[0] - self.min[0]),
            0.5 * (self.max[1] - self.min[1]),
            0.5 * (self.max[2] - self.min[2]),
        ]
    }
}

/// How the synthetic cameras are arranged: an evenly-spaced horizontal ring
/// around the arena center, all looking inward. Intrinsics are an ideal pinhole
/// (no distortion) for the perfect-world baseline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CameraRig {
    /// Number of cameras.
    pub count: usize,
    /// Radius of the camera ring around the arena center, meters.
    pub radius_m: f64,
    /// Height (world z) of the cameras, meters.
    pub height_m: f64,
    /// Focal length in pixels (fx == fy).
    pub focal_length_px: f64,
    /// Image width in pixels.
    pub image_width: usize,
    /// Image height in pixels.
    pub image_height: usize,
}

/// Parameters for rendering an insect as a Gaussian blob (used by the `ci2-sim`
/// backend in M2). Defaults follow the M0 detector-contract spike.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlobParams {
    /// Peak intensity added above the background (gray levels). Must clear the
    /// detector threshold (default `diff_threshold` is 30).
    pub peak: u8,
    /// Gaussian standard deviation, pixels.
    pub sigma: f64,
    /// Flat background gray level.
    pub background: u8,
}

impl Default for BlobParams {
    fn default() -> Self {
        // From the M0 spike: peak well above threshold, sigma ~1.5 localizes best.
        BlobParams {
            peak: 160,
            sigma: 1.5,
            background: 0,
        }
    }
}

/// A smooth, bounded, deterministic 3D motion: a per-axis sinusoid (Lissajous
/// figure) confined to a fraction of the arena half-extent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lissajous {
    /// Per-axis frequency, Hz.
    pub freq_hz: [f64; 3],
    /// Per-axis phase, radians.
    pub phase: [f64; 3],
    /// Fraction of the arena half-extent the motion spans (0..1).
    pub fill: f64,
    /// Amplitude (meters) of an additional high-frequency "maneuver" overlay
    /// per axis. Small amplitude at high [`Self::maneuver_freq_hz`] produces
    /// large acceleration/jerk (sharp turns) without large displacement,
    /// modeling a maneuvering target (e.g. a flying insect). A constant-velocity
    /// EKF cannot predict this, so it produces nonzero innovations — needed to
    /// exercise the over-confident-gate fragmentation bug. Default 0 (smooth).
    #[serde(default)]
    pub maneuver_amp_m: f64,
    /// Frequency (Hz) of the maneuver overlay. Default 0.
    #[serde(default)]
    pub maneuver_freq_hz: f64,
}

impl Default for Lissajous {
    fn default() -> Self {
        Lissajous {
            freq_hz: [0.11, 0.13, 0.07],
            phase: [0.0, 1.0, 2.0],
            fill: 0.7,
            maneuver_amp_m: 0.0,
            maneuver_freq_hz: 0.0,
        }
    }
}

/// One simulated insect.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InsectSpec {
    /// Ground-truth identity.
    pub id: u32,
    /// Time (seconds) at which the insect enters; absent before this.
    #[serde(default)]
    pub enter_t: f64,
    /// Time (seconds) at which the insect leaves; present forever if `None`.
    #[serde(default)]
    pub exit_t: Option<f64>,
    /// Motion model.
    #[serde(default)]
    pub motion: Lissajous,
}

fn default_bg_warmup_frames() -> u32 {
    // M0: establish the background on insect-free frames before insects enter.
    30
}

/// Per-camera frame-arrival timing perturbation (milestone M5).
///
/// The simulated cameras deliver each rendered frame late by this much, which
/// causes their 2D detections to reach the mainbrain after it may have advanced
/// past that frame. Late data is then silently dropped from the *live* 3D
/// bundling (see `braid/flydra2/src/frame_bundler.rs`) while still being saved
/// to disk, so retracking can recover it — the mechanism behind the
/// "live trajectories shorter than retrack" bug.
///
/// The default is no perturbation, so the perfect-world baseline is unchanged.
/// The frame *content* is unaffected: only delivery is delayed, so a frame still
/// depicts the same world time on every camera.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct TimingModel {
    /// Indices of the cameras whose frames are delivered late. Empty = none.
    #[serde(default)]
    pub lagging_cameras: Vec<usize>,
    /// Constant extra delivery latency for lagging cameras, in seconds.
    #[serde(default)]
    pub extra_latency_sec: f64,
    /// Maximum additional uniform-random per-frame latency for lagging cameras,
    /// in seconds (drawn deterministically from the scenario seed, so runs are
    /// reproducible).
    #[serde(default)]
    pub jitter_sec: f64,
}

impl TimingModel {
    /// The extra delivery delay (seconds) for camera index `cam_index` at frame
    /// `fno`, given the scenario `seed`. Deterministic.
    pub fn extra_delay_sec(&self, seed: u64, cam_index: usize, fno: usize) -> f64 {
        if !self.lagging_cameras.contains(&cam_index) {
            return 0.0;
        }
        let jitter = if self.jitter_sec > 0.0 {
            self.jitter_sec * unit_hash(seed, cam_index as u64, fno as u64)
        } else {
            0.0
        };
        self.extra_latency_sec + jitter
    }
}

/// Deterministic pseudo-random value in `[0, 1)` from three integers
/// (splitmix64-style mixing). Used for reproducible per-(camera, frame) jitter.
fn unit_hash(a: u64, b: u64, c: u64) -> f64 {
    let mut x = a
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(b.wrapping_mul(0xD1B5_4A32_D192_ED03))
        .wrapping_add(c.wrapping_mul(0xCA5A_8267_6BE1_1B27));
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    // Top 53 bits → f64 in [0, 1).
    (x >> 11) as f64 / (1u64 << 53) as f64
}

/// A complete simulated scenario, deserialized from `sim.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scenario {
    /// RNG seed (reserved for stochastic motion/noise added in later milestones).
    #[serde(default)]
    pub seed: u64,
    /// Synchronized frame rate, frames per second.
    pub fps: f64,
    /// Tracking volume.
    pub arena: Arena,
    /// Camera arrangement.
    pub cameras: CameraRig,
    /// The insects to simulate.
    pub insects: Vec<InsectSpec>,
    /// Blob rendering parameters.
    #[serde(default)]
    pub blob: BlobParams,
    /// Number of insect-free frames to render first so the background model
    /// settles before insects appear (see M0).
    #[serde(default = "default_bg_warmup_frames")]
    pub bg_warmup_frames: u32,
    /// Per-camera frame-arrival timing perturbation (default: none).
    #[serde(default)]
    pub timing: TimingModel,
    /// If set, the cameras' *host* timestamps advance at this rate (frames per
    /// second) instead of true wall-clock time, modeling a host clock that is
    /// **bunched** relative to the true frame cadence — as happens under load
    /// when the camera driver delivers buffered frames in bursts.
    ///
    /// The sim still emits a hardware (device) timestamp at the true cadence, so
    /// this exercises the frame-rate-estimation fix: a fps estimator that uses
    /// the host clock is fooled (reads `reported_fps`, corrupting the tracker's
    /// `dt = 1/fps` and fragmenting trajectories), while one that uses the
    /// hardware timestamp is correct. `None` reports true wall-clock host
    /// timestamps. See `scratch/strand-braid-suboptimalities.md`.
    #[serde(default)]
    pub reported_fps: Option<f64>,
}

impl Scenario {
    /// Parse a scenario from a `sim.toml` string.
    pub fn from_toml_str(s: &str) -> eyre::Result<Self> {
        Ok(toml::from_str(s)?)
    }

    /// The camera name for camera index `k`. Used as the calibration camera
    /// name, the Braid `[[cameras]]` name, and the `--camera-name` passed to the
    /// simulated `strand-cam`. Kept purely alphanumeric to avoid ROS-name
    /// encoding mismatches.
    pub fn camera_name(k: usize) -> String {
        format!("simcam{k}")
    }

    /// Parse the camera index `k` from a `simcam{k}` name.
    pub fn camera_index(name: &str) -> Option<usize> {
        name.strip_prefix("simcam").and_then(|s| s.parse().ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_name_index_roundtrip() {
        for k in 0..7 {
            assert_eq!(Scenario::camera_index(&Scenario::camera_name(k)), Some(k));
        }
        assert_eq!(Scenario::camera_index("not-a-sim-cam"), None);
    }

    #[test]
    fn timing_default_is_no_delay() {
        let t = TimingModel::default();
        for cam in 0..5 {
            for fno in 0..100 {
                assert_eq!(t.extra_delay_sec(1, cam, fno), 0.0);
            }
        }
    }

    #[test]
    fn timing_lags_only_selected_cameras() {
        let t = TimingModel {
            lagging_cameras: vec![2, 4],
            extra_latency_sec: 0.01,
            jitter_sec: 0.0,
        };
        assert_eq!(t.extra_delay_sec(1, 0, 5), 0.0);
        assert_eq!(t.extra_delay_sec(1, 2, 5), 0.01);
        assert_eq!(t.extra_delay_sec(1, 4, 5), 0.01);
    }

    #[test]
    fn timing_jitter_is_bounded_and_deterministic() {
        let t = TimingModel {
            lagging_cameras: vec![1],
            extra_latency_sec: 0.0,
            jitter_sec: 0.02,
        };
        for fno in 0..1000 {
            let d = t.extra_delay_sec(42, 1, fno);
            assert!((0.0..0.02).contains(&d), "jitter {d} out of range");
            // Deterministic: same inputs -> same output.
            assert_eq!(d, t.extra_delay_sec(42, 1, fno));
        }
    }
}
