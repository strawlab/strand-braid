// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reproducible end-to-end timing benchmark for the in-process 3D tracker.
//!
//! This builds on the deterministic [`crate::inject`] path: ground-truth 3D
//! points are projected to 2D detections and fed into the *real*
//! [`flydra2::CoordProcessor`] (triangulation, undistortion, the EKF,
//! nearest-neighbor data association, multi-target ID management, and braidz
//! writing). It does **not** exercise image rendering, the feature detector, or
//! the network — those are measured by the heavier image-level `ci2-sim` path.
//! What it *does* measure is the 3D reconstruction core, which is where cost
//! grows as the number of cameras and the number of simultaneous insects rise.
//!
//! To make scaling plots meaningful, [`run_once`] separates the wall-clock into
//! three phases so the tracker's cost is not conflated with the simulator's:
//!
//! - **prep**: project every ground-truth frame to synthetic 2D detections and
//!   materialize them in memory. This is the simulator's stand-in for "cameras +
//!   detector" and is *not* part of the tracker.
//! - **track**: drive [`flydra2::CoordProcessor::consume_stream`] over the
//!   pre-generated detections. This is the number that scales with cameras and
//!   insects — the headline tracking-throughput metric.
//! - **io**: drain the braidz writer task and zip the recording to disk.
//!
//! Pre-generating the detections (rather than producing them concurrently as
//! [`crate::inject::inject_and_track`] does) keeps the projection cost out of the
//! `track` measurement, and a generously sized writer buffer keeps disk
//! backpressure out of it too — so `track` reflects tracker compute, not the
//! simulator or the disk.
//!
//! Everything is deterministic: a `(scenario, num_frames)` defines a fixed
//! workload, so timings are reproducible up to machine noise (run with `reps` to
//! report a median).
//!
//! This module is gated behind the non-default `inprocess` cargo feature.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use braid_types::{
    BuiServerInfo, CamNum, FlydraFloatTimestampLocal, HostClock, RawCamName, SyncFno, Triggerbox,
};
use flydra2::{
    BraidMetadataBuilder, ConnectedCamerasManager, CoordProcessor, CoordProcessorConfig, FrameData,
    FrameDataAndPoints, NumberedRawUdpPoint, StreamItem,
};

use crate::Scenario;
use crate::scenario::{Arena, CameraRig, InsectSpec, Lissajous, ObservationModel};
use crate::world::World;

/// The timing (and a little track-quality context) of one benchmark run.
#[derive(Debug, Clone)]
pub struct BenchResult {
    /// Number of cameras in the rig.
    pub num_cameras: usize,
    /// Number of insects simulated.
    pub num_insects: usize,
    /// Number of synchronized frames tracked.
    pub num_frames: usize,
    /// Synchronized frame rate (the simulated real-time cadence).
    pub fps: f64,
    /// Time to project ground truth into synthetic 2D detections (simulator
    /// cost, *not* the tracker).
    pub prep: Duration,
    /// Time spent in [`flydra2::CoordProcessor::consume_stream`] — the tracker.
    pub track: Duration,
    /// Time to drain the braidz writer and zip the recording to disk.
    pub io: Duration,
    /// Number of distinct `obj_id`s (track fragments) the tracker produced. A
    /// sanity signal that tracking actually did work (≈ `num_insects` when
    /// healthy); large values indicate fragmentation under the chosen load.
    pub num_objects: usize,
    /// Total Kalman-estimate rows written (tracked object-frames).
    pub total_rows: usize,
}

impl BenchResult {
    /// Synchronized frames tracked per wall-clock second of the `track` phase.
    /// Higher is better; this is the primary tracker-throughput metric.
    pub fn track_fps(&self) -> f64 {
        self.num_frames as f64 / self.track.as_secs_f64()
    }

    /// How many times faster than real time the tracker ran: simulated seconds
    /// of data (`num_frames / fps`) divided by the `track` wall-clock. A value
    /// of `1.0` means the tracker exactly keeps up with the camera cadence;
    /// below `1.0` it cannot keep up live.
    pub fn realtime_factor(&self) -> f64 {
        let sim_seconds = self.num_frames as f64 / self.fps;
        sim_seconds / self.track.as_secs_f64()
    }

    /// Per-camera-frame tracking cost in microseconds: `track` divided by the
    /// number of (camera, frame) detections processed. This is the most
    /// load-normalized cost figure and is roughly what should be modeled when
    /// reasoning about scaling.
    pub fn us_per_camera_frame(&self) -> f64 {
        let cam_frames = (self.num_frames * self.num_cameras) as f64;
        self.track.as_secs_f64() * 1e6 / cam_frames
    }
}

/// Build a parametric benchmark scenario with `num_cameras` cameras and
/// `num_insects` insects.
///
/// The camera rig and arena match the in-process injector tests (a ~0.30 m cube
/// viewed by a ring of 640×512 pinhole cameras) so benchmark numbers are
/// comparable to those tests. Insects are placed on *distinct* Lissajous paths —
/// each gets its own per-axis frequency and phase offset, deterministically
/// derived from its index — so simultaneous targets trace separable curves
/// through the shared arena rather than overlapping into a degenerate
/// data-association blow-up that would distort timing.
///
/// `observation` lets a caller add detection noise / dropout / clutter; pass
/// [`ObservationModel::default`] for the clean perfect-world load.
pub fn bench_scenario(
    num_cameras: usize,
    num_insects: usize,
    fps: f64,
    seed: u64,
    observation: ObservationModel,
) -> Scenario {
    let insects = (0..num_insects)
        .map(|i| {
            let g = i as f64;
            InsectSpec {
                id: (i + 1) as u32,
                enter_t: 0.0,
                exit_t: None,
                motion: Lissajous {
                    // Distinct per-insect frequencies and phases so the paths do
                    // not collapse onto each other.
                    freq_hz: [0.11 + 0.013 * g, 0.13 + 0.017 * g, 0.07 + 0.011 * g],
                    phase: [0.7 * g, 1.0 + 1.1 * g, 2.0 + 0.5 * g],
                    fill: 0.6,
                    maneuver_amp_m: 0.0,
                    maneuver_freq_hz: 0.0,
                },
            }
        })
        .collect();

    Scenario {
        seed,
        fps,
        arena: Arena {
            min: [-0.15, -0.15, 0.0],
            max: [0.15, 0.15, 0.30],
        },
        cameras: CameraRig {
            count: num_cameras,
            radius_m: 0.6,
            height_m: 0.7,
            focal_length_px: 900.0,
            image_width: 640,
            image_height: 512,
        },
        insects,
        blob: Default::default(),
        bg_warmup_frames: 0,
        timing: Default::default(),
        observation,
        reported_fps: None,
        calibration_perturbation: Default::default(),
    }
}

/// Project all ground-truth frames of `scenario` into synthetic 2D detections,
/// ready to feed the tracker. Returns the per-frame, per-camera stream items in
/// injection order plus a trailing [`StreamItem::EOF`].
///
/// This mirrors the producer in [`crate::inject::inject_and_track`] but
/// materializes the whole stream up front so the projection cost can be timed
/// separately from the tracker.
fn pregenerate_detections(
    scenario: &Scenario,
    num_frames: usize,
    recon: &flydra_mvg::FlydraMultiCameraSystem<f64>,
    cam_manager: &ConnectedCamerasManager,
) -> eyre::Result<Vec<StreamItem>> {
    let world = World::new(scenario.clone());
    let obs = &scenario.observation;
    let fps = scenario.fps;
    let count = scenario.cameras.count;
    let base = chrono::DateTime::<chrono::Utc>::UNIX_EPOCH;

    // One packet per (frame, camera), plus a final EOF.
    let mut items = Vec::with_capacity(num_frames * count + 1);
    for fno in 0..num_frames {
        let t = fno as f64 / fps;
        let states = world.state_at(t);
        let received: FlydraFloatTimestampLocal<HostClock> =
            (base + chrono::Duration::nanoseconds((t * 1e9) as i64)).into();
        let trigger = Some(FlydraFloatTimestampLocal::<Triggerbox>::from_f64(t));
        for k in 0..count {
            let cam_name = RawCamName::new(Scenario::camera_name(k));
            let cam_num = cam_manager
                .cam_num(&cam_name)
                .ok_or_else(|| eyre::eyre!("no cam_num for {}", cam_name.as_str()))?;

            let mut pixels: Vec<(f64, f64)> = states
                .iter()
                .filter(|ins| !obs.is_suppressed(scenario.seed, k, fno, ins.id))
                .filter_map(|ins| {
                    crate::projection::project_pixel(
                        recon,
                        &Scenario::camera_name(k),
                        scenario.cameras.image_width,
                        scenario.cameras.image_height,
                        &ins.pos,
                    )
                    .map(|(x, y)| obs.jitter_pixel(scenario.seed, k, fno, ins.id, x, y))
                })
                .collect();
            pixels.extend(obs.clutter(
                scenario.seed,
                k,
                fno,
                scenario.cameras.image_width,
                scenario.cameras.image_height,
            ));

            let points: Vec<NumberedRawUdpPoint> = pixels
                .iter()
                .enumerate()
                .map(|(i, &(x, y))| NumberedRawUdpPoint {
                    idx: i as u8,
                    pt: braid_types::FlydraRawUdpPoint {
                        x0_abs: x,
                        y0_abs: y,
                        area: 10.0,
                        maybe_slope_eccentricty: None,
                        cur_val: 0,
                        mean_val: 0.0,
                        sumsqf_val: 0.0,
                    },
                })
                .collect();

            let frame_data = FrameData::new(
                cam_name,
                cam_num,
                SyncFno(fno as u64),
                trigger.clone(),
                received.clone(),
                Some((t * 1e9) as u64),
                Some(fno as u64),
            );
            items.push(StreamItem::Packet(FrameDataAndPoints {
                frame_data,
                points,
            }));
        }
    }
    items.push(StreamItem::EOF);
    Ok(items)
}

/// Run one benchmark case: pre-generate detections, drive the real tracker over
/// them, and return the phase timings.
///
/// `out_braid_dir` must end in `.braid`; the tracker writes its working
/// directory there and zips a sibling `.braidz`, exactly as the live system
/// does. Use a path on fast storage (e.g. a tmpdir) so the `io` phase is not
/// dominated by slow disk.
pub async fn run_once(
    scenario: &Scenario,
    num_frames: usize,
    out_braid_dir: &Path,
) -> eyre::Result<BenchResult> {
    if out_braid_dir.extension().and_then(|e| e.to_str()) != Some("braid") {
        eyre::bail!(
            "out_braid_dir must end in `.braid` (got {})",
            out_braid_dir.display()
        );
    }
    let out_braidz = out_braid_dir.with_extension("braidz");

    let recon = crate::calibration::build_calibration(scenario)?;
    let track_recon = crate::calibration::build_tracking_calibration(scenario)?;
    let fps = scenario.fps;
    let count = scenario.cameras.count;

    let all_expected_cameras: BTreeSet<RawCamName> = (0..count)
        .map(|k| RawCamName::new(Scenario::camera_name(k)))
        .collect();
    let predefined_cam_nums: BTreeMap<RawCamName, CamNum> = (0..count)
        .map(|k| (RawCamName::new(Scenario::camera_name(k)), CamNum(k as u8)))
        .collect();

    let mut cam_manager = ConnectedCamerasManager::new(
        &Some(track_recon.clone()),
        all_expected_cameras,
        Arc::new(AtomicBool::new(true)),
        Arc::new(AtomicBool::new(true)),
        None,
        Some(predefined_cam_nums),
    );
    for k in 0..count {
        cam_manager
            .register_new_camera(
                &RawCamName::new(Scenario::camera_name(k)),
                &BuiServerInfo::NoServer,
                None,
            )
            .map_err(|msg| eyre::eyre!("registering sim camera: {msg}"))?;
    }

    // Phase 1: prep. Project all ground truth into synthetic 2D detections.
    let t_prep = Instant::now();
    let items = pregenerate_detections(scenario, num_frames, &recon, &cam_manager)?;
    let prep = t_prep.elapsed();

    // Size the writer buffer to hold the whole run so disk backpressure never
    // stalls the tracker loop during the `track` phase: one data2d message per
    // (frame, camera) plus a healthy margin for per-frame kalman-save messages.
    let write_buffer_size_num_messages = num_frames * (count + 4) + 64;

    let coord_processor = CoordProcessor::new(
        CoordProcessorConfig {
            tracking_params: braid_types::default_tracking_params_full_3d(),
            save_empty_data2d: true,
            ignore_latency: true,
            mini_arena_debug_cfg: None,
            write_buffer_size_num_messages,
        },
        cam_manager.clone(),
        Some(track_recon.clone()),
        BraidMetadataBuilder::saving_program_name("braid-sim-bench"),
    )?;

    std::fs::create_dir_all(out_braid_dir)?;
    coord_processor
        .braidz_write_tx
        .send(flydra2::SaveToDiskMsg::StartSavingCsv(
            flydra2::StartSavingCsvConfig {
                out_dir: out_braid_dir.to_path_buf(),
                local: Some(chrono::Local::now()),
                git_rev: "braid-sim-bench".to_string(),
                fps: Some(fps as f32),
                per_cam_data: Default::default(),
                print_stats: false,
                save_performance_histograms: false,
            },
        ))
        .await
        .map_err(|e| eyre::eyre!("starting braidz save: {e}"))?;

    // Phase 2: track. Feed the pre-generated stream straight into the tracker.
    let stream = tokio_stream::iter(items);
    let t_track = Instant::now();
    let writer_jh = coord_processor
        .consume_stream(stream, Some(fps as f32))
        .await?;
    let track = t_track.elapsed();

    // Phase 3: io. Drain the writer task and zip the braidz to disk.
    let t_io = Instant::now();
    writer_jh.await??;
    let io = t_io.elapsed();

    // Read back the track count for a quick "did it actually track?" signal.
    let stats = crate::score::track_stats(&out_braidz)?;

    Ok(BenchResult {
        num_cameras: count,
        num_insects: scenario.insects.len(),
        num_frames,
        fps,
        prep,
        track,
        io,
        num_objects: stats.num_objects,
        total_rows: stats.total_rows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bench_scenario_has_requested_shape_and_distinct_paths() {
        let s = bench_scenario(4, 3, 100.0, 7, ObservationModel::default());
        assert_eq!(s.cameras.count, 4);
        assert_eq!(s.insects.len(), 3);
        // Distinct ids 1..=3.
        let ids: Vec<u32> = s.insects.iter().map(|i| i.id).collect();
        assert_eq!(ids, vec![1, 2, 3]);
        // Paths are distinct: no two insects share the same x-frequency.
        for a in 0..s.insects.len() {
            for b in (a + 1)..s.insects.len() {
                assert_ne!(
                    s.insects[a].motion.freq_hz, s.insects[b].motion.freq_hz,
                    "insects {a} and {b} share a motion frequency"
                );
            }
        }
        // All insects present together at t=1s.
        let world = World::new(s.clone());
        assert_eq!(world.state_at(1.0).len(), 3);
    }

    /// End-to-end smoke test: a tiny run produces timings and actually tracks.
    #[tokio::test]
    async fn run_once_produces_timings_and_tracks() -> eyre::Result<()> {
        let s = bench_scenario(3, 1, 100.0, 7, ObservationModel::default());
        let tmp = tempfile::tempdir()?;
        let out = tmp.path().join("bench.braid");

        let r = run_once(&s, 120, &out).await?;

        assert_eq!(r.num_cameras, 3);
        assert_eq!(r.num_insects, 1);
        assert_eq!(r.num_frames, 120);
        assert!(r.track > Duration::ZERO, "track phase took no time");
        assert!(r.track_fps() > 0.0);
        assert!(r.realtime_factor() > 0.0);
        assert!(r.us_per_camera_frame() > 0.0);
        // One insect on a clean path should be tracked (some rows written).
        assert!(r.total_rows > 0, "tracker produced no kalman estimates");
        assert!(r.num_objects >= 1, "expected at least one track");
        Ok(())
    }
}
