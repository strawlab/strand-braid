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

/// Observation-model imperfections applied to the 2D detections (milestone M3 /
/// plan §3.3). All default to zero, so the perfect-world baseline is unchanged.
///
/// Everything is sampled deterministically from the scenario `seed` plus the
/// `(camera, frame, insect)` indices, so a `(config, seed)` reproduces a run
/// exactly — the harness can print the seed on failure and replay it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ObservationModel {
    /// Standard deviation (pixels) of zero-mean Gaussian jitter added to each
    /// projected detection. Models finite localization accuracy. Default 0.
    #[serde(default)]
    pub pixel_noise_px: f64,
    /// Per-(camera, insect, frame) probability in `[0, 1]` that a real detection
    /// is missed (dropped), i.i.d. per frame. Models sporadic detector misses;
    /// for sustained misses use [`Self::occlusion`] instead. Default 0.
    #[serde(default)]
    pub dropout_prob: f64,
    /// Expected number of spurious "clutter" detections per camera per frame
    /// (false positives, placed uniformly in the image). Stresses data
    /// association. Modeled as a fixed count `floor(x)` plus a fractional part
    /// included with probability `x - floor(x)`. Default 0.
    #[serde(default)]
    pub clutter_per_frame: f64,
    /// Temporally-correlated occlusion: hides an insect from a camera for whole
    /// spans of frames (default: never). See [`OcclusionModel`].
    #[serde(default)]
    pub occlusion: OcclusionModel,
}

impl ObservationModel {
    /// Whether a real detection of `insect_id` on camera `cam_index` at frame
    /// `fno` is dropped, given the scenario `seed`. Deterministic.
    pub fn is_dropped(&self, seed: u64, cam_index: usize, fno: usize, insect_id: u32) -> bool {
        if self.dropout_prob <= 0.0 {
            return false;
        }
        let u = unit_hash(
            seed ^ 0x44_4f_55_54, // "DOUT"
            (cam_index as u64) << 32 | insect_id as u64,
            fno as u64,
        );
        u < self.dropout_prob
    }

    /// Whether a real detection of `insect_id` on camera `cam_index` at frame
    /// `fno` should be suppressed for *any* reason — an i.i.d. [`Self::is_dropped`]
    /// miss or an [`OcclusionModel`] span. This is the single check both the
    /// image backend and the in-process injector apply before emitting a point.
    pub fn is_suppressed(&self, seed: u64, cam_index: usize, fno: usize, insect_id: u32) -> bool {
        self.is_dropped(seed, cam_index, fno, insect_id)
            || self.occlusion.is_occluded(seed, cam_index, fno, insect_id)
    }

    /// The projected pixel `(x, y)` with deterministic Gaussian jitter applied.
    /// Returns the input unchanged when `pixel_noise_px == 0`.
    pub fn jitter_pixel(
        &self,
        seed: u64,
        cam_index: usize,
        fno: usize,
        insect_id: u32,
        x: f64,
        y: f64,
    ) -> (f64, f64) {
        if self.pixel_noise_px <= 0.0 {
            return (x, y);
        }
        let key = (cam_index as u64) << 32 | insect_id as u64;
        // Two independent uniforms -> a 2D Gaussian via Box-Muller.
        let u1 = unit_hash(seed ^ 0x4e_4f_49_53, key, fno as u64).max(1e-12); // "NOIS"
        let u2 = unit_hash(seed ^ 0x4a_49_54_52, key, fno as u64); // "JITR"
        let r = self.pixel_noise_px * (-2.0 * u1.ln()).sqrt();
        let theta = 2.0 * std::f64::consts::PI * u2;
        (x + r * theta.cos(), y + r * theta.sin())
    }

    /// Spurious clutter detections for camera `cam_index` at frame `fno`, placed
    /// uniformly within a `width` x `height` image. Deterministic.
    pub fn clutter(
        &self,
        seed: u64,
        cam_index: usize,
        fno: usize,
        width: usize,
        height: usize,
    ) -> Vec<(f64, f64)> {
        if self.clutter_per_frame <= 0.0 {
            return Vec::new();
        }
        let whole = self.clutter_per_frame.floor() as usize;
        let frac = self.clutter_per_frame - whole as f64;
        let base = seed ^ 0x43_4c_54_52; // "CLTR"
        let mut count = whole;
        if frac > 0.0 {
            let u = unit_hash(base, (cam_index as u64) << 40, fno as u64);
            if u < frac {
                count += 1;
            }
        }
        (0..count)
            .map(|i| {
                let kx = (cam_index as u64) << 40 | (i as u64) << 1;
                let ky = kx | 1;
                let ux = unit_hash(base, kx, fno as u64);
                let uy = unit_hash(base, ky, fno as u64);
                (ux * width as f64, uy * height as f64)
            })
            .collect()
    }
}

/// Temporally-correlated occlusion (plan §3.3): an insect is hidden from a
/// camera for contiguous *spans* of frames — modeling it passing behind another
/// insect or an arena feature.
///
/// This differs from [`ObservationModel::dropout_prob`], which drops detections
/// i.i.d. per frame: independent single-frame misses rarely line up into a long
/// gap, whereas occlusion suppresses a whole span at once. Those multi-frame,
/// few-or-zero-observation stretches are what fragment *live* tracks (the live
/// EKF kills a coasting track that retrack, seeing all data at once, bridges) —
/// the mechanism flagged in the M6 shortened-trajectory investigation.
///
/// Time is tiled into blocks of `span_frames`; each (camera, insect, block) is
/// independently occluded with probability `prob`. Adjacent occluded blocks
/// merge, so spans are at least one block and occasionally longer. Default
/// (`prob == 0` or `span_frames == 0`) is never occluded, preserving the
/// perfect-world baseline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct OcclusionModel {
    /// Probability in `[0, 1]` that any given block hides the insect from a
    /// camera. Default 0 (never occluded).
    #[serde(default)]
    pub prob: f64,
    /// Block length in frames (the occlusion granularity / typical span).
    /// Default 0 disables occlusion regardless of `prob`.
    #[serde(default)]
    pub span_frames: usize,
}

impl OcclusionModel {
    /// Whether `insect_id` is occluded from camera `cam_index` at frame `fno`,
    /// given the scenario `seed`. Deterministic and constant within a block, so
    /// a `(config, seed)` reproduces the exact occluded spans.
    pub fn is_occluded(&self, seed: u64, cam_index: usize, fno: usize, insect_id: u32) -> bool {
        if self.prob <= 0.0 || self.span_frames == 0 {
            return false;
        }
        let block = (fno / self.span_frames) as u64;
        let u = unit_hash(
            seed ^ 0x4f_43_43_4c, // "OCCL"
            (cam_index as u64) << 32 | insect_id as u64,
            block,
        );
        u < self.prob
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
    /// Observation-model imperfections: detection noise, dropout, clutter
    /// (default: none).
    #[serde(default)]
    pub observation: ObservationModel,
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
    fn imperfect_example_parses_with_occlusion() {
        let s = Scenario::from_toml_str(include_str!("../example-sim-imperfect.toml")).unwrap();
        // The occlusion knob is wired through deserialization and active.
        assert!(s.observation.occlusion.prob > 0.0);
        assert!(s.observation.occlusion.span_frames > 0);
        // It actually occludes some (camera, frame) and `is_suppressed` reflects
        // it, but it is mild enough that the insect is rarely hidden everywhere.
        let occluded = (0..2000).any(|f| s.observation.is_suppressed(s.seed, 0, f, 1));
        assert!(
            occluded,
            "expected the imperfect example to occlude sometimes"
        );
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

    #[test]
    fn observation_default_is_a_no_op() {
        let o = ObservationModel::default();
        for fno in 0..50 {
            assert!(!o.is_dropped(7, 0, fno, 3));
            assert!(!o.is_suppressed(7, 0, fno, 3));
            assert!(!o.occlusion.is_occluded(7, 0, fno, 3));
            assert_eq!(o.jitter_pixel(7, 0, fno, 3, 100.0, 50.0), (100.0, 50.0));
            assert!(o.clutter(7, 0, fno, 640, 480).is_empty());
        }
    }

    #[test]
    fn occlusion_default_and_disabled_never_occludes() {
        // Default, prob-without-span, and span-without-prob all disable it.
        for o in [
            OcclusionModel::default(),
            OcclusionModel {
                prob: 0.5,
                span_frames: 0,
            },
            OcclusionModel {
                prob: 0.0,
                span_frames: 30,
            },
        ] {
            assert!((0..200).all(|fno| !o.is_occluded(1, 0, fno, 3)));
        }
    }

    #[test]
    fn occlusion_is_blockwise_constant_and_deterministic() {
        let span = 25usize;
        let o = OcclusionModel {
            prob: 0.4,
            span_frames: span,
        };
        // Constant within each block; deterministic across calls.
        for block in 0..40usize {
            let first = o.is_occluded(7, 2, block * span, 1);
            for off in 0..span {
                let fno = block * span + off;
                assert_eq!(o.is_occluded(7, 2, fno, 1), first, "frame {fno} in block");
            }
        }
    }

    #[test]
    fn occlusion_rate_matches_prob_and_creates_spans() {
        let span = 20usize;
        let o = OcclusionModel {
            prob: 0.3,
            span_frames: span,
        };
        // Long-run occluded fraction tracks `prob` (sampled per block).
        let n = 40_000usize;
        let occ = (0..n).filter(|&f| o.is_occluded(11, 1, f, 0)).count();
        let frac = occ as f64 / n as f64;
        assert!((frac - 0.3).abs() < 0.03, "occluded fraction {frac}");

        // Whenever occluded, the whole enclosing block is occluded -> a span of
        // at least `span` consecutive frames (never an isolated single frame).
        for f in 0..2000usize {
            if o.is_occluded(11, 1, f, 0) {
                let block_start = (f / span) * span;
                assert!((block_start..block_start + span).all(|g| o.is_occluded(11, 1, g, 0)));
            }
        }
    }

    #[test]
    fn observation_dropout_rate_and_determinism() {
        let o = ObservationModel {
            dropout_prob: 0.25,
            ..Default::default()
        };
        let mut dropped = 0usize;
        let n = 20_000usize;
        for fno in 0..n {
            let d = o.is_dropped(99, 1, fno, 0);
            // Deterministic: same inputs -> same output.
            assert_eq!(d, o.is_dropped(99, 1, fno, 0));
            if d {
                dropped += 1;
            }
        }
        let frac = dropped as f64 / n as f64;
        assert!((frac - 0.25).abs() < 0.02, "dropout fraction {frac}");
    }

    #[test]
    fn observation_jitter_is_bounded_centered_and_deterministic() {
        let sigma = 2.0;
        let o = ObservationModel {
            pixel_noise_px: sigma,
            ..Default::default()
        };
        let (mut sx, mut sy) = (0.0f64, 0.0f64);
        let n = 20_000usize;
        for fno in 0..n {
            let (x, y) = o.jitter_pixel(5, 2, fno, 0, 100.0, 200.0);
            // Deterministic.
            assert_eq!((x, y), o.jitter_pixel(5, 2, fno, 0, 100.0, 200.0));
            // Box-Muller radius is unbounded in theory but practically small;
            // anything beyond ~8 sigma over 20k draws would signal a bug.
            let r = ((x - 100.0).powi(2) + (y - 200.0).powi(2)).sqrt();
            assert!(r < 8.0 * sigma, "jitter radius {r} too large");
            sx += x - 100.0;
            sy += y - 200.0;
        }
        // Zero-mean: sample means are near 0 (a few hundredths of a pixel).
        assert!(
            (sx / n as f64).abs() < 0.1,
            "mean x offset {}",
            sx / n as f64
        );
        assert!(
            (sy / n as f64).abs() < 0.1,
            "mean y offset {}",
            sy / n as f64
        );
    }

    #[test]
    fn observation_clutter_count_position_and_determinism() {
        // Whole part: exactly 2 clutter blobs every frame, inside the image.
        let o = ObservationModel {
            clutter_per_frame: 2.0,
            ..Default::default()
        };
        for fno in 0..100 {
            let c = o.clutter(3, 0, fno, 640, 480);
            assert_eq!(c.len(), 2);
            assert_eq!(c, o.clutter(3, 0, fno, 640, 480)); // deterministic
            for (x, y) in c {
                assert!((0.0..640.0).contains(&x) && (0.0..480.0).contains(&y));
            }
        }

        // Fractional part: ~0.5 expected -> roughly half the frames have one.
        let o = ObservationModel {
            clutter_per_frame: 0.5,
            ..Default::default()
        };
        let n = 20_000usize;
        let with_one = (0..n)
            .filter(|&f| !o.clutter(3, 0, f, 640, 480).is_empty())
            .count();
        let frac = with_one as f64 / n as f64;
        assert!((frac - 0.5).abs() < 0.02, "clutter-present fraction {frac}");
    }
}
