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
}

impl Default for Lissajous {
    fn default() -> Self {
        Lissajous {
            freq_hz: [0.11, 0.13, 0.07],
            phase: [0.0, 1.0, 2.0],
            fill: 0.7,
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
}
