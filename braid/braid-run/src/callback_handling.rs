// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

use axum::response::IntoResponse;
use tracing::{debug, error, warn};

use braid_types::{BraidHttpApiCallback, PerCamSaveData, RawCamName};
use event_stream_types::TolerantJson;
use http::StatusCode;
use strand_cam_bui_types::RecordingPath;

use crate::mainbrain::*;

/// Mirror the camera's image dimensions into the shared state shown in the
/// browser UI, which uses them to size the camera preview.
fn update_camera_image_dimensions(
    app_state: &BraidAppState,
    raw_cam_name: &RawCamName,
    current_image_png: &braid_types::PngImageData,
) {
    let Some(dimensions) = current_image_png.dimensions() else {
        warn!(
            "could not determine image dimensions for camera \"{}\"",
            raw_cam_name.as_str()
        );
        return;
    };
    let mut tracker = app_state.shared_store.write().unwrap();
    if (*tracker)
        .as_ref()
        .camera_image_dimensions
        .get(raw_cam_name)
        != Some(&dimensions)
    {
        tracker.modify(|store| {
            store
                .camera_image_dimensions
                .insert(raw_cam_name.clone(), dimensions);
        });
    }
}

fn start_saving_mp4s_all_cams(app_state: &BraidAppState, start_saving: bool) {
    let mut tracker = app_state.shared_store.write().unwrap();
    tracker.modify(|store| {
        if start_saving {
            store.fake_mp4_recording_path = Some(RecordingPath::new("".to_string()));
        } else {
            store.fake_mp4_recording_path = None;
        }
    });
}

pub(crate) async fn callback_handler(
    axum::extract::State(app_state): axum::extract::State<crate::mainbrain::BraidAppState>,
    session_key: axum_token_auth::SessionKey,
    TolerantJson(payload): TolerantJson<BraidHttpApiCallback>,
) -> impl IntoResponse {
    session_key.is_present();
    let fut = async {
        use BraidHttpApiCallback::*;
        match payload {
            NewCamera(cam_info) => {
                debug!("got NewCamera {:?}", cam_info.raw_cam_name.as_str());
                let http_camserver_info = cam_info.http_camserver_info.unwrap();
                let cam_settings_data = cam_info.cam_settings_data.unwrap();
                let camera_periodic_signal_period_usec =
                    cam_info.camera_periodic_signal_period_usec;
                let mut cam_manager3 = app_state.cam_manager.clone();
                cam_manager3
                    .register_new_camera(
                        &cam_info.raw_cam_name,
                        &http_camserver_info,
                        camera_periodic_signal_period_usec,
                    )
                    .map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;

                update_camera_image_dimensions(
                    &app_state,
                    &cam_info.raw_cam_name,
                    &cam_info.current_image_png,
                );

                let mut current_cam_data = app_state.per_cam_data_arc.write().unwrap();
                if current_cam_data
                    .insert(
                        cam_info.raw_cam_name.clone(),
                        PerCamSaveData {
                            cam_settings_data: Some(cam_settings_data),
                            feature_detect_settings: None,
                            current_image_png: cam_info.current_image_png,
                        },
                    )
                    .is_some()
                {
                    panic!("camera {} already known", cam_info.raw_cam_name.as_str());
                }
            }
            UpdateCurrentImage(image_info) => {
                // new image from camera
                debug!(
                    "got new image for camera \"{}\"",
                    image_info.raw_cam_name.as_str()
                );
                update_camera_image_dimensions(
                    &app_state,
                    &image_info.raw_cam_name,
                    &image_info.inner.current_image_png,
                );
                let mut current_cam_data = app_state.per_cam_data_arc.write().unwrap();
                current_cam_data
                    .get_mut(&image_info.raw_cam_name)
                    .unwrap()
                    .current_image_png = image_info.inner.current_image_png;
            }
            UpdateCamSettings(cam_settings) => {
                let mut current_cam_data = app_state.per_cam_data_arc.write().unwrap();
                current_cam_data
                    .get_mut(&cam_settings.raw_cam_name)
                    .unwrap()
                    .cam_settings_data = Some(cam_settings.inner);
            }
            UpdateFeatureDetectSettings(feature_detect_settings) => {
                let raw_cam_name = feature_detect_settings.raw_cam_name.clone();
                let do_update = feature_detect_settings
                    .inner
                    .current_feature_detect_settings
                    .do_update_background_model;
                {
                    let mut current_cam_data = app_state.per_cam_data_arc.write().unwrap();
                    current_cam_data
                        .get_mut(&raw_cam_name)
                        .unwrap()
                        .feature_detect_settings = Some(feature_detect_settings.inner);
                }
                // Mirror the per-camera background updating state into the
                // shared state shown in the browser UI.
                let mut tracker = app_state.shared_store.write().unwrap();
                if (*tracker)
                    .as_ref()
                    .background_model_updating
                    .get(&raw_cam_name)
                    != Some(&do_update)
                {
                    tracker.modify(|store| {
                        store
                            .background_model_updating
                            .insert(raw_cam_name, do_update);
                    });
                }
            }
            DoRecordCsvTables(value) => {
                debug!("got DoRecordCsvTables({})", value);
                toggle_saving_csv_tables(
                    value,
                    app_state.expected_framerate_arc.clone(),
                    app_state.output_base_dirname.clone(),
                    app_state.braidz_write_tx_weak.clone(),
                    app_state.per_cam_data_arc.clone(),
                    app_state.shared_store.clone(),
                )
                .await;
            }
            DoRecordMp4Files(start_saving) => {
                debug!("got DoRecordMp4Files({start_saving})");

                app_state
                    .strand_cam_http_session_handler
                    .toggle_saving_mp4_files_all(start_saving)
                    .await
                    .map_err(|_e| {
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "toggle_saving_mp4_files_all failed",
                        )
                    })?;

                start_saving_mp4s_all_cams(&app_state, start_saving);
            }
            SetExperimentUuid(value) => {
                debug!("got SetExperimentUuid({})", value);
                if let Some(braidz_write_tx) = app_state.braidz_write_tx_weak.upgrade() {
                    // `braidz_write_tx` will be dropped after this scope.
                    braidz_write_tx
                        .send(flydra2::SaveToDiskMsg::SetExperimentUuid(value))
                        .await
                        .unwrap();
                }
            }
            SetPostTriggerBufferSize(val) => {
                debug!("got SetPostTriggerBufferSize({val})");

                app_state
                    .strand_cam_http_session_handler
                    .set_post_trigger_buffer_all(val)
                    .await
                    .map_err(|_e| {
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "set_post_trigger_buffer_all failed",
                        )
                    })?;

                {
                    let mut tracker = app_state.shared_store.write().unwrap();
                    tracker.modify(|store| {
                        store.post_trigger_buffer_size = val;
                    });
                }
            }
            PostTriggerMp4Recording => {
                debug!("got PostTriggerMp4Recording");

                let is_saving = {
                    let tracker = app_state.shared_store.read().unwrap();
                    (*tracker).as_ref().fake_mp4_recording_path.is_some()
                };

                if !is_saving {
                    app_state
                        .strand_cam_http_session_handler
                        .initiate_post_trigger_mp4_all()
                        .await
                        .map_err(|_e| {
                            (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                "initiate_post_trigger_mp4_all failed",
                            )
                        })?;

                    start_saving_mp4s_all_cams(&app_state, true);
                } else {
                    debug!("Already saving, not initiating again.");
                }
            }
            DoTakeNewBackgroundImage => {
                debug!("got DoTakeNewBackgroundImage");
                app_state
                    .strand_cam_http_session_handler
                    .take_new_background_all()
                    .await
                    .map_err(|_e| {
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "take_new_background_all failed",
                        )
                    })?;
            }
            SetBackgroundUpdating(value) => {
                debug!("got SetBackgroundUpdating({value})");
                // Build the updated per-camera configuration from the most
                // recently received feature detection settings of each camera.
                // (Collected first so no lock is held across `await`.)
                let per_cam_cfgs: Vec<(RawCamName, String)> = {
                    let current_cam_data = app_state.per_cam_data_arc.read().unwrap();
                    current_cam_data
                        .iter()
                        .filter_map(|(cam_name, cam_data)| {
                            match &cam_data.feature_detect_settings {
                                Some(settings) => {
                                    let mut cfg = settings.current_feature_detect_settings.clone();
                                    cfg.do_update_background_model = value;
                                    match serde_yaml::to_string(&cfg) {
                                        Ok(yaml) => Some((cam_name.clone(), yaml)),
                                        Err(e) => {
                                            error!(
                                                "serializing object detection config for \
                                                \"{}\": {e}",
                                                cam_name.as_str()
                                            );
                                            None
                                        }
                                    }
                                }
                                None => {
                                    warn!(
                                        "not setting background updating for camera \"{}\": \
                                        no feature detection settings received yet",
                                        cam_name.as_str()
                                    );
                                    None
                                }
                            }
                        })
                        .collect()
                };
                for (cam_name, cfg_yaml) in per_cam_cfgs {
                    app_state
                        .strand_cam_http_session_handler
                        .send_obj_detection_config(&cam_name, cfg_yaml)
                        .await
                        .map_err(|_e| {
                            (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                "send_obj_detection_config failed",
                            )
                        })?;
                }
            }
        }
        Ok::<_, (StatusCode, &'static str)>(())
    };
    fut.await
}
