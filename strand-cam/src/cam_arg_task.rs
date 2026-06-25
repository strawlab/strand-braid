// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Camera command (`CamArg`) dispatch task.
//!
//! Extracted from the monolithic `run()` function in `strand-cam.rs`. This
//! drives the top-level control loop: it receives `CamArg` commands from the
//! web/UI layer, applies them to the camera and the shared store, and performs
//! the camera shutdown sequence once a `DoQuit` command (or a closed channel)
//! ends the loop.

use std::sync::{Arc, RwLock};

use futures::stream::StreamExt;
use tracing::{debug, error, info};

use eyre::Result;

use async_change_tracker::ChangeTracker;
use ci2::Camera;
use event_stream_types::{ConnectionSessionKey, EventBroadcaster};
use strand_cam_bui_types::RecordingPath;
use strand_cam_remote_control::CamArg;
use strand_cam_storetype::{STRAND_CAM_QUIT_EVENT_NAME, StoreType};

use crate::{FrameProcessingErrorState, Msg, send_cam_settings_to_braid, to_eyre};

#[cfg(feature = "flydra_feat_detect")]
use preferences_serde1::Preferences;

#[cfg(feature = "flydra_feat_detect")]
use flydra_feature_detector_types::ImPtDetectCfg;
#[cfg(feature = "flydra_feat_detect")]
use strand_cam_remote_control::CsvSaveConfig;

#[cfg(feature = "flydra_feat_detect")]
use crate::ImPtDetectCfgSource;

#[cfg(feature = "flydratrax")]
use strand_cam_storetype::{KalmanTrackingConfig, LedProgramConfig};

#[cfg(feature = "flydratrax")]
use crate::{KALMAN_TRACKING_PREFS_KEY, LED_PROGRAM_PREFS_KEY};

#[cfg(feature = "checkercal")]
use std::fs::File;

#[cfg(feature = "checkercal")]
use serde::Serialize;

#[cfg(feature = "checkercal")]
use crate::APP_INFO;

/// Receive and apply `CamArg` commands until the loop is told to quit, then
/// cleanly stop the camera.
#[expect(
    clippy::too_many_arguments,
    reason = "extracted verbatim from run(); grouping these into a context struct is left for a later cleanup"
)]
pub(crate) async fn run_cam_arg_task<C>(
    mut cam: ci2_async::ThreadedAsyncCamera<C>,
    cam_args_rx: tokio::sync::mpsc::Receiver<CamArg>,
    shared_store_arc: Arc<RwLock<ChangeTracker<StoreType>>>,
    event_broadcaster: EventBroadcaster<ConnectionSessionKey>,
    frame_processing_error_state: Arc<RwLock<crate::FrameProcessingErrorState>>,
    transmit_msg_tx: Option<tokio::sync::mpsc::Sender<braid_types::BraidHttpApiCallback>>,
    current_cam_settings_extension: String,
    raw_cam_name: braid_types::RawCamName,
    tx_frame2: tokio::sync::mpsc::Sender<Msg>,
    #[cfg(feature = "flydra_feat_detect")] tracker_cfg_src: crate::ImPtDetectCfgSource,
    #[cfg(feature = "checkercal")] cam_name2: braid_types::RawCamName,
    #[cfg(feature = "checkercal")] collected_corners_arc: crate::CollectedCornersArc,
    #[cfg(feature = "checkercal")] image_width: u32,
    #[cfg(feature = "checkercal")] image_height: u32,
) -> Result<()>
where
    C: 'static + ci2::Camera + Send,
{
    let mut cam_args_rx = tokio_stream::wrappers::ReceiverStream::new(cam_args_rx);

    while let Some(cam_args) = cam_args_rx.next().await {
        debug!("handling camera command {:?}", cam_args);
        #[expect(unused_variables)]
        match cam_args {
            CamArg::SetIngoreFutureFrameProcessingErrors(v) => {
                let mut state = frame_processing_error_state.write().unwrap();
                match v {
                    None => {
                        *state = FrameProcessingErrorState::IgnoreAll;
                    }
                    Some(val) => {
                        if val <= 0 {
                            *state = FrameProcessingErrorState::NotifyAll;
                        } else {
                            let when =
                                chrono::Utc::now() + chrono::Duration::try_seconds(val).unwrap();
                            *state = FrameProcessingErrorState::IgnoreUntil(when);
                        }
                    }
                }

                let mut tracker = shared_store_arc.write().unwrap();
                tracker.modify(|tracker| tracker.had_frame_processing_error = false);
            }
            CamArg::SetExposureTime(v) => match cam.set_exposure_time(v) {
                Ok(()) => {
                    if let Some(transmit_msg_tx) = &transmit_msg_tx {
                        send_cam_settings_to_braid(
                            &cam.node_map_save().unwrap(),
                            transmit_msg_tx,
                            &current_cam_settings_extension,
                            &raw_cam_name,
                        )
                        .await
                        .unwrap();
                    }
                    let mut tracker = shared_store_arc.write().unwrap();
                    tracker.modify(|tracker| tracker.exposure_time.current = v);
                }
                Err(e) => {
                    error!("setting exposure_time: {:?}", e);
                }
            },
            CamArg::SetGain(v) => match cam.set_gain(v) {
                Ok(()) => {
                    if let Some(transmit_msg_tx) = &transmit_msg_tx {
                        send_cam_settings_to_braid(
                            &cam.node_map_save().unwrap(),
                            transmit_msg_tx,
                            &current_cam_settings_extension,
                            &raw_cam_name,
                        )
                        .await
                        .unwrap();
                    }
                    let mut tracker = shared_store_arc.write().unwrap();
                    tracker.modify(|tracker| tracker.gain.current = v);
                }
                Err(e) => {
                    error!("setting gain: {:?}", e);
                }
            },
            CamArg::SetGainAuto(v) => match cam.set_gain_auto(v) {
                Ok(()) => {
                    if let Some(transmit_msg_tx) = &transmit_msg_tx {
                        send_cam_settings_to_braid(
                            &cam.node_map_save().unwrap(),
                            transmit_msg_tx,
                            &current_cam_settings_extension,
                            &raw_cam_name,
                        )
                        .await
                        .unwrap();
                    }
                    let mut tracker = shared_store_arc.write().unwrap();
                    tracker.modify(|shared| match cam.gain_auto() {
                        Ok(latest) => {
                            shared.gain_auto = Some(latest);
                        }
                        Err(e) => {
                            shared.gain_auto = Some(v);
                            error!("after setting gain_auto, error getting: {:?}", e);
                        }
                    });
                }
                Err(e) => {
                    error!("setting gain_auto: {:?}", e);
                }
            },
            CamArg::SetRecordingFps(v) => {
                let mut tracker = shared_store_arc.write().unwrap();
                tracker.modify(|tracker| tracker.mp4_max_framerate = v);
            }
            CamArg::SetMp4CudaDevice(v) => {
                let mut tracker = shared_store_arc.write().unwrap();
                tracker.modify(|tracker| tracker.mp4_cuda_device = v);
            }
            CamArg::SetMp4MaxFramerate(v) => {
                let mut tracker = shared_store_arc.write().unwrap();
                tracker.modify(|tracker| tracker.mp4_max_framerate = v);
            }
            CamArg::SetMp4Bitrate(v) => {
                let mut tracker = shared_store_arc.write().unwrap();
                tracker.modify(|tracker| tracker.mp4_bitrate = v);
            }
            CamArg::SetMp4Codec(v) => {
                let mut tracker = shared_store_arc.write().unwrap();
                tracker.modify(|tracker| tracker.mp4_codec = v);
            }
            CamArg::SetExposureAuto(v) => match cam.set_exposure_auto(v) {
                Ok(()) => {
                    if let Some(transmit_msg_tx) = &transmit_msg_tx {
                        send_cam_settings_to_braid(
                            &cam.node_map_save().unwrap(),
                            transmit_msg_tx,
                            &current_cam_settings_extension,
                            &raw_cam_name,
                        )
                        .await
                        .unwrap();
                    }
                    let mut tracker = shared_store_arc.write().unwrap();
                    tracker.modify(|shared| match cam.exposure_auto() {
                        Ok(latest) => {
                            shared.exposure_auto = Some(latest);
                        }
                        Err(e) => {
                            shared.exposure_auto = Some(v);
                            error!("after setting exposure_auto, error getting: {:?}", e);
                        }
                    });
                }
                Err(e) => {
                    error!("setting exposure_auto: {:?}", e);
                }
            },
            CamArg::SetFrameRateLimitEnabled(v) => match cam.set_acquisition_frame_rate_enable(v) {
                Ok(()) => {
                    if let Some(transmit_msg_tx) = &transmit_msg_tx {
                        send_cam_settings_to_braid(
                            &cam.node_map_save().unwrap(),
                            transmit_msg_tx,
                            &current_cam_settings_extension,
                            &raw_cam_name,
                        )
                        .await
                        .unwrap();
                    }
                    let mut tracker = shared_store_arc.write().unwrap();
                    tracker.modify(|shared| match cam.acquisition_frame_rate_enable() {
                        Ok(latest) => {
                            shared.frame_rate_limit_enabled = latest;
                        }
                        Err(e) => {
                            error!(
                                "after setting frame_rate_limit_enabled, error getting: {:?}",
                                e
                            );
                        }
                    });
                }
                Err(e) => {
                    error!("setting frame_rate_limit_enabled: {:?}", e);
                }
            },
            CamArg::SetFrameRateLimit(v) => match cam.set_acquisition_frame_rate(v) {
                Ok(()) => {
                    if let Some(transmit_msg_tx) = &transmit_msg_tx {
                        send_cam_settings_to_braid(
                            &cam.node_map_save().unwrap(),
                            transmit_msg_tx,
                            &current_cam_settings_extension,
                            &raw_cam_name,
                        )
                        .await
                        .unwrap();
                    }
                    let mut tracker = shared_store_arc.write().unwrap();
                    tracker.modify(|shared| match cam.acquisition_frame_rate() {
                        Ok(latest) => {
                            if let Some(ref mut frl) = shared.frame_rate_limit {
                                frl.current = latest;
                            } else {
                                error!("frame_rate_limit is expectedly None");
                            }
                        }
                        Err(e) => {
                            error!("after setting frame_rate_limit, error getting: {:?}", e);
                        }
                    });
                }
                Err(e) => {
                    error!("setting frame_rate_limit: {:?}", e);
                }
            },
            CamArg::SetFrameOffset(fo) => {
                tx_frame2
                    .send(Msg::SetFrameOffset(fo))
                    .await
                    .map_err(to_eyre)?;
            }
            CamArg::SetTriggerboxClockModel(cm) => {
                tx_frame2
                    .send(Msg::SetTriggerboxClockModel(cm))
                    .await
                    .map_err(to_eyre)?;
            }
            CamArg::SetFormatStr(v) => {
                let mut tracker = shared_store_arc.write().unwrap();
                tracker.modify(|tracker| tracker.format_str = v);
            }
            CamArg::SetIsRecordingMp4(do_recording) => {
                // Copy values from cache and release the lock immediately.
                let is_recording_mp4 = {
                    let tracker = shared_store_arc.read().unwrap();
                    let shared: &StoreType = tracker.as_ref();
                    shared.is_recording_mp4.is_some()
                };

                if is_recording_mp4 != do_recording {
                    let msg = if do_recording {
                        Msg::StartMp4
                    } else {
                        Msg::StopMp4
                    };

                    // Send the command.
                    tx_frame2.send(msg).await.map_err(to_eyre)?;
                }
            }
            CamArg::ToggleAprilTagFamily(family) => {
                let mut tracker = shared_store_arc.write().unwrap();
                tracker.modify(|shared| {
                    if let Some(ref mut ts) = shared.apriltag_state {
                        if ts.is_recording_csv.is_some() {
                            error!("will not change families while recording CSV");
                        } else {
                            ts.april_family = family;
                        }
                    } else {
                        error!("no apriltag support, not switching state");
                    }
                });
            }
            CamArg::ToggleAprilTagDetection(do_detection) => {
                let mut tracker = shared_store_arc.write().unwrap();
                tracker.modify(|shared| {
                    if let Some(ref mut ts) = shared.apriltag_state {
                        ts.do_detection = do_detection;
                    } else {
                        error!("no apriltag support, not switching state");
                    }
                });
            }
            CamArg::ToggleImOpsDetection(do_detection) => {
                let mut tracker = shared_store_arc.write().unwrap();
                tracker.modify(|shared| {
                    shared.im_ops_state.do_detection = do_detection;
                });
            }
            CamArg::SetImOpsDestination(v) => {
                let mut tracker = shared_store_arc.write().unwrap();
                tracker.modify(|shared| {
                    shared.im_ops_state.destination = v;
                });
            }
            CamArg::SetImOpsSource(v) => {
                let mut tracker = shared_store_arc.write().unwrap();
                tracker.modify(|shared| {
                    shared.im_ops_state.source = v;
                });
            }
            CamArg::SetImOpsCenterX(v) => {
                let mut tracker = shared_store_arc.write().unwrap();
                tracker.modify(|shared| {
                    shared.im_ops_state.center_x = v;
                });
            }
            CamArg::SetImOpsCenterY(v) => {
                let mut tracker = shared_store_arc.write().unwrap();
                tracker.modify(|shared| {
                    shared.im_ops_state.center_y = v;
                });
            }
            CamArg::SetImOpsThreshold(v) => {
                let mut tracker = shared_store_arc.write().unwrap();
                tracker.modify(|shared| {
                    shared.im_ops_state.threshold = v;
                });
            }

            CamArg::SetIsRecordingAprilTagCsv(do_recording) => {
                let new_val = {
                    let tracker = shared_store_arc.read().unwrap();
                    let shared: &StoreType = tracker.as_ref();
                    if let Some(ref ts) = shared.apriltag_state {
                        info!(
                            "changed recording april tag value: do_recording={}",
                            do_recording
                        );
                        if do_recording {
                            Some(Some(RecordingPath::new(
                                shared.format_str_apriltag_csv.clone(),
                            )))
                        } else {
                            Some(None)
                        }
                    } else {
                        error!("no apriltag support, not switching state");
                        None
                    }
                };

                // Here we asynchronously send the message to initiate or stop
                // recording without holding any lock.
                if let Some(new_val) = &new_val {
                    let msg = match new_val {
                        Some(recording_path) => Msg::StartAprilTagRec(recording_path.path()),
                        None => Msg::StopAprilTagRec,
                    };
                    tx_frame2.send(msg).await.map_err(to_eyre)?;
                }

                // Here we save the new recording state.
                if let Some(new_val) = new_val {
                    let mut tracker = shared_store_arc.write().unwrap();
                    tracker.modify(|shared| {
                        if let Some(ref mut ts) = shared.apriltag_state {
                            ts.is_recording_csv = new_val;
                        };
                    });
                }
            }
            CamArg::PostTrigger => {
                info!("Start MP4 recording via post trigger.");
                tx_frame2
                    .send(Msg::PostTriggerStartMp4)
                    .await
                    .map_err(to_eyre)?;
            }
            CamArg::SetPostTriggerBufferSize(size) => {
                info!("Set post trigger buffer size to {size}.");
                tx_frame2
                    .send(Msg::SetPostTriggerBufferSize(size))
                    .await
                    .map_err(to_eyre)?;
            }
            CamArg::SetIsRecordingFmf(do_recording) => {
                // Copy values from cache and release the lock immediately.
                let (is_recording_fmf, format_str, recording_framerate) = {
                    let tracker = shared_store_arc.read().unwrap();
                    let shared: &StoreType = tracker.as_ref();
                    (
                        shared.is_recording_fmf.clone(),
                        shared.format_str.clone(),
                        shared.mp4_max_framerate.clone(),
                    )
                };

                if is_recording_fmf.is_some() != do_recording {
                    info!("changed recording fmf value: do_recording={}", do_recording);

                    // Compute new values.
                    let (msg, new_val) = if do_recording {
                        // change state
                        let local: chrono::DateTime<chrono::Local> = chrono::Local::now();
                        let filename = local.format(format_str.as_str()).to_string();
                        (
                            Msg::StartFMF((filename.clone(), recording_framerate)),
                            Some(RecordingPath::new(filename)),
                        )
                    } else {
                        (Msg::StopFMF, None)
                    };

                    // Send the command.
                    tx_frame2.send(msg).await.map_err(to_eyre)?;

                    // Save the new recording state.
                    let mut tracker = shared_store_arc.write().unwrap();
                    tracker.modify(|shared| {
                        shared.is_recording_fmf = new_val;
                    });
                }
            }
            CamArg::SetIsRecordingUfmf(do_recording) => {
                #[cfg(feature = "flydra_feat_detect")]
                {
                    // Copy values from cache and release the lock immediately.
                    let (is_recording_ufmf, format_str_ufmf) = {
                        let tracker = shared_store_arc.read().unwrap();
                        let shared: &StoreType = tracker.as_ref();
                        (
                            shared.is_recording_ufmf.clone(),
                            shared.format_str_ufmf.clone(),
                        )
                    };

                    if is_recording_ufmf.is_some() != do_recording {
                        info!(
                            "changed recording ufmf value: do_recording={}",
                            do_recording
                        );

                        // Compute new values.
                        let (msg, new_val) = if do_recording {
                            // change state
                            let local: chrono::DateTime<chrono::Local> = chrono::Local::now();
                            let filename = local.format(format_str_ufmf.as_str()).to_string();
                            (
                                Msg::StartUFMF(filename.clone()),
                                Some(RecordingPath::new(filename)),
                            )
                        } else {
                            (Msg::StopUFMF, None)
                        };

                        // Send the command.
                        tx_frame2.send(msg).await.map_err(to_eyre)?;

                        // Save the new recording state.
                        let mut tracker = shared_store_arc.write().unwrap();
                        tracker.modify(|shared| {
                            shared.is_recording_ufmf = new_val;
                        });
                    }
                }
            }
            CamArg::SetIsDoingObjDetection(value) => {
                #[cfg(feature = "flydra_feat_detect")]
                {
                    {
                        // update store
                        let mut tracker = shared_store_arc.write().unwrap();
                        tracker.modify(|shared| {
                            shared.is_doing_object_detection = value;
                        });
                    }
                    tx_frame2
                        .send(Msg::SetTracking(value))
                        .await
                        .map_err(to_eyre)?;
                }
            }
            CamArg::DoQuit => {
                break;
            }
            CamArg::SetIsSavingObjDetectionCsv(value) => {
                // update store in worker thread
                #[cfg(feature = "flydra_feat_detect")]
                tx_frame2
                    .send(Msg::SetIsSavingObjDetectionCsv(value))
                    .await
                    .map_err(to_eyre)?;
            }
            CamArg::SetObjDetectionConfig(yaml_buf) => {
                // parse buffer
                #[cfg(feature = "flydra_feat_detect")]
                match serde_yaml::from_str::<ImPtDetectCfg>(&yaml_buf) {
                    Err(e) => {
                        error!("ignoring ImPtDetectCfg with parse error: {:?}", e)
                    }
                    Ok(cfg) => {
                        let cfg2 = cfg.clone();

                        // Update config and send to frame process thread
                        tx_frame2
                            .send(Msg::SetExpConfig(cfg.clone()))
                            .await
                            .map_err(to_eyre)?;
                        {
                            let mut tracker = shared_store_arc.write().unwrap();
                            tracker.modify(|shared| {
                                shared.im_pt_detect_cfg = cfg;
                            });
                        }

                        if let ImPtDetectCfgSource::ChangedSavedToDisk(ref src) = tracker_cfg_src {
                            let (app_info, prefs_key) = src;
                            match cfg2.save(app_info, prefs_key) {
                                Ok(()) => {
                                    info!("saved new detection config");
                                }
                                Err(e) => {
                                    error!(
                                        "saving preferences failed: \
                                            {} {:?}",
                                        e, e
                                    );
                                }
                            }
                        }
                    }
                }
            }
            CamArg::CamArgSetKalmanTrackingConfig(yaml_buf) => {
                #[cfg(feature = "flydratrax")]
                {
                    // parse buffer
                    match serde_yaml::from_str::<KalmanTrackingConfig>(&yaml_buf) {
                        Err(e) => {
                            error!("ignoring KalmanTrackingConfig with parse error: {:?}", e)
                        }
                        Ok(cfg) => {
                            let cfg2 = cfg.clone();
                            {
                                // Update config and send to frame process thread
                                let mut tracker = shared_store_arc.write().unwrap();
                                tracker.modify(|shared| {
                                    shared.kalman_tracking_config = cfg;
                                });
                            }
                            if let ImPtDetectCfgSource::ChangedSavedToDisk(ref src) =
                                tracker_cfg_src
                            {
                                let (app_info, _) = src;
                                match cfg2.save(app_info, KALMAN_TRACKING_PREFS_KEY) {
                                    Ok(()) => {
                                        info!("saved new kalman tracker config");
                                    }
                                    Err(e) => {
                                        error!(
                                            "saving kalman tracker config failed: \
                                                {} {:?}",
                                            e, e
                                        );
                                    }
                                }
                            } else {
                                panic!("flydratrax requires saving changes to disk");
                            }
                        }
                    }
                }
            }
            CamArg::CamArgSetLedProgramConfig(yaml_buf) => {
                #[cfg(feature = "flydratrax")]
                {
                    // parse buffer
                    match serde_yaml::from_str::<LedProgramConfig>(&yaml_buf) {
                        Err(e) => {
                            error!("ignoring LedProgramConfig with parse error: {:?}", e)
                        }
                        Ok(cfg) => {
                            let cfg2 = cfg.clone();
                            {
                                // Update config and send to frame process thread
                                let mut tracker = shared_store_arc.write().unwrap();
                                tracker.modify(|shared| {
                                    shared.led_program_config = cfg;
                                });
                            }
                            if let ImPtDetectCfgSource::ChangedSavedToDisk(ref src) =
                                tracker_cfg_src
                            {
                                let (app_info, _) = src;
                                match cfg2.save(app_info, LED_PROGRAM_PREFS_KEY) {
                                    Ok(()) => {
                                        info!("saved new LED program config");
                                    }
                                    Err(e) => {
                                        error!(
                                            "saving LED program config failed: \
                                                {} {:?}",
                                            e, e
                                        );
                                    }
                                }
                            } else {
                                panic!("flydratrax requires saving changes to disk");
                            }
                        }
                    }
                }
            }
            CamArg::ToggleCheckerboardDetection(val) => {
                #[cfg(feature = "checkercal")]
                {
                    let mut tracker = shared_store_arc.write().unwrap();
                    tracker.modify(|shared| {
                        shared.checkerboard_data.enabled = val;
                    });
                }
            }
            CamArg::ToggleCheckerboardDebug(val) => {
                #[cfg(feature = "checkercal")]
                {
                    let mut tracker = shared_store_arc.write().unwrap();
                    tracker.modify(|shared| {
                        if val {
                            if shared.checkerboard_save_debug.is_none() {
                                // start saving checkerboard data
                                let basedir = std::env::temp_dir();

                                let local: chrono::DateTime<chrono::Local> = chrono::Local::now();
                                let format_str = "checkerboard_debug_%Y%m%d_%H%M%S";
                                let stamped = local.format(format_str).to_string();
                                let dirname = basedir.join(stamped);
                                info!("Saving checkerboard debug data to: {}", dirname.display());
                                std::fs::create_dir_all(&dirname).unwrap();
                                shared.checkerboard_save_debug =
                                    Some(format!("{}", dirname.display()));
                            }
                        } else {
                            if shared.checkerboard_save_debug.is_some() {
                                // stop saving checkerboard data
                                info!("Stop saving checkerboard debug data.");
                                shared.checkerboard_save_debug = None;
                            }
                        }
                    });
                }
            }

            CamArg::SetCheckerboardWidth(val) => {
                #[cfg(feature = "checkercal")]
                {
                    let mut tracker = shared_store_arc.write().unwrap();
                    tracker.modify(|shared| {
                        shared.checkerboard_data.width = val;
                    });
                }
            }
            CamArg::SetCheckerboardHeight(val) => {
                #[cfg(feature = "checkercal")]
                {
                    let mut tracker = shared_store_arc.write().unwrap();
                    tracker.modify(|shared| {
                        shared.checkerboard_data.height = val;
                    });
                }
            }
            CamArg::ClearCheckerboards => {
                #[cfg(feature = "checkercal")]
                {
                    {
                        let mut collected_corners = collected_corners_arc.write().unwrap();
                        collected_corners.clear();
                    }

                    {
                        let mut tracker = shared_store_arc.write().unwrap();
                        tracker.modify(|shared| {
                            shared.checkerboard_data.num_checkerboards_collected = 0;
                        });
                    }
                }
            }

            CamArg::PerformCheckerboardCalibration => {
                #[cfg(feature = "checkercal")]
                {
                    info!("computing calibration");
                    let (n_rows, n_cols, checkerboard_save_debug) = {
                        let tracker = shared_store_arc.read().unwrap();
                        let shared = (*tracker).as_ref();
                        let n_rows = shared.checkerboard_data.height;
                        let n_cols = shared.checkerboard_data.width;
                        let checkerboard_save_debug = shared.checkerboard_save_debug.clone();
                        (n_rows, n_cols, checkerboard_save_debug)
                    };

                    let goodcorners: Vec<camcal::CheckerBoardData> = {
                        let collected_corners = collected_corners_arc.read().unwrap();
                        collected_corners
                            .iter()
                            .map(|corners| {
                                let x: Vec<(f64, f64)> =
                                    corners.iter().map(|x| (x.0 as f64, x.1 as f64)).collect();
                                camcal::CheckerBoardData::new(n_rows as usize, n_cols as usize, &x)
                            })
                            .collect()
                    };

                    let local: chrono::DateTime<chrono::Local> = chrono::Local::now();

                    if let Some(debug_dir) = &checkerboard_save_debug {
                        let format_str = format!(
                            "checkerboard_input_{}.%Y%m%d_%H%M%S.yaml",
                            cam_name2.as_str()
                        );
                        let stamped = local.format(&format_str).to_string();

                        let debug_path = std::path::PathBuf::from(debug_dir);
                        let corners_path = debug_path.join(stamped);

                        let f = File::create(&corners_path).expect("create file");

                        #[derive(Serialize)]
                        struct CornersData<'a> {
                            corners: &'a Vec<camcal::CheckerBoardData>,
                            image_width: u32,
                            image_height: u32,
                        }
                        let debug_data = CornersData {
                            corners: &goodcorners,
                            image_width,
                            image_height,
                        };
                        serde_yaml::to_writer(f, &debug_data).expect("serde_yaml::to_writer");
                    }

                    let size = camcal::PixelSize::new(image_width as usize, image_height as usize);

                    match camcal::compute_intrinsics_with_raw_opencv::<f64>(size, &goodcorners) {
                        Ok(raw_opencv_cal) => {
                            let cal_dir = directories::BaseDirs::new()
                                .as_ref()
                                .map(|bd| bd.config_dir().join(APP_INFO.name).join("camera_info"))
                                .unwrap();

                            if !cal_dir.exists() {
                                std::fs::create_dir_all(&cal_dir)?;
                            }

                            info!("Using calibration directory at \"{}\"", cal_dir.display());

                            let format_str =
                                format!("{}.%Y%m%d_%H%M%S.yaml", raw_cam_name.as_str());
                            let stamped = local.format(&format_str).to_string();
                            let cam_info_file_stamped = cal_dir.join(stamped);

                            let mut cam_info_file = cal_dir.clone();
                            cam_info_file.push(raw_cam_name.as_str());
                            cam_info_file.set_extension("yaml");

                            // Save timestamped version first for backup purposes (since below
                            // we overwrite the non-timestamped file).
                            camcal::save_yaml(
                                &cam_info_file_stamped,
                                env!["CARGO_PKG_NAME"],
                                local,
                                &raw_opencv_cal,
                                raw_cam_name.as_str(),
                            )?;

                            // Now copy the successfully saved file into
                            // the non-timestamped name. This will
                            // overwrite an existing file.
                            std::fs::copy(&cam_info_file_stamped, &cam_info_file)
                                .expect("copy file");

                            info!(
                                "Saved camera calibration to file: {}",
                                cam_info_file.display(),
                            );
                        }
                        Err(e) => {
                            error!("failed doing calibration {:?} {}", e, e);
                        }
                    };
                }
            }
        }
    }

    // We get here iff DoQuit (or a closed command channel) broke us out of the
    // infinite loop.

    // Tell every connected browser that we are shutting down, so that all
    // clients show the "Strand Camera has quit" screen and stop reconnecting,
    // not just the one that pressed Quit. This must happen here, while the HTTP
    // server is still running: once this task returns, the top-level `select!`
    // drops (cancels) the HTTP server future. The camera-stop sequence below
    // then gives the message time to flush to clients before that happens.
    event_broadcaster.broadcast_frame(quit_event_chunk()).await;

    // In theory, all things currently being saved should nicely stop themselves when dropped.
    // For now, while we are working on ctrlc handling, we manually stop them.
    tx_frame2.send(Msg::StopFMF).await.map_err(to_eyre)?;
    tx_frame2.send(Msg::StopMp4).await.map_err(to_eyre)?;
    #[cfg(feature = "flydra_feat_detect")]
    tx_frame2.send(Msg::StopUFMF).await.map_err(to_eyre)?;
    #[cfg(feature = "flydra_feat_detect")]
    tx_frame2
        .send(Msg::SetIsSavingObjDetectionCsv(CsvSaveConfig::NotSaving))
        .await
        .map_err(to_eyre)?;

    info!("attempting to nicely stop camera");
    match cam.control_and_join_handle() {
        Some((control, join_handle)) => {
            control.stop();
            while !control.is_done() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            info!("camera thread stopped");
            join_handle.join().expect("join camera thread");
            info!("camera thread joined");
        }
        _ => {
            error!("camera thread not running!?");
        }
    }

    info!("cam_args_rx future is resolved");

    Ok(())
}

/// Build the Server-Sent Events frame announcing that the server is quitting.
///
/// The data payload is unused by the frontend (the event name alone is the
/// signal), but SSE frames must carry a `data:` line.
fn quit_event_chunk() -> String {
    format!("event: {STRAND_CAM_QUIT_EVENT_NAME}\ndata: quit\n\n")
}
