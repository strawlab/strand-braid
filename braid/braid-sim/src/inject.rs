// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Level-B detection injector (plan §3.4-B / §5): drive the real flydra2 3D
//! tracker *in-process* with synthetic 2D detections.
//!
//! This is the fast, deterministic complement to the image-level `ci2-sim`
//! path. It skips image rendering, the feature detector, UDP, and camera
//! registration over the network: ground-truth 3D points are projected to 2D
//! with the same calibration the tracker reconstructs with, the (optionally
//! imperfect) detections are fed straight into [`flydra2::CoordProcessor`], and
//! the resulting `.braid` recording is written to disk so the same
//! [`crate::truth`] oracle can score it.
//!
//! Because there is no real-time async camera path and no nondeterministic
//! detector, a `(scenario, seed)` reproduces the 3D-core behavior
//! (triangulation, the EKF, nearest-neighbor data association, multi-target ID
//! management) exactly — ideal for tight regression tests of the tracker.
//!
//! This module is gated behind the non-default `inprocess` cargo feature
//! because it pulls in the full `flydra2` tracker and a tokio runtime.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use braid_types::{
    BuiServerInfo, CamNum, FlydraFloatTimestampLocal, HostClock, RawCamName, SyncFno, Triggerbox,
};
use flydra2::{
    BraidMetadataBuilder, ConnectedCamerasManager, CoordProcessor, CoordProcessorConfig, FrameData,
    FrameDataAndPoints, NumberedRawUdpPoint, StreamItem,
};

use crate::Scenario;
use crate::world::World;

/// Synthesize 2D detections for a scenario and run them through the real
/// flydra2 tracker in-process, writing the result as a `.braidz` and returning
/// its path.
///
/// `out_braid_dir` must end in `.braid`: the tracker writes its working
/// directory there, then zips it into a sibling `.braidz` (with the `.braid`
/// directory removed), exactly as the live system does. The returned path is
/// that `.braidz`, ready for [`crate::truth::score_against_truth`].
///
/// `num_frames` frames are injected at the scenario frame rate, starting at
/// simulation time `t = 0` (so the recording's frame `f` depicts world time
/// `f / fps`, and the oracle needs no frame offset). The scenario's
/// [`crate::scenario::ObservationModel`] is applied to the detections, so noise,
/// dropout, and clutter all exercise the 3D core.
///
/// `tracker_fps` overrides the frame rate handed to the tracker (and thus the
/// EKF `dt = 1 / tracker_fps`) *without* changing the cadence at which
/// detections are produced — they are always injected at `scenario.fps`. Pass
/// `None` to track at the true cadence (the normal case). Passing a value that
/// disagrees with `scenario.fps` reproduces the live-vs-retrack fps-mismatch
/// fragmentation mechanism in isolation: a too-high `tracker_fps` shrinks the
/// process noise so a maneuvering target falls outside the acceptance gate, the
/// track coasts, and covariance kill fragments it (see `tests/m7_fps_*`).
pub async fn inject_and_track(
    scenario: &Scenario,
    num_frames: usize,
    tracker_fps: Option<f64>,
    out_braid_dir: &Path,
) -> eyre::Result<std::path::PathBuf> {
    if out_braid_dir.extension().and_then(|e| e.to_str()) != Some("braid") {
        eyre::bail!(
            "out_braid_dir must end in `.braid` (got {})",
            out_braid_dir.display()
        );
    }
    let out_braidz = out_braid_dir.with_extension("braidz");
    let recon = crate::calibration::build_calibration(scenario)?;
    let world = World::new(scenario.clone());
    // The cadence at which detections are produced (frame `f` depicts `f / fps`).
    let fps = scenario.fps;
    // The frame rate the tracker uses for its EKF `dt`; defaults to the true
    // cadence but can be deliberately mismatched to reproduce the fps bug.
    let tracker_fps = tracker_fps.unwrap_or(fps);
    let count = scenario.cameras.count;

    let all_expected_cameras: BTreeSet<RawCamName> = (0..count)
        .map(|k| RawCamName::new(Scenario::camera_name(k)))
        .collect();
    let predefined_cam_nums: BTreeMap<RawCamName, CamNum> = (0..count)
        .map(|k| (RawCamName::new(Scenario::camera_name(k)), CamNum(k as u8)))
        .collect();

    let mut cam_manager = ConnectedCamerasManager::new(
        &Some(recon.clone()),
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

    let coord_processor = CoordProcessor::new(
        CoordProcessorConfig {
            tracking_params: braid_types::default_tracking_params_full_3d(),
            save_empty_data2d: true,
            ignore_latency: true,
            mini_arena_debug_cfg: None,
            write_buffer_size_num_messages:
                braid_config_data::default_write_buffer_size_num_messages(),
        },
        cam_manager.clone(),
        Some(recon.clone()),
        BraidMetadataBuilder::saving_program_name("braid-sim-inject"),
    )?;

    // Write the tracking output to `out_braid_dir`.
    std::fs::create_dir_all(out_braid_dir)?;
    coord_processor
        .braidz_write_tx
        .send(flydra2::SaveToDiskMsg::StartSavingCsv(
            flydra2::StartSavingCsvConfig {
                out_dir: out_braid_dir.to_path_buf(),
                local: Some(chrono::Local::now()),
                git_rev: "braid-sim-inject".to_string(),
                fps: Some(fps as f32),
                per_cam_data: Default::default(),
                print_stats: false,
                save_performance_histograms: false,
            },
        ))
        .await
        .map_err(|e| eyre::eyre!("starting braidz save: {e}"))?;

    let (frame_data_tx, frame_data_rx) = tokio::sync::mpsc::channel(10);
    let frame_data_rx = tokio_stream::wrappers::ReceiverStream::new(frame_data_rx);

    // Producer: project ground truth and feed synthetic detections.
    let scenario = scenario.clone();
    let cam_manager_prod = cam_manager.clone();
    let producer = async move {
        let obs = &scenario.observation;
        // A fixed reference instant: per-frame host timestamps advance at the
        // true cadence (deterministic; no wall-clock dependence in the data).
        let base = chrono::DateTime::<chrono::Utc>::UNIX_EPOCH;
        for fno in 0..num_frames {
            let t = fno as f64 / fps;
            let states = world.state_at(t);
            let received: FlydraFloatTimestampLocal<HostClock> =
                (base + chrono::Duration::nanoseconds((t * 1e9) as i64)).into();
            let trigger = Some(FlydraFloatTimestampLocal::<Triggerbox>::from_f64(t));
            for k in 0..count {
                let cam_name = RawCamName::new(Scenario::camera_name(k));
                let cam_num = cam_manager_prod
                    .cam_num(&cam_name)
                    .ok_or_else(|| eyre::eyre!("no cam_num for {}", cam_name.as_str()))?;

                let mut pixels: Vec<(f64, f64)> = states
                    .iter()
                    .filter(|ins| !obs.is_dropped(scenario.seed, k, fno, ins.id))
                    .filter_map(|ins| {
                        crate::projection::project_pixel(
                            &recon,
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
                frame_data_tx
                    .send(StreamItem::Packet(FrameDataAndPoints {
                        frame_data,
                        points,
                    }))
                    .await
                    .map_err(|e| eyre::eyre!("sending frame: {e}"))?;
            }
        }
        frame_data_tx
            .send(StreamItem::EOF)
            .await
            .map_err(|e| eyre::eyre!("sending EOF: {e}"))?;
        Ok::<(), eyre::Report>(())
    };

    let consume = coord_processor.consume_stream(frame_data_rx, Some(tracker_fps as f32));
    let (writer_jh, prod) = tokio::join!(consume, producer);
    prod?;
    writer_jh?.await??;
    Ok(out_braidz)
}
