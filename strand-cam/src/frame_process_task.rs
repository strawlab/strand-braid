use eyre::Result;
#[cfg(feature = "fiducial")]
use libflate::{finish::AutoFinishUnchecked, gzip::Encoder};

#[cfg(feature = "checkercal")]
use machine_vision_formats as formats;
use serde::{Deserialize, Serialize};
#[cfg(feature = "flydra_feat_detect")]
use std::io::Write;
use std::{fs::File, path::Path, sync::Arc};
use tracing::{debug, error, info, trace};

use async_change_tracker::ChangeTracker;
use basic_frame::{match_all_dynamic_fmts, DynamicFrame};
use flydra_feature_detector_types::ImPtDetectCfg;
use flydra_types::{
    FlydraFloatTimestampLocal, PtpStamp, RawCamName, RealtimePointsDestAddr, TriggerType,
};
use fmf::FMFWriter;
use http_video_streaming::AnnotatedFrame;
use rust_cam_bui_types::RecordingPath;

use strand_cam_storetype::StoreType;
use timestamped_frame::ExtraTimeData;

#[cfg(feature = "fiducial")]
use ads_apriltag as apriltag;

use crate::{
    convert_stream, open_braid_destination_addr, post_trigger_buffer, video_streaming,
    FinalMp4RecordingConfig, FmfWriteInfo, FpsCalc, MomentCentroid, Msg, TimestampSource, ToDevice,
    LED_BOX_HEARTBEAT_INTERVAL_MSEC, MOMENT_CENTROID_SCHEMA_VERSION,
};

/// Perform image analysis
pub(crate) async fn frame_process_task(
    #[cfg(feature = "flydratrax")] model_server_data_tx: tokio::sync::mpsc::Sender<(
        flydra2::SendType,
        flydra2::TimeDataPassthrough,
    )>,
    cam_name: RawCamName,
    #[cfg(feature = "flydra_feat_detect")]
    camera_cfg: strand_cam_csv_config_types::CameraCfgFview2_0_26,
    #[cfg(feature = "flydra_feat_detect")] width: u32,
    #[cfg(feature = "flydra_feat_detect")] height: u32,
    mut incoming_frame_rx: tokio::sync::mpsc::Receiver<Msg>,
    #[cfg(feature = "flydra_feat_detect")] im_pt_detect_cfg: ImPtDetectCfg,
    #[cfg(feature = "flydra_feat_detect")] csv_save_pathbuf: std::path::PathBuf,
    firehose_tx: tokio::sync::mpsc::Sender<AnnotatedFrame>,
    #[cfg(feature = "plugin-process-frame")] plugin_handler_thread_tx: channellib::Sender<
        DynamicFrame,
    >,
    #[cfg(feature = "plugin-process-frame")] plugin_result_rx: channellib::Receiver<
        Vec<http_video_streaming_types::Point>,
    >,
    #[cfg(feature = "plugin-process-frame")] plugin_wait_dur: std::time::Duration,
    #[cfg(feature = "flydratrax")] led_box_tx_std: tokio::sync::mpsc::Sender<crate::ToLedBoxDevice>,
    #[cfg(feature = "flydratrax")] http_camserver_info: flydra_types::BuiServerAddrInfo,
    transmit_msg_tx: Option<tokio::sync::mpsc::Sender<flydra_types::BraidHttpApiCallback>>,
    camdata_addr: Option<RealtimePointsDestAddr>,
    led_box_heartbeat_update_arc: Arc<parking_lot::RwLock<Option<std::time::Instant>>>,
    #[cfg(feature = "plugin-process-frame")] do_process_frame_callback: bool,
    #[cfg(feature = "checkercal")] collected_corners_arc: crate::CollectedCornersArc,
    #[cfg(feature = "flydratrax")] args: &crate::StrandCamArgs,
    #[cfg(feature = "flydra_feat_detect")] acquisition_duration_allowed_imprecision_msec: Option<
        f64,
    >,
    frame_info_extractor: &dyn ci2::ExtractFrameInfo,
    #[cfg(feature = "flydra_feat_detect")] app_name: &'static str,
    device_clock_model: Option<rust_cam_bui_types::ClockModel>,
    local_and_cam_time0: Option<(u64, u64)>,
    trigger_type: Option<TriggerType>,
) -> Result<()> {
    // As currently implemented, this function has a problem: it does
    // potentially computationally expensive image processing and thus should
    // theoretically not be async but rather this processing should be offloaded
    // to a synchronous worker thread. However, especially with the "flydratrax"
    // stuff, there is also a lot of IO which is (and should be) async.
    let my_runtime: tokio::runtime::Handle = tokio::runtime::Handle::current();

    let is_braid = camdata_addr.is_some();

    let raw_cam_name: RawCamName = cam_name.clone();

    #[cfg(feature = "flydratrax")]
    let mut maybe_flydra2_stream = None;
    #[cfg(feature = "flydratrax")]
    let mut opt_braidz_write_tx_weak = None;

    #[cfg_attr(not(feature = "flydra_feat_detect"), allow(dead_code))]
    struct CsvSavingState {
        fd: File,
        min_interval: chrono::Duration,
        last_save: chrono::DateTime<chrono::Utc>,
        t0: chrono::DateTime<chrono::Utc>,
    }

    // CSV saving
    #[cfg_attr(not(feature = "flydra_feat_detect"), allow(dead_code))]
    enum SavingState {
        NotSaving,
        Starting(Option<f32>),
        Saving(CsvSavingState),
    }

    #[cfg(feature = "fiducial")]
    let mut apriltag_writer: Option<_> = None;
    let mut my_mp4_writer: Option<bg_movie_writer::BgMovieWriter> = None;
    let mut fmf_writer: Option<FmfWriteInfo<_>> = None;
    #[cfg(feature = "flydra_feat_detect")]
    let mut ufmf_state = Some(flydra_feature_detector::UfmfState::Stopped);
    #[cfg(feature = "flydra_feat_detect")]
    #[allow(unused_assignments)]
    let mut is_doing_object_detection = is_braid;

    #[cfg(feature = "flydra_feat_detect")]
    let frame_offset = if is_braid {
        // We start initially unsynchronized. We wait for synchronizaton.
        None
    } else {
        Some(0)
    };

    let transmit_feature_detect_settings_tx = if is_braid {
        let (transmit_feature_detect_settings_tx, transmit_feature_detect_settings_rx) =
            tokio::sync::mpsc::channel::<ImPtDetectCfg>(10);

        let transmit_msg_tx = transmit_msg_tx.unwrap();

        my_runtime.spawn(convert_stream(
            raw_cam_name.clone(),
            transmit_feature_detect_settings_rx,
            transmit_msg_tx,
        ));

        Some(transmit_feature_detect_settings_tx)
    } else {
        None
    };

    #[cfg(not(feature = "flydra_feat_detect"))]
    std::mem::drop(transmit_feature_detect_settings_tx);

    #[cfg(not(feature = "flydra_feat_detect"))]
    debug!("Not using FlydraFeatureDetector.");

    let coord_socket = if let Some(camdata_addr) = camdata_addr {
        // If `camdata_addr` is not None, it is used to set open a socket to send
        // the detected feature information.
        debug!("sending tracked points to {:?}", camdata_addr);
        Some(open_braid_destination_addr(&camdata_addr)?)
    } else {
        debug!("Not sending tracked points to braid.");
        None
    };

    #[cfg(feature = "flydra_feat_detect")]
    let mut im_tracker = flydra_feature_detector::FlydraFeatureDetector::new(
        &cam_name,
        width,
        height,
        im_pt_detect_cfg.clone(),
        frame_offset,
        transmit_feature_detect_settings_tx,
        acquisition_duration_allowed_imprecision_msec,
    )?;
    #[cfg(feature = "flydra_feat_detect")]
    let mut csv_save_state = SavingState::NotSaving;
    let mut shared_store_arc: Option<Arc<parking_lot::RwLock<ChangeTracker<StoreType>>>> = None;
    let mut fps_calc = FpsCalc::new(100); // average 100 frames to get mean fps
    #[cfg(feature = "flydratrax")]
    let mut kalman_tracking_config = strand_cam_storetype::KalmanTrackingConfig::default(); // this is replaced below
    #[cfg(feature = "flydratrax")]
    let mut led_program_config;
    #[cfg(feature = "flydratrax")]
    let mut led_state = false;
    #[cfg(feature = "flydratrax")]
    let mut current_flydra_config_state = None;
    #[cfg(feature = "flydratrax")]
    let mut dirty_flydra = false;
    #[cfg(feature = "flydratrax")]
    let mut current_led_program_config_state: Option<strand_cam_storetype::LedProgramConfig> = None;

    #[cfg(feature = "flydratrax")]
    let red_style = http_video_streaming_types::StrokeStyle::from_rgb(255, 100, 100);

    let expected_framerate_arc = Arc::new(parking_lot::RwLock::new(None));

    let mut post_trig_buffer = post_trigger_buffer::PostTriggerBuffer::new();

    #[cfg(feature = "fiducial")]
    let mut april_td = apriltag::Detector::new();

    #[cfg(feature = "fiducial")]
    let mut current_tag_family = ci2_remote_control::TagFamily::default();
    #[cfg(feature = "fiducial")]
    let april_tf = make_family(&current_tag_family);
    #[cfg(feature = "fiducial")]
    april_td.add_family(april_tf);

    #[cfg(feature = "checkercal")]
    let mut last_checkerboard_detection = std::time::Instant::now();

    // This limits the frequency at which the checkerboard detection routine is
    // called. This is meant to both prevent flooding the calibration routine
    // with many highly similar checkerboard images and also to allow the image
    // processing thread to keep a low queue depth on incoming frames. In the
    // current form here, however, keeping a low queue depth is dependent on the
    // checkerboard detection function returning fairly quickly. I have observed
    // the OpenCV routine taking ~90 seconds even though usually it takes 100
    // msec. Thus, this requirement is not always met. We could move this
    // checkerboard detection routine to a different thread (e.g. using a tokio
    // work pool) to avoid this problem.
    #[cfg(feature = "checkercal")]
    let mut checkerboard_loop_dur = std::time::Duration::from_millis(500);

    // let current_image_timer_arc = Arc::new(parking_lot::RwLock::new(std::time::Instant::now()));

    let mut im_ops_socket: Option<std::net::UdpSocket> = None;

    let mut triggerbox_clock_model = None;
    let mut opt_frame_offset = None;

    loop {
        #[cfg(feature = "flydra_feat_detect")]
        {
            if let Some(ref ssa) = shared_store_arc {
                if let Some(store) = ssa.try_read() {
                    let tracker = store.as_ref();
                    is_doing_object_detection = tracker.is_doing_object_detection;
                    // make copy. TODO only copy on change.
                }
            }
        }

        #[cfg(feature = "flydratrax")]
        {
            if dirty_flydra {
                // stop flydra if things changed, will be restarted on next frame.
                is_doing_object_detection = false;
                current_flydra_config_state = None;
                dirty_flydra = false;
            }

            let kalman_tracking_enabled = if let Some(ref ssa) = shared_store_arc {
                let tracker = ssa.read();
                tracker.as_ref().kalman_tracking_config.enabled
            } else {
                false
            };

            // start kalman tracking if we are doing object detection but not kalman tracking yet
            // TODO if kalman_tracking_config or
            // im_pt_detect_cfg.valid_region changes, restart tracker.
            if is_doing_object_detection && maybe_flydra2_stream.is_none() {
                let mut new_cam = None;
                if let Some(ref ssa) = shared_store_arc {
                    let region = {
                        let tracker = ssa.read();
                        kalman_tracking_config = tracker.as_ref().kalman_tracking_config.clone();
                        led_program_config = tracker.as_ref().led_program_config.clone();
                        tracker.as_ref().im_pt_detect_cfg.valid_region.clone()
                    };
                    if kalman_tracking_enabled {
                        current_flydra_config_state = Some(FlydraConfigState {
                            region: region.clone(),
                            kalman_tracking_config: kalman_tracking_config.clone(),
                        });
                        current_led_program_config_state = Some(led_program_config.clone());
                        match region {
                            video_streaming::Shape::Polygon(_points) => {
                                unimplemented!();
                            }
                            video_streaming::Shape::MultipleCircles(_) => {
                                unimplemented!();
                            }
                            video_streaming::Shape::Circle(circ) => {
                                let recon = match &args.flydratrax_calibration_source {
                                    crate::CalSource::PseudoCal => {
                                        let cal_data =
                                            strand_cam_pseudo_cal::PseudoCameraCalibrationData {
                                                cam_name: cam_name.clone(),
                                                width,
                                                height,
                                                physical_diameter_meters: kalman_tracking_config
                                                    .arena_diameter_meters,
                                                image_circle: circ,
                                            };
                                        cal_data.to_camera_system()?
                                    }
                                    crate::CalSource::XmlFile(cal_fname) => {
                                        let rdr = std::fs::File::open(&cal_fname)?;
                                        flydra_mvg::FlydraMultiCameraSystem::from_flydra_xml(rdr)?
                                    }
                                    crate::CalSource::PymvgJsonFile(cal_fname) => {
                                        let rdr = std::fs::File::open(&cal_fname)?;
                                        let sys = mvg::MultiCameraSystem::from_pymvg_json(rdr)?;
                                        flydra_mvg::FlydraMultiCameraSystem::from_system(sys, None)
                                    }
                                };

                                let (flydra2_tx, flydra2_rx) = futures::channel::mpsc::channel(100);

                                let (model_sender, model_receiver) =
                                    tokio::sync::mpsc::channel(100);

                                let led_box_tx_std2 = led_box_tx_std.clone();
                                let ssa2 = ssa.clone();

                                assert_eq!(recon.len(), 1); // TODO: check if camera name in system and allow that?
                                let cam_cal = recon.cameras().next().unwrap().to_cam();
                                new_cam = Some(cam_cal.clone());

                                let msg_handler_fut = async move {
                                    crate::flydratrax_handle_msg::create_message_handler(
                                        cam_cal,
                                        model_receiver,
                                        &mut led_state,
                                        ssa2,
                                        led_box_tx_std2,
                                    )
                                    .await
                                    .unwrap();
                                };
                                let msg_handler_jh = my_runtime.spawn(msg_handler_fut);

                                let expected_framerate_arc2 = expected_framerate_arc.clone();
                                let cam_name2 = cam_name.clone();
                                let http_camserver = flydra_types::BuiServerInfo::Server(
                                    http_camserver_info.clone(),
                                );
                                let recon2 = recon.clone();
                                let model_server_data_tx2 = model_server_data_tx.clone();

                                let cam_manager = flydra2::ConnectedCamerasManager::new_single_cam(
                                    &cam_name2,
                                    &http_camserver,
                                    &Some(recon2),
                                    None,
                                );
                                let tracking_params =
                                    flydra_types::default_tracking_params_flat_3d();
                                let ignore_latency = false;
                                let mut coord_processor = flydra2::CoordProcessor::new(
                                    flydra2::CoordProcessorConfig {
                                        tracking_params,
                                        save_empty_data2d: args.save_empty_data2d,
                                        ignore_latency,
                                        mini_arena_debug_image_dir: None,
                                        write_buffer_size_num_messages: args
                                            .write_buffer_size_num_messages,
                                    },
                                    cam_manager,
                                    Some(recon),
                                    flydra2::BraidMetadataBuilder::saving_program_name(
                                        "strand-cam",
                                    ),
                                )
                                .expect("create CoordProcessor");

                                let braidz_write_tx_weak =
                                    coord_processor.braidz_write_tx.downgrade();

                                opt_braidz_write_tx_weak = Some(braidz_write_tx_weak);

                                let model_server_data_tx = model_server_data_tx2;

                                coord_processor.add_listener(model_sender); // the local LED control thing
                                coord_processor.add_listener(model_server_data_tx); // the HTTP thing

                                let expected_framerate = *expected_framerate_arc2.read();
                                let consume_future =
                                    coord_processor.consume_stream(flydra2_rx, expected_framerate);

                                let flydra_jh = my_runtime.spawn(async {
                                    // Run until flydra is done.
                                    let jh = consume_future.await.unwrap();

                                    debug!(
                                        "waiting on flydratrax coord processor {}:{}",
                                        file!(),
                                        line!()
                                    );
                                    jh.await.unwrap().unwrap();
                                    debug!(
                                        "done waiting on flydratrax coord processor {}:{}",
                                        file!(),
                                        line!()
                                    );
                                });
                                maybe_flydra2_stream = Some(flydra2_tx);
                                std::mem::drop((msg_handler_jh, flydra_jh)); // todo: keep these join handles.
                            }
                            video_streaming::Shape::Everything => {
                                error!("cannot start tracking without circular region to use as camera calibration");
                            }
                        }
                    }
                }
                if let Some(cam) = new_cam {
                    if let Some(ref mut store) = shared_store_arc {
                        let mut tracker = store.write();
                        tracker.modify(|tracker| {
                            tracker.camera_calibration = Some(cam);
                        });
                    }
                }
            }

            if !is_doing_object_detection | !kalman_tracking_enabled {
                // drop all flydra2 stuff if we are not tracking
                maybe_flydra2_stream = None;
                if let Some(braidz_write_tx_weak) = opt_braidz_write_tx_weak.take() {
                    if let Some(braidz_write_tx) = braidz_write_tx_weak.upgrade() {
                        // `braidz_write_tx` will be dropped after this scope.
                        match braidz_write_tx
                            .send(flydra2::SaveToDiskMsg::StopSavingCsv)
                            .await
                        {
                            Ok(()) => {}
                            Err(_) => {
                                info!("Channel to data writing task closed. Ending.");
                                break;
                            }
                        }
                    }
                }
            }
        }

        let msg = match incoming_frame_rx.recv().await {
            Some(msg) => msg,
            None => {
                info!("incoming frame channel closed for '{}'", cam_name.as_str());
                break;
            }
        };
        let store_cache = if let Some(ref ssa) = shared_store_arc {
            let tracker = ssa.read();
            Some(tracker.as_ref().clone())
        } else {
            None
        };

        if let Some(ref store_cache_ref) = store_cache {
            #[cfg(not(feature = "flydratrax"))]
            let _ = store_cache_ref;
            #[cfg(feature = "flydratrax")]
            {
                if let Some(ref cfcs) = current_flydra_config_state {
                    if store_cache_ref.kalman_tracking_config != cfcs.kalman_tracking_config {
                        dirty_flydra = true;
                    }
                    if store_cache_ref.im_pt_detect_cfg.valid_region != cfcs.region {
                        dirty_flydra = true;
                    }
                }
                if let Some(ref clpcs) = current_led_program_config_state {
                    if &store_cache_ref.led_program_config != clpcs {
                        current_led_program_config_state =
                            Some(store_cache_ref.led_program_config.clone());
                    }
                }
            }
        }

        match msg {
            Msg::Store(stor) => {
                // We get the shared store once at startup.
                if is_braid {
                    let mut tracker = stor.write();
                    tracker.modify(|tracker| {
                        tracker.is_doing_object_detection = true;
                    });
                }
                {
                    let tracker = stor.read();
                    let shared = tracker.as_ref();
                    post_trig_buffer.set_size(shared.post_trigger_buffer_size);
                }
                shared_store_arc = Some(stor);
            }
            Msg::StartFMF((dest, recording_framerate)) => {
                let path = Path::new(&dest);
                let f = std::fs::File::create(path)?;
                fmf_writer = Some(FmfWriteInfo::new(FMFWriter::new(f)?, recording_framerate));
            }
            #[cfg(feature = "flydra_feat_detect")]
            Msg::StartUFMF(dest) => {
                ufmf_state = Some(flydra_feature_detector::UfmfState::Starting(dest));
            }
            Msg::StartMp4 | Msg::PostTriggerStartMp4 => {
                // get buffer of accumulated frames
                let frames = match msg {
                    Msg::PostTriggerStartMp4 => post_trig_buffer.get_and_clear(),
                    Msg::StartMp4 => std::collections::VecDeque::with_capacity(0),
                    _ => unreachable!(),
                };

                let local = chrono::Local::now();

                // Get start time, either from buffered frames if present or current time.
                let creation_time = if let Some(frame0) = frames.front() {
                    frame0.extra().host_timestamp().into()
                } else {
                    local
                };

                let (format_str_mp4, mp4_recording_config) = {
                    // scope for reading cache
                    let tracker = shared_store_arc.as_ref().unwrap().read();
                    let shared: &StoreType = tracker.as_ref();

                    let mp4_recording_config = FinalMp4RecordingConfig::new(shared, creation_time);

                    (shared.format_str_mp4.clone(), mp4_recording_config)
                };

                let filename = creation_time.format(format_str_mp4.as_str()).to_string();
                let is_recording_mp4 = Some(RecordingPath::new(filename.clone()));

                let mut raw = bg_movie_writer::BgMovieWriter::new_mp4_writer(
                    format_str_mp4,
                    mp4_recording_config.final_cfg,
                    frames.len() + 100,
                );
                for mut frame in frames.into_iter() {
                    // Force frame width to be power of 2.
                    let val = 2;
                    let clipped_width = (frame.width() / val as u32) * val as u32;
                    match_all_dynamic_fmts!(&mut frame, x, { x.width = clipped_width });
                    // frame.width = clipped_width;
                    let ts = frame.extra().host_timestamp();
                    raw.write(frame, ts)?;
                }
                my_mp4_writer = Some(raw);

                if let Some(ref mut store) = shared_store_arc {
                    let mut tracker = store.write();
                    tracker.modify(|tracker| {
                        tracker.is_recording_mp4 = is_recording_mp4;
                    });
                }
            }
            Msg::StartAprilTagRec(format_str_apriltags_csv) => {
                #[cfg(feature = "fiducial")]
                {
                    if let Some(x) = store_cache.as_ref() {
                        apriltag_writer = Some(AprilTagWriter::new(
                            format_str_apriltags_csv,
                            &x.camera_name,
                            x.image_width as usize,
                            x.image_height as usize,
                        )?);
                    }
                }
                #[cfg(not(feature = "fiducial"))]
                let _ = format_str_apriltags_csv;
            }
            Msg::StopAprilTagRec => {
                #[cfg(feature = "fiducial")]
                {
                    apriltag_writer = None;
                }
            }
            Msg::SetPostTriggerBufferSize(size) => {
                post_trig_buffer.set_size(size);
                if let Some(ref mut store) = shared_store_arc {
                    let mut tracker = store.write();
                    tracker.modify(|tracker| {
                        tracker.post_trigger_buffer_size = size;
                    });
                }
            }
            Msg::Mframe(frame) => {
                let extracted_frame_info = frame_info_extractor.extract_frame_info(&frame);
                let device_timestamp = extracted_frame_info.device_timestamp;
                tracing::trace!("device_timestamp: {device_timestamp:?}");
                let block_id = extracted_frame_info.frame_id;

                // Compute, as cleverly as possible, a timestamp.
                let braid_ts = match &trigger_type {
                    Some(TriggerType::TriggerboxV1(_)) | Some(TriggerType::FakeSync(_)) => {
                        flydra_types::triggerbox_time(
                            triggerbox_clock_model.as_ref(),
                            opt_frame_offset,
                            extracted_frame_info.host_framenumber,
                        )
                    }
                    Some(TriggerType::PtpSync(ptpcfg)) => {
                        let ptp_stamp = PtpStamp::new(device_timestamp.unwrap().get());
                        if tracing::Level::TRACE <= tracing::level_filters::STATIC_MAX_LEVEL {
                            // Only run run this block if we compiled with
                            // trace-level logging enabled.
                            if let Some(periodic_signal_period_usec) =
                                &ptpcfg.periodic_signal_period_usec
                            {
                                let nanos = ptp_stamp.get();
                                let fno_f64 = nanos as f64 / periodic_signal_period_usec * 1000.0;
                                let device_timestamp_chrono =
                                    chrono::DateTime::<chrono::Utc>::try_from(ptp_stamp.clone())
                                        .unwrap();
                                tracing::trace!(
                                    "fno_f64: {fno_f64}, device_timestamp_chrono: {device_timestamp_chrono}"
                                );
                            }
                        }
                        Some(ptp_stamp.try_into().unwrap())
                    }
                    Some(TriggerType::DeviceTimestamp) => {
                        let cm = device_clock_model.as_ref().unwrap();
                        let this_local_and_cam_time0 = local_and_cam_time0.as_ref().unwrap();
                        let (local_time0, cam_time0) = this_local_and_cam_time0;
                        let device_timestamp = device_timestamp.unwrap().get();
                        let device_elapsed_nanos = device_timestamp - cam_time0;
                        let local_elapsed_nanos: f64 =
                            (device_elapsed_nanos as f64) * cm.gain + cm.offset;
                        // let ts: f64 = (device_timestamp as f64) * cm.gain + cm.offset;
                        dbg!((local_elapsed_nanos, device_timestamp, &cm));

                        let local_nanos = local_time0 + local_elapsed_nanos.round() as u64;
                        let local: chrono::DateTime<chrono::Utc> =
                            PtpStamp::new(local_nanos).try_into().unwrap();
                        let x = FlydraFloatTimestampLocal::<flydra_types::Triggerbox>::from(local);
                        dbg!(&x);
                        Some(x)
                    }
                    None => None,
                };
                let (timestamp_source, save_mp4_fmf_stamp) = if let Some(stamp) = &braid_ts {
                    (TimestampSource::BraidTrigger, stamp.into())
                } else {
                    (
                        TimestampSource::HostAcquiredTimestamp,
                        extracted_frame_info.host_timestamp,
                    )
                };

                if let Some(new_fps) = fps_calc.update(&extracted_frame_info) {
                    if let Some(ref mut store) = shared_store_arc {
                        let mut tracker = store.write();
                        tracker.modify(|tracker| {
                            tracker.measured_fps = new_fps as f32;
                        });
                    }

                    {
                        let mut expected_framerate = expected_framerate_arc.write();
                        *expected_framerate = Some(new_fps as f32);
                    }
                }

                post_trig_buffer.push(&frame); // If buffer size larger than 0, copies data.

                #[cfg(feature = "checkercal")]
                let checkercal_tmp = store_cache.as_ref().and_then(|x| {
                    if x.checkerboard_data.enabled {
                        Some((
                            x.checkerboard_data.clone(),
                            x.checkerboard_save_debug.clone(),
                        ))
                    } else {
                        None
                    }
                });

                #[cfg(not(feature = "checkercal"))]
                let checkercal_tmp: Option<()> = None;

                #[allow(unused_mut)]
                let (mut found_points, valid_display) = if let Some(inner) = checkercal_tmp {
                    #[allow(unused_mut)]
                    let mut results = Vec::new();
                    #[cfg(not(feature = "checkercal"))]
                    #[allow(clippy::let_unit_value)]
                    let _ = inner;
                    #[cfg(feature = "checkercal")]
                    {
                        let (checkerboard_data, checkerboard_save_debug) = inner;

                        // do not do this too often
                        if last_checkerboard_detection.elapsed() > checkerboard_loop_dur {
                            let debug_image_stamp: chrono::DateTime<chrono::Local> =
                                chrono::Local::now();
                            if let Some(debug_dir) = &checkerboard_save_debug {
                                let format_str = format!(
                                    "input_{}_{}_%Y%m%d_%H%M%S.png",
                                    checkerboard_data.width, checkerboard_data.height
                                );
                                let stamped = debug_image_stamp.format(&format_str).to_string();
                                let png_buf = match_all_dynamic_fmts!(&frame, x, {
                                    convert_image::frame_to_image(
                                        x,
                                        convert_image::ImageOptions::Png,
                                    )?
                                });

                                let debug_path = std::path::PathBuf::from(debug_dir);
                                let image_path = debug_path.join(stamped);

                                let mut f = File::create(&image_path).expect("create file");
                                f.write_all(&png_buf).unwrap();
                            }

                            let start_time = std::time::Instant::now();

                            info!(
                                "Attempting to find {}x{} chessboard.",
                                checkerboard_data.width, checkerboard_data.height
                            );

                            let corners = basic_frame::match_all_dynamic_fmts!(&frame, x, {
                                let rgb: Box<
                                    dyn formats::ImageStride<formats::pixel_format::RGB8>,
                                > = Box::new(convert_image::convert::<
                                    _,
                                    formats::pixel_format::RGB8,
                                >(x)?);
                                let corners = opencv_calibrate::find_chessboard_corners(
                                    rgb.image_data(),
                                    rgb.width(),
                                    rgb.height(),
                                    checkerboard_data.width as usize,
                                    checkerboard_data.height as usize,
                                )?;
                                corners
                            });

                            let work_duration = start_time.elapsed();
                            if work_duration > checkerboard_loop_dur {
                                checkerboard_loop_dur =
                                    work_duration + std::time::Duration::from_millis(5);
                            }
                            last_checkerboard_detection = std::time::Instant::now();

                            debug!("corners: {:?}", corners);

                            if let Some(debug_dir) = &checkerboard_save_debug {
                                let format_str = "input_%Y%m%d_%H%M%S.yaml";
                                let stamped = debug_image_stamp.format(&format_str).to_string();

                                let debug_path = std::path::PathBuf::from(debug_dir);
                                let yaml_path = debug_path.join(stamped);

                                let mut f = File::create(&yaml_path).expect("create file");

                                #[derive(Serialize)]
                                struct CornerData<'a> {
                                    corners: &'a Option<Vec<(f32, f32)>>,
                                    work_duration: std::time::Duration,
                                }
                                let debug_data = CornerData {
                                    corners: &corners,
                                    work_duration,
                                };

                                serde_yaml::to_writer(f, &debug_data)
                                    .expect("serde_yaml::to_writer");
                            }

                            if let Some(corners) = corners {
                                info!(
                                    "Found {} chessboard corners in {} msec.",
                                    corners.len(),
                                    work_duration.as_millis()
                                );
                                results = corners
                                    .iter()
                                    .map(|(x, y)| video_streaming::Point {
                                        x: *x,
                                        y: *y,
                                        theta: None,
                                        area: None,
                                    })
                                    .collect();

                                let num_checkerboards_collected = {
                                    let mut collected_corners = collected_corners_arc.write();
                                    collected_corners.push(corners);
                                    collected_corners.len().try_into().unwrap()
                                };

                                if let Some(ref ssa) = shared_store_arc {
                                    // scope for write lock on ssa
                                    let mut tracker = ssa.write();
                                    tracker.modify(|shared| {
                                        shared.checkerboard_data.num_checkerboards_collected =
                                            num_checkerboards_collected;
                                    });
                                }
                            } else {
                                info!(
                                    "Found no chessboard corners in {} msec.",
                                    work_duration.as_millis()
                                );
                            }
                        }
                    }
                    (results, None)
                } else {
                    let mut all_points = Vec::new();
                    let mut blkajdsfads = None;

                    {
                        if let Some(ref store_cache_ref) = store_cache {
                            if store_cache_ref.im_ops_state.do_detection {
                                let thresholded = if let DynamicFrame::Mono8(mono8) = &frame {
                                    imops::threshold(
                                        mono8.clone(),
                                        imops::CmpOp::LessThan,
                                        store_cache_ref.im_ops_state.threshold,
                                        0,
                                        255,
                                    )
                                } else {
                                    panic!("imops only implemented for Mono8 pixel format");
                                };
                                let mu00 = imops::spatial_moment_00(&thresholded);
                                let mu01 = imops::spatial_moment_01(&thresholded);
                                let mu10 = imops::spatial_moment_10(&thresholded);
                                let mc = if mu00 != 0.0 {
                                    let x = mu10 / mu00;
                                    let y = mu01 / mu00;

                                    // If mu00 is 0.0, these will be NaN. CBOR explicitly can represent NaNs.

                                    let mc = ToDevice::Centroid(MomentCentroid {
                                        schema_version: MOMENT_CENTROID_SCHEMA_VERSION,
                                        framenumber: block_id.unwrap().get().try_into()?,
                                        timestamp: save_mp4_fmf_stamp,
                                        timestamp_source,
                                        mu00,
                                        mu01,
                                        mu10,
                                        center_x: store_cache_ref.im_ops_state.center_x,
                                        center_y: store_cache_ref.im_ops_state.center_y,
                                        cam_name: cam_name.as_str().to_string(),
                                    });
                                    all_points.push(video_streaming::Point {
                                        x,
                                        y,
                                        area: None,
                                        theta: None,
                                    });

                                    Some(mc)
                                } else {
                                    None
                                };

                                let need_new_socket = if let Some(socket) = &im_ops_socket {
                                    socket.local_addr().unwrap().ip()
                                        != store_cache_ref.im_ops_state.source
                                } else {
                                    true
                                };

                                if need_new_socket {
                                    let mut iter = std::net::ToSocketAddrs::to_socket_addrs(&(
                                        store_cache_ref.im_ops_state.source,
                                        0u16,
                                    ))
                                    .unwrap();
                                    let sockaddr = iter.next().unwrap();

                                    im_ops_socket = std::net::UdpSocket::bind(sockaddr)
                                        .map_err(|e| {
                                            error!("failed opening socket: {}", e);
                                        })
                                        .ok();
                                }

                                if let Some(socket) = &mut im_ops_socket {
                                    if let Some(mc) = mc {
                                        let buf = serde_cbor::to_vec(&mc).unwrap();
                                        match socket
                                            .send_to(&buf, store_cache_ref.im_ops_state.destination)
                                        {
                                            Ok(_n_bytes) => {}
                                            Err(e) => {
                                                error!("Unable to send image moment data. {}", e);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    #[cfg(feature = "fiducial")]
                    {
                        if let Some(ref store_cache_ref) = store_cache {
                            if let Some(ref ts) = store_cache_ref.apriltag_state {
                                if ts.do_detection {
                                    use apriltag::ImageU8;

                                    if current_tag_family != ts.april_family {
                                        april_td.clear_families();
                                        current_tag_family = ts.april_family.clone();
                                        let april_tf = make_family(&current_tag_family);
                                        april_td.add_family(april_tf);
                                    }

                                    if let Some(mut im) = frame2april(&frame) {
                                        let detections = april_td.detect(im.inner_mut());

                                        if let Some(ref mut wtr) = apriltag_writer {
                                            wtr.save(
                                                &detections,
                                                frame.extra().host_framenumber(),
                                                frame.extra().host_timestamp(),
                                            )?;
                                        }

                                        let tag_points =
                                            detections.as_slice().iter().map(det2display);
                                        all_points.extend(tag_points);
                                    }
                                }
                            }
                        }
                    }

                    #[cfg(not(feature = "flydra_feat_detect"))]
                    {
                        use flydra_types::ImageProcessingSteps;

                        // In case we are not doing flydra feature detection, send frame data to braid anyway.
                        let process_new_frame_start = chrono::Utc::now();
                        let acquire_stamp = FlydraFloatTimestampLocal::from_dt(
                            &extracted_frame_info.host_timestamp,
                        );

                        let preprocess_stamp =
                            datetime_conversion::datetime_to_f64(&process_new_frame_start);

                        let tracker_annotation = flydra_types::FlydraRawUdpPacket {
                            cam_name: raw_cam_name.as_str().to_string(),
                            timestamp: braid_ts,
                            cam_received_time: acquire_stamp,
                            device_timestamp,
                            block_id,
                            framenumber: frame.extra().host_framenumber() as i32,
                            n_frames_skipped: 0, // FIXME TODO XXX FIX THIS, should be n_frames_skipped
                            done_camnode_processing: 0.0,
                            preprocess_stamp,
                            image_processing_steps: ImageProcessingSteps::empty(),
                            points: vec![],
                        };
                        if let Some(ref coord_socket) = coord_socket {
                            // Send the data to the mainbrain
                            let mut vec = Vec::new();
                            {
                                let mut serializer = serde_cbor::ser::Serializer::new(&mut vec);
                                serializer.self_describe().unwrap();
                                tracker_annotation.serialize(&mut serializer).unwrap();
                            }
                            coord_socket.send_complete(&vec)?;
                        }
                    }

                    #[cfg(feature = "flydra_feat_detect")]
                    {
                        if is_doing_object_detection {
                            let inner_ufmf_state = ufmf_state.take().unwrap();
                            // Detect features in the image and send them to the
                            // mainbrain for 3D processing.
                            let (tracker_annotation, new_ufmf_state) = im_tracker
                                .process_new_frame(
                                    &frame,
                                    inner_ufmf_state,
                                    device_timestamp,
                                    block_id,
                                    braid_ts,
                                )?;
                            if let Some(ref coord_socket) = coord_socket {
                                // Send the data to the mainbrain
                                let mut vec = Vec::new();
                                {
                                    let mut serializer = serde_cbor::ser::Serializer::new(&mut vec);
                                    serializer.self_describe().unwrap();
                                    tracker_annotation.serialize(&mut serializer).unwrap();
                                }
                                coord_socket.send_complete(&vec)?;
                            }
                            ufmf_state.get_or_insert(new_ufmf_state);

                            #[cfg(feature = "flydratrax")]
                            {
                                if let Some(ref mut flydra2_stream) = maybe_flydra2_stream {
                                    let points = tracker_annotation
                                        .points
                                        .iter()
                                        .filter(|pt| {
                                            pt.area
                                                >= kalman_tracking_config.min_central_moment as f64
                                        })
                                        .enumerate()
                                        .map(|(i, pt)| {
                                            assert!(i <= u8::max_value() as usize);
                                            let idx = i as u8;
                                            flydra2::NumberedRawUdpPoint {
                                                idx,
                                                pt: pt.clone(),
                                            }
                                        })
                                        .collect();

                                    let cam_received_timestamp =
                                        datetime_conversion::datetime_to_f64(
                                            &frame.extra().host_timestamp(),
                                        );

                                    // TODO FIXME XXX It is a lie that this
                                    // timesource is Triggerbox. This is just for
                                    // single-camera flydratrax, though.
                                    let trigger_timestamp = Some(FlydraFloatTimestampLocal::<
                                        flydra_types::Triggerbox,
                                    >::from_f64(
                                        cam_received_timestamp
                                    ));

                                    // This is not a lie.
                                    let cam_received_timestamp = FlydraFloatTimestampLocal::<
                                        flydra_types::HostClock,
                                    >::from_f64(
                                        cam_received_timestamp
                                    );

                                    let cam_num = 0.into(); // Only one camera, so this must be correct.
                                    let frame_data = flydra2::FrameData::new(
                                        raw_cam_name.clone(),
                                        cam_num,
                                        flydra_types::SyncFno(
                                            frame.extra().host_framenumber().try_into().unwrap(),
                                        ),
                                        trigger_timestamp,
                                        cam_received_timestamp,
                                        device_timestamp,
                                        block_id,
                                    );
                                    let fdp = flydra2::FrameDataAndPoints { frame_data, points };
                                    let si = flydra2::StreamItem::Packet(fdp);

                                    // block until sent
                                    match futures::executor::block_on(futures::sink::SinkExt::send(
                                        flydra2_stream,
                                        si,
                                    )) {
                                        Ok(()) => {}
                                        Err(e) => return Err(e.into()),
                                    }
                                }
                            }

                            let points = tracker_annotation.points;

                            let mut new_state = None;
                            match csv_save_state {
                                SavingState::NotSaving => {}
                                SavingState::Starting(rate_limit) => {
                                    // create dir if needed
                                    std::fs::create_dir_all(&csv_save_pathbuf)?;

                                    // start saving tracking
                                    let base_template = "flytrax%Y%m%d_%H%M%S";
                                    let now = frame.extra().host_timestamp();
                                    let local = now.with_timezone(&chrono::Local);
                                    let base = local.format(base_template).to_string();

                                    // save jpeg image
                                    {
                                        let mut image_path = csv_save_pathbuf.clone();
                                        image_path.push(base.clone());
                                        image_path.set_extension("jpg");

                                        let bytes = match_all_dynamic_fmts!(&frame, x, {
                                            convert_image::frame_to_image(
                                                x,
                                                convert_image::ImageOptions::Jpeg(99),
                                            )?
                                        });
                                        File::create(image_path)?.write_all(&bytes)?;
                                    }

                                    let mut csv_path = csv_save_pathbuf.clone();
                                    csv_path.push(base);
                                    csv_path.set_extension("csv");
                                    info!("saving data to {}.", csv_path.display());

                                    if let Some(ref ssa) = shared_store_arc {
                                        // scope for write lock on ssa
                                        let new_val =
                                            RecordingPath::new(csv_path.display().to_string());
                                        let mut tracker = ssa.write();
                                        tracker.modify(|shared| {
                                            shared.is_saving_im_pt_detect_csv = Some(new_val);
                                        });
                                    }

                                    let mut fd = File::create(csv_path)?;

                                    // save configuration as commented yaml
                                    {
                                        let save_cfg =
                                            strand_cam_csv_config_types::SaveCfgFview2_0_25 {
                                                name: app_name.to_string(),
                                                version: env!("CARGO_PKG_VERSION").to_string(),
                                                git_hash: env!("GIT_HASH").to_string(),
                                            };

                                        let object_detection_cfg = im_tracker.config();

                                        let full_cfg =
                                            strand_cam_csv_config_types::FullCfgFview2_0_26 {
                                                app: save_cfg,
                                                camera: camera_cfg.clone(),
                                                created_at: local,
                                                csv_rate_limit: rate_limit,
                                                object_detection_cfg,
                                            };
                                        let cfg_yaml = serde_yaml::to_string(&full_cfg).unwrap();
                                        writeln!(fd, "# -- start of yaml config --")?;
                                        for line in cfg_yaml.lines() {
                                            writeln!(fd, "# {line}")?;
                                        }
                                        writeln!(fd, "# -- end of yaml config --")?;
                                    }

                                    writeln!(fd, "time_microseconds,frame,x_px,y_px,orientation_radians_mod_pi,central_moment,led_1,led_2,led_3")?;
                                    fd.flush()?;

                                    let min_interval_sec = if let Some(fps) = rate_limit {
                                        1.0 / fps
                                    } else {
                                        0.0
                                    };
                                    let min_interval = chrono::Duration::nanoseconds(
                                        (min_interval_sec * 1e9) as i64,
                                    );

                                    let inner = CsvSavingState {
                                        fd,
                                        min_interval,
                                        last_save: now
                                            .checked_sub_signed(
                                                chrono::Duration::try_days(1).unwrap(),
                                            )
                                            .unwrap(),
                                        t0: now,
                                    };

                                    new_state = Some(SavingState::Saving(inner));
                                }
                                SavingState::Saving(ref mut inner) => {
                                    let interval = frame
                                        .extra()
                                        .host_timestamp()
                                        .signed_duration_since(inner.last_save);
                                    // save found points
                                    if interval >= inner.min_interval && !points.is_empty() {
                                        let time_microseconds = frame
                                            .extra()
                                            .host_timestamp()
                                            .signed_duration_since(inner.t0)
                                            .num_microseconds()
                                            .unwrap();

                                        let mut led1 = "".to_string();
                                        let mut led2 = "".to_string();
                                        let mut led3 = "".to_string();
                                        {
                                            if let Some(ref store) = store_cache {
                                                if let Some(ref device_state) =
                                                    store.led_box_device_state
                                                {
                                                    led1 = format!(
                                                        "{}",
                                                        crate::get_intensity(device_state, 1)
                                                    );
                                                    led2 = format!(
                                                        "{}",
                                                        crate::get_intensity(device_state, 2)
                                                    );
                                                    led3 = format!(
                                                        "{}",
                                                        crate::get_intensity(device_state, 3)
                                                    );
                                                }
                                            }
                                        }
                                        for pt in points.iter() {
                                            let orientation_mod_pi =
                                                match pt.maybe_slope_eccentricty {
                                                    Some((slope, _ecc)) => {
                                                        let orientation_mod_pi =
                                                            f32::atan(slope as f32);
                                                        format!("{orientation_mod_pi:.3}")
                                                    }
                                                    None => "".to_string(),
                                                };
                                            writeln!(
                                                inner.fd,
                                                "{},{},{:.1},{:.1},{},{},{},{},{}",
                                                time_microseconds,
                                                frame.extra().host_framenumber(),
                                                pt.x0_abs,
                                                pt.y0_abs,
                                                orientation_mod_pi,
                                                pt.area,
                                                led1,
                                                led2,
                                                led3
                                            )?;
                                            inner.fd.flush()?;
                                        }
                                        inner.last_save = frame.extra().host_timestamp();
                                    }
                                }
                            }
                            if let Some(ns) = new_state {
                                csv_save_state = ns;
                            }

                            let display_points: Vec<_> = points
                                .iter()
                                .map(|pt| video_streaming::Point {
                                    x: pt.x0_abs as f32,
                                    y: pt.y0_abs as f32,
                                    theta: pt
                                        .maybe_slope_eccentricty
                                        .map(|(slope, _ecc)| f32::atan(slope as f32)),
                                    area: Some(pt.area as f32),
                                })
                                .collect();

                            all_points.extend(display_points);
                            blkajdsfads = Some(im_tracker.valid_region())
                        }
                    }
                    (all_points, blkajdsfads)
                };

                if let Some(ref mut inner) = my_mp4_writer {
                    let data = frame.clone(); // copy entire frame data
                    inner.write(data, save_mp4_fmf_stamp)?;
                }

                if let Some(ref mut inner) = fmf_writer {
                    // Based on our recording framerate, do we need to save this frame?
                    let do_save = match inner.last_saved_stamp {
                        None => true,
                        Some(stamp) => {
                            let elapsed = save_mp4_fmf_stamp - stamp;
                            elapsed
                                >= chrono::Duration::from_std(inner.recording_framerate.interval())?
                        }
                    };
                    if do_save {
                        match_all_dynamic_fmts!(&frame, x, {
                            inner.writer.write(x, save_mp4_fmf_stamp)?
                        });
                        inner.last_saved_stamp = Some(save_mp4_fmf_stamp);
                    }
                }

                #[cfg(feature = "plugin-process-frame")]
                {
                    // Do FFI image processing with lowest latency possible
                    if do_process_frame_callback {
                        if plugin_handler_thread_tx.is_full() {
                            error!("cannot transmit frame to plugin: channel full");
                        } else {
                            plugin_handler_thread_tx.send(frame.clone()).unwrap();
                            match plugin_result_rx.recv_timeout(plugin_wait_dur) {
                                Ok(results) => {
                                    found_points.extend(results);
                                }
                                Err(e) => {
                                    if e.is_timeout() {
                                        error!("Not displaying annotation because the plugin took too long.");
                                    } else {
                                        error!("The plugin disconnected.");
                                        eyre::bail!("The plugin disconnected.");
                                    }
                                }
                            }
                        }
                    }
                }

                let found_points = found_points
                    .iter()
                    .map(
                        |pt: &http_video_streaming_types::Point| video_streaming::Point {
                            x: pt.x,
                            y: pt.y,
                            theta: pt.theta,
                            area: pt.area,
                        },
                    )
                    .collect();

                // check led_box device heartbeat
                if let Some(reader) = *led_box_heartbeat_update_arc.read() {
                    let elapsed = reader.elapsed();
                    if elapsed
                        > std::time::Duration::from_millis(2 * LED_BOX_HEARTBEAT_INTERVAL_MSEC)
                    {
                        error!("No led_box heatbeat for {:?}.", elapsed);

                        // No heartbeat within the specified interval.
                        if let Some(ref ssa) = shared_store_arc {
                            let mut tracker = ssa.write();
                            tracker.modify(|store| store.led_box_device_lost = true);
                        }
                    }
                }

                #[cfg(feature = "flydratrax")]
                let annotations = if let Some(ref clpcs) = current_led_program_config_state {
                    vec![http_video_streaming_types::DrawableShape::from_shape(
                        &clpcs.led_on_shape_pixels,
                        &red_style,
                        1.0,
                    )]
                } else {
                    vec![]
                };

                #[cfg(not(feature = "flydratrax"))]
                let annotations = vec![];

                if firehose_tx.capacity() == 0 {
                    trace!("cannot transmit frame for viewing: channel full");
                } else {
                    let result = firehose_tx
                        .send(AnnotatedFrame {
                            frame,
                            found_points,
                            valid_display,
                            annotations,
                        })
                        .await;
                    match result {
                        Ok(()) => {}
                        Err(e) => {
                            tracing::error!(
                                "error while sending frame for display in browser: {e} {e:?}"
                            );
                        }
                    }
                }
            }
            #[cfg(feature = "flydra_feat_detect")]
            Msg::SetIsSavingObjDetectionCsv(new_value) => {
                info!(
                    "setting object detection CSV save state to: {:?}",
                    new_value
                );
                if let ci2_remote_control::CsvSaveConfig::Saving(fps_limit) = new_value {
                    if !store_cache
                        .map(|s| s.is_doing_object_detection)
                        .unwrap_or(false)
                    {
                        error!("Not doing object detection, ignoring command to save data to CSV.");
                    } else {
                        csv_save_state = SavingState::Starting(fps_limit);

                        #[cfg(feature = "flydratrax")]
                        {
                            if let Some(ref mut braidz_write_tx_weak) =
                                opt_braidz_write_tx_weak.as_mut()
                            {
                                let local: chrono::DateTime<chrono::Local> = chrono::Local::now();
                                let dirname = local.format("%Y%m%d_%H%M%S.braid").to_string();
                                let mut my_dir = csv_save_pathbuf.clone();
                                my_dir.push(dirname);

                                tracing::warn!("unimplemented setting of FPS and camera images");

                                // We could and should add this data here:
                                let expected_fps = None;
                                let per_cam_data = Default::default();

                                let cfg = flydra2::StartSavingCsvConfig {
                                    out_dir: my_dir.clone(),
                                    local: Some(local),
                                    git_rev: env!("GIT_HASH").to_string(),
                                    fps: expected_fps,
                                    per_cam_data,
                                    print_stats: false,
                                    save_performance_histograms: true,
                                };
                                if let Some(braidz_write_tx) = braidz_write_tx_weak.upgrade() {
                                    // `braidz_write_tx` will be dropped after this scope.
                                    braidz_write_tx
                                        .send(flydra2::SaveToDiskMsg::StartSavingCsv(cfg))
                                        .await
                                        .unwrap();
                                }
                            }
                        }
                    }
                } else {
                    match csv_save_state {
                        SavingState::NotSaving => {}
                        _ => {
                            info!("stopping data saving.");
                        }
                    }
                    // this potentially drops file, thus closing it.
                    csv_save_state = SavingState::NotSaving;
                    #[cfg(feature = "flydratrax")]
                    {
                        if let Some(ref mut braidz_write_tx_weak) =
                            opt_braidz_write_tx_weak.as_mut()
                        {
                            if let Some(braidz_write_tx) = braidz_write_tx_weak.upgrade() {
                                // `braidz_write_tx` will be dropped after this scope.
                                match braidz_write_tx
                                    .send(flydra2::SaveToDiskMsg::StopSavingCsv)
                                    .await
                                {
                                    Ok(()) => {}
                                    Err(_) => {
                                        info!("Channel to data writing task closed. Ending.");
                                        break;
                                    }
                                }
                            }
                        }
                    }

                    // update UI
                    if let Some(ref ssa) = shared_store_arc {
                        // scope for write lock on ssa
                        let mut tracker = ssa.write();
                        tracker.modify(|shared| {
                            shared.is_saving_im_pt_detect_csv = None;
                        });
                    }
                }
            }
            #[cfg(feature = "flydra_feat_detect")]
            Msg::SetExpConfig(cfg) => {
                im_tracker.set_config(cfg).expect("set_config()");
            }
            #[cfg(feature = "flydra_feat_detect")]
            Msg::TakeCurrentImageAsBackground => {
                im_tracker.do_take_current_image_as_background()?;
            }
            #[cfg(feature = "flydra_feat_detect")]
            Msg::ClearBackground(value) => {
                im_tracker.do_clear_background(value)?;
            }
            Msg::SetFrameOffset(fo) => {
                opt_frame_offset = Some(fo);
                #[cfg(feature = "flydra_feat_detect")]
                {
                    im_tracker.set_frame_offset(fo);
                }
            }
            Msg::SetTriggerboxClockModel(cm) => {
                triggerbox_clock_model = cm;
            }
            Msg::StopMp4 => {
                if let Some(mut inner) = my_mp4_writer.take() {
                    inner.finish()?;
                }
                if let Some(ref mut store) = shared_store_arc {
                    let mut tracker = store.write();
                    tracker.modify(|tracker| {
                        tracker.is_recording_mp4 = None;
                    });
                }
            }
            Msg::StopFMF => {
                fmf_writer = None;
            }
            #[cfg(feature = "flydra_feat_detect")]
            Msg::StopUFMF => {
                ufmf_state = Some(flydra_feature_detector::UfmfState::Stopped);
            }
            #[cfg(feature = "flydra_feat_detect")]
            Msg::SetTracking(value) => {
                is_doing_object_detection = value;
            }
        };
    }
    info!(
        "frame process thread done for camera '{}'",
        cam_name.as_str()
    );
    Ok(())
}

#[cfg(feature = "fiducial")]
fn make_family(family: &ci2_remote_control::TagFamily) -> apriltag::Family {
    use ci2_remote_control::TagFamily::*;
    match family {
        Family36h11 => apriltag::Family::new_tag_36h11(),
        FamilyStandard41h12 => apriltag::Family::new_tag_standard_41h12(),
        Family16h5 => apriltag::Family::new_tag_16h5(),
        Family25h9 => apriltag::Family::new_tag_25h9(),
        FamilyCircle21h7 => apriltag::Family::new_tag_circle_21h7(),
        FamilyCircle49h12 => apriltag::Family::new_tag_circle_49h12(),
        FamilyCustom48h12 => apriltag::Family::new_tag_custom_48h12(),
        FamilyStandard52h13 => apriltag::Family::new_tag_standard_52h13(),
    }
}

#[cfg(feature = "fiducial")]
struct AprilTagWriter {
    wtr: csv::Writer<Box<dyn std::io::Write + Send>>,
    t0: chrono::DateTime<chrono::Utc>,
}

#[cfg(feature = "fiducial")]
impl AprilTagWriter {
    fn new(
        template: String,
        camera_name: &str,
        camera_width_pixels: usize,
        camera_height_pixels: usize,
    ) -> Result<Self> {
        let now: chrono::DateTime<chrono::Utc> = chrono::Utc::now();
        let local = now.with_timezone(&chrono::Local);
        let fname = local.format(&template).to_string();

        let fd = std::fs::File::create(&fname)?;
        let mut fd: Box<dyn std::io::Write + Send> =
            Box::new(AutoFinishUnchecked::new(Encoder::new(fd)?));

        let april_config = AprilConfig {
            created_at: local,
            camera_name: camera_name.to_string(),
            camera_width_pixels,
            camera_height_pixels,
        };
        let cfg_yaml = serde_yaml::to_string(&april_config).unwrap();
        writeln!(
            fd,
            "# The homography matrix entries (h00,...) are described in the April Tags paper"
        )?;
        writeln!(
            fd,
            "# https://dx.doi.org/10.1109/ICRA.2011.5979561 . Entry h22 is not saved because"
        )?;
        writeln!(
            fd,
            "# it always has value 1. The center pixel of the detection is (h02,h12)."
        )?;
        writeln!(fd, "# -- start of yaml config --")?;
        for line in cfg_yaml.lines() {
            writeln!(fd, "# {}", line)?;
        }
        writeln!(fd, "# -- end of yaml config --")?;

        let wtr = csv::Writer::from_writer(fd);

        Ok(Self { wtr, t0: now })
    }
    fn save(
        &mut self,
        detections: &apriltag::Zarray<apriltag::Detection>,
        frame: usize,
        ts: chrono::DateTime<chrono::Utc>,
    ) -> Result<()> {
        let time_microseconds = ts
            .signed_duration_since(self.t0)
            .num_microseconds()
            .unwrap();
        for det in detections.as_slice().iter() {
            let atd: DetectionSerializer = to_serializer(det, frame, time_microseconds);
            self.wtr.serialize(atd)?;
        }
        Ok(())
    }
}

#[cfg(feature = "fiducial")]
fn det2display(det: &apriltag::Detection) -> http_video_streaming_types::Point {
    let center = det.center();
    video_streaming::Point {
        x: center[0] as f32,
        y: center[1] as f32,
        theta: None,
        area: None,
    }
}

#[cfg(feature = "fiducial")]
fn frame2april(frame: &DynamicFrame) -> Option<apriltag::ImageU8Borrowed> {
    match frame {
        DynamicFrame::Mono8(frame) => Some(apriltag::ImageU8Borrowed::view(frame)),
        _ => None,
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct AprilConfig {
    created_at: chrono::DateTime<chrono::Local>,
    camera_name: String,
    camera_width_pixels: usize,
    camera_height_pixels: usize,
}

// The center pixel of the detection is (h02,h12)
#[cfg(feature = "fiducial")]
#[derive(Serialize, Deserialize, Debug, Clone)]
struct DetectionSerializer {
    frame: usize,
    time_microseconds: i64,
    id: i32,
    hamming: i32,
    decision_margin: f32,
    h00: f64,
    h01: f64,
    h02: f64,
    h10: f64,
    h11: f64,
    h12: f64,
    h20: f64,
    h21: f64,
    // no h22 because it is always 1.0
    family: String,
}

#[cfg(feature = "fiducial")]
fn my_round(a: f32) -> f32 {
    let b = (a * 10.0).round() as i64;
    b as f32 / 10.0
}

#[cfg(feature = "fiducial")]
fn to_serializer(
    orig: &apriltag::Detection,
    frame: usize,
    time_microseconds: i64,
) -> DetectionSerializer {
    let h = orig.h();
    // We are not going to save h22, so (in debug builds) let's check it meets
    // our expectations.
    debug_assert!((h[8] - 1.0).abs() < 1e-16);
    DetectionSerializer {
        frame,
        time_microseconds,
        id: orig.id(),
        hamming: orig.hamming(),
        decision_margin: my_round(orig.decision_margin()),
        h00: h[0],
        h01: h[1],
        h02: h[2],
        h10: h[3],
        h11: h[4],
        h12: h[5],
        h20: h[6],
        h21: h[7],
        family: orig.family_type().to_str().to_string(),
    }
}

#[cfg(feature = "flydratrax")]
struct FlydraConfigState {
    region: video_streaming::Shape,
    kalman_tracking_config: strand_cam_storetype::KalmanTrackingConfig,
}
