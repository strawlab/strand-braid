#![recursion_limit = "1000"]

use std::net::{IpAddr, SocketAddr};

use ci2_remote_control::CamArg;

#[cfg(feature = "with_camtrig")]
use camtrig_comms::ToDevice as ToCamtrigDevice;

#[cfg(not(feature = "with_camtrig"))]
#[allow(dead_code)]
type ToCamtrigDevice = std::marker::PhantomData<u8>;

use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;
use yew::format::Json;
use yew::prelude::*;

use ads_webasm::components::{EnumToggle, VecToggle};
use http_video_streaming_types::ToClient as FirehoseImageData;
use strand_cam_storetype::CallbackType;
use strand_cam_storetype::StoreType as ServerState;
#[cfg(feature = "flydratrax")]
use strand_cam_storetype::{KalmanTrackingConfig, LedProgramConfig};
use yew_tincture::components::CheckboxLabel;

use ci2_remote_control::{RecordingFrameRate, TagFamily};
use ci2_types::AutoMode;

use image_tracker_types::ImPtDetectCfg;
use yew_tincture::components::{TypedInput, TypedInputStorage};

use yew::services::fetch::{Credentials, FetchOptions, FetchService, FetchTask, Request, Response};

use yew_event_source::{EventSourceService, EventSourceStatus, EventSourceTask, ReadyState};

use ads_webasm::video_data::VideoData;

mod components;
use crate::components::AutoModeSelect;

use ads_webasm::components::{
    Button, ConfigField, RangedValue, RecordingPathWidget, ReloadButton, Toggle, VideoField,
};

#[cfg(feature = "with_camtrig")]
use components::CamtrigControl;

const LAST_DETECTED_VALUE_LABEL: &'static str = "Last detected value: ";

enum Msg {
    /// Trigger a check of the event source state.
    EsCheckState,

    NewImageFrame(FirehoseImageData),

    NewServerState(ServerState),

    FailedCallbackJsonDecode(String),
    DismissJsonDecodeError,

    DismissProcessingErrorModal,
    SetIgnoreAllFutureErrors(bool),

    SetGainAuto(AutoMode),
    SetGainValue(f64),
    SetExposureAuto(AutoMode),
    SetExposureValue(f64),

    SetFrameRateLimitEnabled(bool),
    SetFrameRateLimit(f64),

    // only used when image-tracker crate used
    SetObjDetectionConfig(String),
    // only used when image-tracker crate used
    ToggleObjDetection(bool),
    // only used when image-tracker crate used
    ToggleObjDetectionSaveCsv(bool),
    // only used when image-tracker crate used
    ToggleCsvRecordingRate(RecordingFrameRate),

    ToggleTagFamily(TagFamily),
    ToggleAprilTagDetection(bool),
    ToggleAprilTagDetectionSaveCsv(bool),

    ToggleImOpsDetection(bool),
    SetImOpsDestination(SocketAddr),
    SetImOpsSource(IpAddr),
    SetImOpsCenterX(u32),
    SetImOpsCenterY(u32),
    SetImOpsTheshold(u8),

    #[cfg(feature = "flydratrax")]
    CamArgSetKalmanTrackingConfig(String),
    #[cfg(feature = "flydratrax")]
    CamArgSetLedProgramConfig(String),

    ToggleFmfSave(bool),
    ToggleFmfRecordingFrameRate(RecordingFrameRate),

    // only used when image-tracker crate used
    ToggleUfmfSave(bool),

    ToggleMkvSave(bool),
    ToggleMkvRecordingFrameRate(RecordingFrameRate),
    ToggleMkvBitrate(BitrateSelection),
    ToggleMkvCodec(usize),
    ToggleCudaDevice(i32),

    // only used when image-tracker crate used
    TakeCurrentImageAsBackground,
    // only used when image-tracker crate used
    ClearBackground(f32),

    #[cfg(feature = "with_camtrig")]
    CamtrigControlEvent(ToCamtrigDevice),

    #[cfg(feature = "checkercal")]
    ToggleCheckerboardDetection(bool),
    #[cfg(feature = "checkercal")]
    ToggleCheckerboardDebug(bool),
    #[cfg(feature = "checkercal")]
    SetCheckerboardWidth(u32),
    #[cfg(feature = "checkercal")]
    SetCheckerboardHeight(u32),
    #[cfg(feature = "checkercal")]
    PerformCheckerboardCalibration,
    #[cfg(feature = "checkercal")]
    ClearCheckerboards,

    SetPostTriggerBufferSize(usize),
    PostTriggerMkvRecording,

    // UpdateConnectionState(ReadyState),
    Ignore,
}

struct Model {
    /// Keep task to prevent it from being dropped.
    #[allow(dead_code)]
    ft: Option<FetchTask>,
    video_data: VideoData,

    server_state: Option<ServerState>,
    json_decode_err: Option<String>,
    html_page_title: Option<String>,
    es: EventSourceTask,
    link: ComponentLink<Self>,

    csv_recording_rate: RecordingFrameRate,
    #[cfg(feature = "checkercal")]
    checkerboard_width: TypedInputStorage<u32>,
    #[cfg(feature = "checkercal")]
    checkerboard_height: TypedInputStorage<u32>,
    post_trigger_buffer_size_local: TypedInputStorage<usize>,

    im_ops_destination_local: TypedInputStorage<SocketAddr>,
    im_ops_source_local: TypedInputStorage<IpAddr>,
    im_ops_center_x: TypedInputStorage<u32>,
    im_ops_center_y: TypedInputStorage<u32>,
    im_ops_threshold: TypedInputStorage<u8>,

    ignore_all_future_frame_processing_errors: bool,
}

impl Component for Model {
    type Message = Msg;
    type Properties = ();

    fn create(_: Self::Properties, link: ComponentLink<Self>) -> Self {
        let task = {
            let data_callback = link.callback(|Json(data)| match data {
                Ok(data_result) => Msg::NewServerState(data_result),
                Err(e) => {
                    log::error!("in data callback: {}", e);
                    Msg::FailedCallbackJsonDecode(format!("{}", e))
                }
            });
            let stream_callback = link.callback(|Json(data)| match data {
                Ok(image_result) => Msg::NewImageFrame(image_result),
                Err(e) => {
                    log::error!("in stream callback: {}", e);
                    Msg::FailedCallbackJsonDecode(format!("{}", e))
                }
            });
            let notification = link.callback(|status| {
                if status == EventSourceStatus::Error {
                    log::error!("event source error");
                }
                Msg::EsCheckState
            });
            let mut task = EventSourceService::new()
                .connect(
                    strand_cam_storetype::STRAND_CAM_EVENTS_URL_PATH,
                    notification,
                )
                .unwrap();
            task.add_event_listener(strand_cam_storetype::STRAND_CAM_EVENT_NAME, data_callback);
            task.add_event_listener(
                http_video_streaming_types::VIDEO_STREAM_EVENT_NAME,
                stream_callback,
            );
            task
        };

        Self {
            ft: None,
            video_data: VideoData::default(),

            server_state: None,
            json_decode_err: None,
            html_page_title: None,
            es: task,
            link,
            csv_recording_rate: RecordingFrameRate::Unlimited,
            #[cfg(feature = "checkercal")]
            checkerboard_width: TypedInputStorage::empty(),
            #[cfg(feature = "checkercal")]
            checkerboard_height: TypedInputStorage::empty(),
            post_trigger_buffer_size_local: TypedInputStorage::empty(),

            im_ops_destination_local: TypedInputStorage::empty(),
            im_ops_source_local: TypedInputStorage::empty(),
            im_ops_center_x: TypedInputStorage::empty(),
            im_ops_center_y: TypedInputStorage::empty(),
            im_ops_threshold: TypedInputStorage::empty(),

            ignore_all_future_frame_processing_errors: false,
        }
    }

    fn change(&mut self, _: ()) -> ShouldRender {
        false
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            Msg::EsCheckState => {
                return true;
            }
            Msg::NewImageFrame(in_msg) => {
                self.video_data = VideoData::new(in_msg);
            }
            Msg::NewServerState(response) => {
                // Set the html page title once.
                if self.html_page_title.is_none() {
                    let strand_cam_name = get_strand_cam_name(self.server_state.as_ref());
                    let title = format!("{} - {}", response.camera_name, strand_cam_name);
                    web_sys::window()
                        .unwrap()
                        .document()
                        .unwrap()
                        .set_title(&title);
                    self.html_page_title = Some(title);
                }

                // Do this only if user is not focused on field.
                #[cfg(feature = "checkercal")]
                {
                    self.checkerboard_width
                        .set_if_not_focused(response.checkerboard_data.width);
                    self.checkerboard_height
                        .set_if_not_focused(response.checkerboard_data.height);
                }

                self.post_trigger_buffer_size_local
                    .set_if_not_focused(response.post_trigger_buffer_size);

                self.im_ops_destination_local
                    .set_if_not_focused(response.im_ops_state.destination);

                self.im_ops_source_local
                    .set_if_not_focused(response.im_ops_state.source);

                self.im_ops_center_x
                    .set_if_not_focused(response.im_ops_state.center_x);

                self.im_ops_center_y
                    .set_if_not_focused(response.im_ops_state.center_y);

                self.im_ops_threshold
                    .set_if_not_focused(response.im_ops_state.threshold);

                // Update our cache of the server state
                self.server_state = Some(response);
            }
            Msg::FailedCallbackJsonDecode(s) => {
                self.json_decode_err = Some(s);
            }
            Msg::DismissJsonDecodeError => {
                self.json_decode_err = None;
            }
            Msg::DismissProcessingErrorModal => {
                let limit_duration = if self.ignore_all_future_frame_processing_errors {
                    None
                } else {
                    Some(5)
                };
                self.ft = send_cam_message(
                    CamArg::SetIngoreFutureFrameProcessingErrors(limit_duration),
                    self,
                );
                return false; // don't update DOM, do that on return
            }
            Msg::SetIgnoreAllFutureErrors(val) => {
                self.ignore_all_future_frame_processing_errors = val;
            }
            Msg::SetGainAuto(v) => {
                self.ft = send_cam_message(CamArg::SetGainAuto(v), self);
                return false; // don't update DOM, do that on return
            }
            Msg::SetGainValue(v) => {
                self.ft = send_cam_message(CamArg::SetGain(v), self);
                return false; // don't update DOM, do that on return
            }
            Msg::SetExposureAuto(v) => {
                self.ft = send_cam_message(CamArg::SetExposureAuto(v), self);
                return false; // don't update DOM, do that on return
            }
            Msg::SetExposureValue(v) => {
                self.ft = send_cam_message(CamArg::SetExposureTime(v), self);
                return false; // don't update DOM, do that on return
            }
            Msg::SetFrameRateLimitEnabled(v) => {
                self.ft = send_cam_message(CamArg::SetFrameRateLimitEnabled(v.into()), self);
                return false; // don't update DOM, do that on return
            }
            Msg::SetFrameRateLimit(v) => {
                self.ft = send_cam_message(CamArg::SetFrameRateLimit(v), self);
                return false; // don't update DOM, do that on return
            }
            // only used when image-tracker crate used
            Msg::SetObjDetectionConfig(v) => {
                self.ft = send_cam_message(CamArg::SetObjDetectionConfig(v), self);
                return false; // don't update DOM, do that on return
            }
            #[cfg(feature = "flydratrax")]
            Msg::CamArgSetKalmanTrackingConfig(v) => {
                self.ft = send_cam_message(CamArg::CamArgSetKalmanTrackingConfig(v), self);
                return false; // don't update DOM, do that on return
            }
            #[cfg(feature = "flydratrax")]
            Msg::CamArgSetLedProgramConfig(v) => {
                self.ft = send_cam_message(CamArg::CamArgSetLedProgramConfig(v), self);
                return false; // don't update DOM, do that on return
            }
            // only used when image-tracker crate used
            Msg::ToggleObjDetection(v) => {
                self.ft = send_cam_message(CamArg::SetIsDoingObjDetection(v), self);
                return false; // don't update DOM, do that on return
            }
            // only used when image-tracker crate used
            Msg::ToggleObjDetectionSaveCsv(v) => {
                let cfg = if v {
                    ci2_remote_control::CsvSaveConfig::Saving(to_rate(&self.csv_recording_rate))
                } else {
                    ci2_remote_control::CsvSaveConfig::NotSaving
                };
                self.ft = send_cam_message(CamArg::SetIsSavingObjDetectionCsv(cfg), self);
                return false; // don't update DOM, do that on return
            }
            // only used when image-tracker crate used
            Msg::ToggleCsvRecordingRate(v) => {
                self.csv_recording_rate = v;
                return false; // don't update DOM, do that on return
            }
            Msg::ToggleTagFamily(v) => {
                self.ft = send_cam_message(CamArg::ToggleAprilTagFamily(v), self);
                return false; // don't update DOM, do that on return
            }
            Msg::ToggleAprilTagDetection(v) => {
                self.ft = send_cam_message(CamArg::ToggleAprilTagDetection(v), self);
                return false; // don't update DOM, do that on return
            }
            Msg::ToggleAprilTagDetectionSaveCsv(v) => {
                self.ft = send_cam_message(CamArg::SetIsRecordingAprilTagCsv(v), self);
                return false; // don't update DOM, do that on return
            }
            Msg::ToggleImOpsDetection(v) => {
                self.ft = send_cam_message(CamArg::ToggleImOpsDetection(v), self);
                return false; // don't update DOM, do that on return
            }
            Msg::SetImOpsDestination(v) => {
                self.ft = send_cam_message(CamArg::SetImOpsDestination(v), self);
                return false; // don't update DOM, do that on return
            }
            Msg::SetImOpsSource(v) => {
                self.ft = send_cam_message(CamArg::SetImOpsSource(v), self);
                return false; // don't update DOM, do that on return
            }
            Msg::SetImOpsCenterX(v) => {
                self.ft = send_cam_message(CamArg::SetImOpsCenterX(v), self);
                return false; // don't update DOM, do that on return
            }
            Msg::SetImOpsCenterY(v) => {
                self.ft = send_cam_message(CamArg::SetImOpsCenterY(v), self);
                return false; // don't update DOM, do that on return
            }
            Msg::SetImOpsTheshold(v) => {
                self.ft = send_cam_message(CamArg::SetImOpsThreshold(v), self);
                return false; // don't update DOM, do that on return
            }
            Msg::ToggleFmfRecordingFrameRate(v) => {
                self.ft = send_cam_message(CamArg::SetRecordingFps(v), self);
                return false; // don't update DOM, do that on return
            }
            Msg::ToggleMkvRecordingFrameRate(v) => {
                self.ft = send_cam_message(CamArg::SetMkvRecordingFps(v), self);
                return false; // don't update DOM, do that on return
            }
            Msg::ToggleMkvBitrate(v) => {
                let mut old_config = if let Some(ref state) = self.server_state {
                    state.mkv_recording_config.clone()
                } else {
                    ci2_remote_control::MkvRecordingConfig::default()
                };
                match &mut old_config.codec {
                    ci2_remote_control::MkvCodec::VP8(ref mut o) => o.bitrate = v.to_u32(),
                    ci2_remote_control::MkvCodec::VP9(ref mut o) => o.bitrate = v.to_u32(),
                    ci2_remote_control::MkvCodec::H264(ref mut o) => o.bitrate = v.to_u32(),
                }
                self.ft = send_cam_message(CamArg::SetMkvRecordingConfig(old_config), self);
                return false; // don't update DOM, do that on return
            }
            Msg::ToggleMkvCodec(idx) => {
                if let Some(ref shared) = self.server_state {
                    let available_codecs = shared.available_codecs();
                    let v = available_codecs[idx].clone();
                    let default = ci2_remote_control::MkvRecordingConfig::default();
                    let old_config = {
                        if let Some(ref state) = self.server_state {
                            state.mkv_recording_config.clone()
                        } else {
                            default
                        }
                    };
                    let cfg = ci2_remote_control::MkvRecordingConfig {
                        codec: v.get_codec(&old_config.codec),
                        max_framerate: old_config.max_framerate.clone(),
                        writing_application: None,
                    };
                    self.ft = send_cam_message(CamArg::SetMkvRecordingConfig(cfg), self);
                }
                return false; // don't update DOM, do that on return
            }
            Msg::ToggleCudaDevice(cuda_device) => {
                if let Some(ref shared) = self.server_state {
                    let mut cfg = shared.mkv_recording_config.clone();
                    // TODO: right now, the selected CUDA device is a property
                    // of the H264 codec options. This means that if a different
                    // codec is selected, the user's choice is forgotten.
                    match &mut cfg.codec {
                        ci2_remote_control::MkvCodec::H264(ref mut opts) => {
                            opts.cuda_device = cuda_device;
                        }
                        _ => {}
                    }
                    self.ft = send_cam_message(CamArg::SetMkvRecordingConfig(cfg), self);
                }
                return false; // don't update DOM, do that on return
            }
            Msg::ToggleFmfSave(v) => {
                self.ft = send_cam_message(CamArg::SetIsRecordingFmf(v), self);
                return false; // don't update DOM, do that on return
            }
            // only used when image-tracker crate used
            Msg::ToggleUfmfSave(v) => {
                self.ft = send_cam_message(CamArg::SetIsRecordingUfmf(v), self);
                return false; // don't update DOM, do that on return
            }
            Msg::ToggleMkvSave(v) => {
                self.ft = send_cam_message(CamArg::SetIsRecordingMkv(v), self);
                return false; // don't update DOM, do that on return
            }
            // only used when image-tracker crate used
            Msg::TakeCurrentImageAsBackground => {
                self.ft = self.send_message(&CallbackType::TakeCurrentImageAsBackground);
                return false; // don't update DOM, do that on return
            }
            // only used when image-tracker crate used
            Msg::ClearBackground(value) => {
                self.ft = self.send_message(&CallbackType::ClearBackground(value));
                return false; // don't update DOM, do that on return
            }
            #[cfg(feature = "with_camtrig")]
            Msg::CamtrigControlEvent(command) => {
                self.ft = self.send_message(&CallbackType::ToCamtrig(command));
                return false; // don't update DOM, do that on return
            }
            #[cfg(feature = "checkercal")]
            Msg::ToggleCheckerboardDetection(val) => {
                self.ft = send_cam_message(CamArg::ToggleCheckerboardDetection(val), self);
                return false;
            }
            #[cfg(feature = "checkercal")]
            Msg::ToggleCheckerboardDebug(val) => {
                self.ft = send_cam_message(CamArg::ToggleCheckerboardDebug(val), self);
                return false;
            }
            #[cfg(feature = "checkercal")]
            Msg::SetCheckerboardWidth(val) => {
                self.ft = send_cam_message(CamArg::SetCheckerboardWidth(val), self);
                return false;
            }
            #[cfg(feature = "checkercal")]
            Msg::SetCheckerboardHeight(val) => {
                self.ft = send_cam_message(CamArg::SetCheckerboardHeight(val), self);
                return false;
            }
            #[cfg(feature = "checkercal")]
            Msg::PerformCheckerboardCalibration => {
                self.ft = send_cam_message(CamArg::PerformCheckerboardCalibration, self);
                return false;
            }
            #[cfg(feature = "checkercal")]
            Msg::ClearCheckerboards => {
                self.ft = send_cam_message(CamArg::ClearCheckerboards, self);
                return false;
            }

            Msg::SetPostTriggerBufferSize(val) => {
                self.ft = send_cam_message(CamArg::SetPostTriggerBufferSize(val), self);
                return false;
            }

            Msg::PostTriggerMkvRecording => {
                let mkv_recording_config = if let Some(ref state) = self.server_state {
                    state.mkv_recording_config.clone()
                } else {
                    ci2_remote_control::MkvRecordingConfig::default()
                };

                self.ft = send_cam_message(CamArg::PostTrigger(mkv_recording_config), self);
                return false; // don't update DOM, do that on return
            }

            Msg::Ignore => {
                return false;
            }
        }
        true
    }

    fn view(&self) -> Html {
        let strand_cam_name = get_strand_cam_name(self.server_state.as_ref());
        html! {
            <div>
                <h1 style="text-align: center;">{strand_cam_name}<a href="https://strawlab.org/strand-cam/"><span class="infoCircle">{"ℹ"}</span></a></h1>
                <img src="strand-camera-no-text.png" width="521" height="118" class="center" />
                { self.disconnected_dialog() }
                { self.frame_processing_error_dialog() }
                { self.camtrig_failed() }
                <div class="wrapper">
                    { self.view_video() }
                    { self.view_decode_error() }
                    { self.view_camtrig() }
                    { self.view_led_triggering() }
                    { self.view_mkv_recording_options() }
                    { self.view_post_trigger_options() }
                    { self.point_detection_ui() }
                    { self.apriltag_detection_ui() }
                    { self.im_ops_ui() }
                    { self.checkerboard_calibration_ui() }

                    <div class="wrap-collapsible">
                        <CheckboxLabel label="Camera Settings" initially_checked=true />
                        <div>
                            <p>{"Set values on the camera itself."}</p>
                        </div>
                        <div>
                            { self.view_gain() }
                            { self.view_exposure() }
                            { self.view_frame_rate_limit() }
                        </div>
                    </div>
                    { self.view_fmf_recording_options() }
                    { self.view_kalman_tracking() }
                </div>
                <footer id="footer">
                {format!(
                    "Strand Camera version: {} (revision {})",
                    env!("CARGO_PKG_VERSION"),
                    env!("GIT_HASH")
                )}
                </footer>

            </div>
        }
    }
}

impl Model {
    fn view_decode_error(&self) -> Html {
        if let Some(ref json_decode_err) = self.json_decode_err {
            html! {
                <div>
                    <p>{"Error decoding callback JSON from server: "}{json_decode_err}</p>
                    <p><Button title="Dismiss" onsignal=self.link.callback(|_| Msg::DismissJsonDecodeError) /></p>
                </div>
            }
        } else {
            html! {}
        }
    }

    #[cfg(feature = "with_camtrig")]
    fn view_camtrig(&self) -> Html {
        if let Some(ref shared) = self.server_state {
            if let Some(ref device_state) = shared.camtrig_device_state {
                return html! {
                    <CamtrigControl
                        device_state=device_state.clone()
                        onsignal=self.link.callback(|x| Msg::CamtrigControlEvent(x))
                    />
                };
            }
        }
        html! {
            <div>{""}</div>
        }
    }

    #[cfg(not(feature = "with_camtrig"))]
    fn view_camtrig(&self) -> Html {
        html! {
            <div>{""}</div>
        }
    }

    fn view_video(&self) -> Html {
        if let Some(ref shared) = self.server_state {
            let title = format!("Live view - {}", shared.camera_name);
            let frame_number = self.video_data.frame_number().unwrap_or(0);
            html! {
                <VideoField title=title.clone()
                    video_data=self.video_data.clone()
                    frame_number=frame_number
                    width=shared.image_width
                    height=shared.image_height
                    measured_fps=shared.measured_fps
                />
            }
        } else {
            html! {
               <div>
                 { "" }
               </div>
            }
        }
    }

    fn disconnected_dialog(&self) -> Html {
        if self.es.ready_state() == ReadyState::Open {
            html! {
               <div>
                 { "" }
               </div>
            }
        } else {
            html! {
                <div class="modal-container">
                    <h1> { "Web browser not connected to Strand Camera" } </h1>
                    <p>{ format!("Connection State: {:?}", self.es.ready_state()) }</p>
                    <p>{ "Please restart Strand Camera and " }<ReloadButton label="reload"/></p>
                </div>
            }
        }
    }

    fn frame_processing_error_dialog(&self) -> Html {
        if let Some(ref shared) = self.server_state {
            if shared.had_frame_processing_error {
                return {
                    html! {
                    <div class="modal-container">
                        <h1> { "Error: frame processing too slow" } </h1>
                        <p>{"Processing of image frames is taking too long. Reduce the computational cost of image processing."}</p>
                        <p><Toggle
                                label="Ignore all future errors"
                                value=self.ignore_all_future_frame_processing_errors
                                ontoggle=self.link.callback(|checked| {
                                    Msg::SetIgnoreAllFutureErrors(checked)
                                })
                            /></p>
                        <p><Button title="Dismiss" onsignal=self.link.callback(|_| Msg::DismissProcessingErrorModal) /></p>
                    </div>
                    }
                };
            }
        }
        html! {
            <div>
            </div>
        }
    }

    #[cfg(not(feature = "with_camtrig"))]
    fn camtrig_failed(&self) -> Html {
        html! {
            <div>
                { "" }
            </div>
        }
    }

    #[cfg(feature = "with_camtrig")]
    fn camtrig_failed(&self) -> Html {
        let camtrig_device_lost = if let Some(ref shared) = self.server_state {
            shared.camtrig_device_lost
        } else {
            false
        };

        if !camtrig_device_lost {
            html! {
               <div>
                 { "" }
               </div>
            }
        } else {
            html! {
                <div class="modal-container">
                    <h1>{"LED box disconnected"}</h1>
                    <p>{"Please reconnect and restart."}</p>
                </div>
            }
        }
    }

    fn view_mkv_recording_options(&self) -> Html {
        if let Some(ref shared) = self.server_state {
            let available_codecs = shared.available_codecs();

            let selected_idx = match shared.mkv_recording_config.codec {
                ci2_remote_control::MkvCodec::VP8(_) => 0,
                ci2_remote_control::MkvCodec::VP9(_) => 1,
                ci2_remote_control::MkvCodec::H264(_) => 2,
            };

            // TODO: should we bother showing devices if only 1?
            let cuda_select_div = if shared.cuda_devices.len() > 0 {
                let selected_cuda_idx = match &shared.mkv_recording_config.codec {
                    ci2_remote_control::MkvCodec::H264(ref opts) => opts.cuda_device,
                    _ => 0,
                };
                html! {<div>
                    <h5>{"NVIDIA device to use for H264 encoding"}</h5>
                    <VecToggle<String>
                        values=shared.cuda_devices.clone()
                        selected_idx=selected_cuda_idx as usize
                        onsignal=self.link.callback(|item| Msg::ToggleCudaDevice(item as i32))
                    />
                </div>}
            } else {
                html! {<div></div>}
            };

            // TODO: select cuda device

            html! {
                <div class="wrap-collapsible">
                    <CheckboxLabel label="MKV Recording Options" initially_checked=true />
                    <div>
                        <p>{"Record video files."}</p>
                    </div>
                    <div>

                        <div>
                            <RecordingPathWidget
                                label="Record MKV file"
                                value=shared.is_recording_mkv.clone()
                                ontoggle=self.link.callback(|checked| {Msg::ToggleMkvSave(checked)})
                                />
                        </div>
                        <div>
                            <h5>{"MKV Max Framerate"}</h5>
                            <EnumToggle<RecordingFrameRate>
                                value=shared.mkv_recording_config.max_framerate.clone()
                                onsignal=self.link.callback(|variant| Msg::ToggleMkvRecordingFrameRate(variant))
                            />
                        </div>

                        <div>
                            <h5>{"MKV Codec"}</h5>
                            <VecToggle<CodecSelection>
                                values=available_codecs
                                selected_idx=selected_idx
                                onsignal=self.link.callback(|idx| Msg::ToggleMkvCodec(idx))
                            />
                        </div>

                        <div>
                            <h5>{"MKV Bitrate"}</h5>
                            <EnumToggle<BitrateSelection>
                                value=get_bitrate(&shared.mkv_recording_config.codec).unwrap()
                                onsignal=self.link.callback(|variant| Msg::ToggleMkvBitrate(variant))
                            />
                        </div>

                        { cuda_select_div }

                    </div>
                </div>
            }
        } else {
            html! {
                <div></div>
            }
        }
    }

    fn view_post_trigger_options(&self) -> Html {
        html! {
            <div class="wrap-collapsible">
                <CheckboxLabel label="Post Triggering" initially_checked=true />
                <div>
                    <p>{"Acquire video into a large buffer. This enables 'going back in time' to trigger saving of images
                    that were acquired prior to the Post Trigger occurring."}</p>
                </div>
                <div>
                    <label>{"buffer size (number of frames) "}
                        <TypedInput<usize>
                            storage=self.post_trigger_buffer_size_local.clone()
                            on_send_valid=self.link.callback(|v| Msg::SetPostTriggerBufferSize(v))
                            />
                    </label>

                    <Button title="Post Trigger MKV Recording" onsignal=self.link.callback(|_| Msg::PostTriggerMkvRecording)/>
                    {"(Initiates MKV recording as set above. MKV recording must be manually stopped.)"}

                </div>
            </div>
        }
    }

    fn view_fmf_recording_options(&self) -> Html {
        if let Some(ref shared) = self.server_state {
            let ufmf_div = if shared.has_image_tracker_compiled {
                html! {
                    <div>
                    <RecordingPathWidget
                        label="Record µFMF file"
                        value=shared.is_recording_ufmf.clone()
                        ontoggle=self.link.callback(|checked| {Msg::ToggleUfmfSave(checked)})
                        />
                    </div>
                }
            } else {
                html! {}
            };

            html! {
                <div class="wrap-collapsible">
                    <CheckboxLabel label="FMF & µFMF Recording" initially_checked=false />
                    <div>
                        <p>{"Record special-purpose uncompressed video files."}</p>
                    </div>
                    <div>
                        { ufmf_div }
                        <div>
                            <RecordingPathWidget
                                label="Record FMF file (warning: huge files)"
                                value=shared.is_recording_fmf.clone()
                                ontoggle=self.link.callback(|checked| {Msg::ToggleFmfSave(checked)})
                                />
                        </div>
                        <div>
                            <h5>{"Record FMF Framerate"}</h5>
                            <EnumToggle<RecordingFrameRate>
                                value=shared.recording_framerate.clone()
                                onsignal=self.link.callback(|variant| Msg::ToggleFmfRecordingFrameRate(variant))
                            />
                        </div>
                    </div>
                </div>
            }
        } else {
            html! {
                <div></div>
            }
        }
    }

    fn apriltag_detection_ui(&self) -> Html {
        let no_tag_result = html! {
            <div>
            </div>
        };
        if let Some(ref shared) = self.server_state {
            if let Some(ref ts) = shared.apriltag_state {
                html! {
                    <div class="wrap-collapsible">

                        <CheckboxLabel label="April Tag Detection" initially_checked=true />
                        <div>
                            <h5>{"Tag Family"}</h5>
                            <EnumToggle<TagFamily>
                                value=ts.april_family.clone()
                                onsignal=self.link.callback(|variant| Msg::ToggleTagFamily(variant))
                            />
                        </div>
                        <div>

                            <div>
                                <Toggle
                                    label="Enable detection"
                                    value=ts.do_detection
                                    ontoggle=self.link.callback(|checked| {Msg::ToggleAprilTagDetection(checked)})
                                    />
                            </div>

                            <div>
                                <RecordingPathWidget
                                    label="Record detections to CSV file"
                                    value=ts.is_recording_csv.clone()
                                    ontoggle=self.link.callback(|checked| {Msg::ToggleAprilTagDetectionSaveCsv(checked)})
                                    />
                            </div>
                        </div>

                    </div>
                }
            } else {
                no_tag_result
            }
        } else {
            no_tag_result
        }
    }

    fn im_ops_ui(&self) -> Html {
        let empty = html! {
            <div>
            </div>
        };
        if let Some(ref shared) = self.server_state {
            html! {
                <div class="wrap-collapsible">
                    <CheckboxLabel label="ImOps Detection" initially_checked=false />
                    <div>
                        <p>{"⚠ This is an in-development, specialized low-latency detector which detects
                        a bright point in the image and transmits the pixel coordinates to a
                        defined network socket. ⚠"}</p>
                    </div>
                    <div>
                        <div>
                            <Toggle
                                label="Enable detection"
                                value=shared.im_ops_state.do_detection
                                ontoggle=self.link.callback(|checked| {Msg::ToggleImOpsDetection(checked)})
                                />
                        </div>

                        <div>
                            <label>{"Destination (IP:Port)"}
                                <TypedInput<SocketAddr>
                                    storage=self.im_ops_destination_local.clone()
                                    on_send_valid=self.link.callback(|v| Msg::SetImOpsDestination(v))
                                    />
                            </label>
                        </div>


                        <div>
                            <label>{"Source (IP)"}
                                <TypedInput<IpAddr>
                                    storage=self.im_ops_source_local.clone()
                                    on_send_valid=self.link.callback(|v| Msg::SetImOpsSource(v))
                                    />
                            </label>
                        </div>


                        <div>
                            <label>{"Center X"}
                                <TypedInput<u32>
                                    storage=self.im_ops_center_x.clone()
                                    on_send_valid=self.link.callback(|v| Msg::SetImOpsCenterX(v))
                                    />
                            </label>
                        </div>

                        <div>
                            <label>{"Center Y"}
                                <TypedInput<u32>
                                    storage=self.im_ops_center_y.clone()
                                    on_send_valid=self.link.callback(|v| Msg::SetImOpsCenterY(v))
                                    />
                            </label>
                        </div>

                        <div>
                            <label>{"Threshold"}
                                <TypedInput<u8>
                                    storage=self.im_ops_threshold.clone()
                                    on_send_valid=self.link.callback(|v| Msg::SetImOpsTheshold(v))
                                    />
                            </label>
                        </div>

                    </div>
                </div>
            }
        } else {
            empty
        }
    }

    fn point_detection_ui(&self) -> Html {
        if let Some(ref shared) = self.server_state {
            if shared.has_image_tracker_compiled {
                let cfg_clone = shared.im_pt_detect_cfg.clone();
                return html! {
                    <div class="wrap-collapsible">
                        <CheckboxLabel label="Object Detection" initially_checked=true />
                        <div>

                            <div>
                                <Toggle
                                    label="Enable object detection"
                                    value=shared.is_doing_object_detection
                                    ontoggle=self.link.callback(|checked| {Msg::ToggleObjDetection(checked)})
                                    />
                            </div>

                            <div>
                                <RecordingPathWidget
                                    label="Record CSV file"
                                    value=shared.is_saving_im_pt_detect_csv.clone()
                                    ontoggle=self.link.callback(|checked| {Msg::ToggleObjDetectionSaveCsv(checked)})
                                    />
                            </div>

                            <div>
                                <h5>{"CSV Max Rate"}</h5>
                                <EnumToggle<RecordingFrameRate>
                                    value=self.csv_recording_rate.clone()
                                    onsignal=self.link.callback(|variant| Msg::ToggleCsvRecordingRate(variant))
                                />
                            </div>

                            <div>
                                <Toggle
                                    label="Update background model"
                                    value=shared.im_pt_detect_cfg.do_update_background_model
                                    ontoggle=self.link.callback(move |checked| {
                                        let mut cfg_clone2 = cfg_clone.clone();
                                        cfg_clone2.do_update_background_model = checked;
                                        let cfg_str = serde_yaml::to_string(&cfg_clone2).unwrap();
                                        Msg::SetObjDetectionConfig(cfg_str)
                                    })
                                    />
                            </div>
                            <div>
                                <h5>{"Detailed configuration"}</h5>
                                <ConfigField<ImPtDetectCfg>
                                    server_version=Some(shared.im_pt_detect_cfg.clone())
                                    rows=16
                                    onsignal=self.link.callback(|cfg| {Msg::SetObjDetectionConfig(cfg)})
                                    />
                                <div class="reset-background-btn">
                                    <Button title="Take Current Image As Background" onsignal=self.link.callback(|_| Msg::TakeCurrentImageAsBackground)/>
                                    <Button title="Set background to mid-gray" onsignal=self.link.callback(|_| Msg::ClearBackground(127.0))/>
                                </div>
                            </div>
                        </div>
                    </div>
                };
            }
        }
        html! {
            <div></div>
        }
    }

    fn checkerboard_calibration_ui(&self) -> Html {
        #[cfg(feature = "checkercal")]
        {
            if let Some(ref shared) = self.server_state {
                let (ncs, disabled) = {
                    let ref cdata = shared.checkerboard_data;
                    let ncs = format!("{}", cdata.num_checkerboards_collected);
                    (ncs, cdata.num_checkerboards_collected == 0)
                };

                let is_active = true;

                // TODO: add UI for setting checkerboard width and height (num corners)

                let num_checkerboards_collected =
                    format!("Number of checkerboards collected: {}", ncs);

                let checkerboard_debug = if let Some(ref debug) = &shared.checkerboard_save_debug {
                    format!("Saving debug data to {}", debug)
                } else {
                    "".to_string()
                };

                html! {
                    <div class="wrap-collapsible">
                        <CheckboxLabel label="Checkerboard Calibration" />
                        <div>
                            <p>{"This enables estimation of lens distortion parameters."}</p>
                        </div>
                        <div>

                            <Toggle
                                label="Enable checkerboard calibration"
                                value=shared.checkerboard_data.enabled
                                ontoggle=self.link.callback(|checked| {Msg::ToggleCheckerboardDetection(checked)})
                                />

                            <Toggle
                                label="Save debug information"
                                value=shared.checkerboard_save_debug.is_some()
                                ontoggle=self.link.callback(|checked| {Msg::ToggleCheckerboardDebug(checked)})
                                />

                            <div>{checkerboard_debug}</div>

                            <h2>{"Input: Checkerboard Size"}</h2>
                            <p>{"Enter the size of your checkerboard in number of inner corners (e.g. 7 x 7 for a standard chessboard)."}</p>
                            <label>{"width"}
                                <TypedInput<u32>
                                    storage=self.checkerboard_width.clone()
                                    on_send_valid=self.link.callback(|v| Msg::SetCheckerboardWidth(v))
                                    />
                            </label>
                            <label>{"height"}
                                <TypedInput<u32>
                                    storage=self.checkerboard_height.clone()
                                    on_send_valid=self.link.callback(|v| Msg::SetCheckerboardHeight(v))
                                    />
                            </label>

                            <h2>{"Action: Perform Calibration"}</h2>

                            <div>
                                {num_checkerboards_collected}
                            </div>

                            <Button
                                title="Clear Checkerboards"
                                onsignal=self.link.callback(move |_| Msg::ClearCheckerboards)
                                />

                            <Button
                                title="Perform and Save Calibration"
                                disabled=disabled
                                is_active=is_active
                                onsignal=self.link.callback(move |_| Msg::PerformCheckerboardCalibration)
                                />

                        </div>
                    </div>
                }
            } else {
                html! {
                    <div></div>
                }
            }
        }
        #[cfg(not(feature = "checkercal"))]
        {
            html! {
                <div></div>
            }
        }
    }

    fn view_kalman_tracking(&self) -> Html {
        #[cfg(feature = "flydratrax")]
        {
            if let Some(ref shared) = self.server_state {
                html! {
                    <div class="wrap-collapsible">
                        <CheckboxLabel label="Kalman tracking" initially_checked=false />
                        <div>
                            <div>
                                <h5>{"Kalman tracking configuration"}</h5>
                                <ConfigField<KalmanTrackingConfig>
                                    server_version=Some(shared.kalman_tracking_config.clone())
                                    rows=5
                                    onsignal=self.link.callback(|cfg| {Msg::CamArgSetKalmanTrackingConfig(cfg)})
                                    />
                            </div>
                        </div>
                    </div>
                }
            } else {
                html! {
                    <div></div>
                }
            }
        }
        #[cfg(not(feature = "flydratrax"))]
        {
            html! {
                <div></div>
            }
        }
    }

    fn view_led_triggering(&self) -> Html {
        #[cfg(feature = "flydratrax")]
        {
            if let Some(ref shared) = self.server_state {
                html! {
                    <div class="wrap-collapsible">
                        <CheckboxLabel label="Online LED triggering" initially_checked=false />

                            <div>
                                <h5>{"Led program configuration"}</h5>
                                <ConfigField<LedProgramConfig>
                                    server_version=Some(shared.led_program_config.clone())
                                    rows=7
                                    onsignal=self.link.callback(|cfg| {Msg::CamArgSetLedProgramConfig(cfg)})
                                    />
                            </div>
                    </div>
                }
            } else {
                html! {
                    <div></div>
                }
            }
        }
        #[cfg(not(feature = "flydratrax"))]
        {
            html! {
                <div></div>
            }
        }
    }

    fn view_gain(&self) -> Html {
        if let Some(ref shared) = self.server_state {
            if let Some(gain_auto) = shared.gain_auto {
                return html! {
                    <div class=classes!("gain-main","cam-range-main")>
                        <h3>{ "Gain" }</h3>
                        <div class="cam-range-inner">
                            <AutoModeSelect mode=gain_auto onsignal=self.link.callback(|g| {Msg::SetGainAuto(g)}) />
                            <RangedValue
                                unit=shared.gain.unit.clone()
                                min=shared.gain.min as f32
                                max=shared.gain.max as f32
                                current=shared.gain.current as f32
                                current_value_label=LAST_DETECTED_VALUE_LABEL
                                placeholder=shared.gain.name.clone()
                                onsignal=self.link.callback(|v| {Msg::SetGainValue(v as f64)})
                                />
                        </div>
                    </div>
                };
            }
        }
        html! {
            <div></div>
        }
    }

    fn view_exposure(&self) -> Html {
        if let Some(ref shared) = self.server_state {
            if let Some(exposure_auto) = shared.exposure_auto {
                return html! {
                    <div class=classes!("exposure-main","cam-range-main")>
                        <h3>{ "Exposure Time" }</h3>
                        <div class="cam-range-inner">
                            <AutoModeSelect mode=exposure_auto onsignal=self.link.callback(|g| {Msg::SetExposureAuto(g)}) />
                            <RangedValue
                                unit=shared.exposure_time.unit.clone()
                                min=shared.exposure_time.min as f32
                                max=shared.exposure_time.max as f32
                                current=shared.exposure_time.current as f32
                                current_value_label=LAST_DETECTED_VALUE_LABEL
                                placeholder=shared.exposure_time.name.clone()
                                onsignal=self.link.callback(|v| {Msg::SetExposureValue(v as f64)})
                                />
                        </div>
                    </div>
                };
            }
        }
        html! {
            <div></div>
        }
    }

    fn view_frame_rate_limit(&self) -> Html {
        if let Some(ref shared) = self.server_state {
            if let Some(ref frl) = shared.frame_rate_limit {
                html! {
                    <div class=classes!("frame-rate-main","cam-range-main")>
                        <h3>{ "Maximum Frame Rate" }</h3>
                            <div class="auto-mode-container">
                                <div class="auto-mode-label">
                                    {"Limit Frame Rate: "}
                                </div>
                                <div class="auto-mode-buttons">
                                    <EnumToggle<bool>
                                        value=shared.frame_rate_limit_enabled
                                        onsignal=self.link.callback(|variant| Msg::SetFrameRateLimitEnabled(variant))
                                    />
                                </div>
                            </div>
                        <div class="cam-range-inner">
                            <RangedValue
                                unit=frl.unit.clone()
                                min=frl.min as f32
                                max=frl.max as f32
                                current=frl.current as f32
                                current_value_label=LAST_DETECTED_VALUE_LABEL
                                placeholder=frl.name.clone()
                                onsignal=self.link.callback(|v| {Msg::SetFrameRateLimit(v as f64)})
                                />
                        </div>
                    </div>
                }
            } else {
                html! {
                    <div></div>
                }
            }
        } else {
            html! {
                <div></div>
            }
        }
    }
}

fn send_cam_message(args: CamArg, model: &mut Model) -> Option<FetchTask> {
    model.send_message(&CallbackType::ToCamera(args))
}

fn to_rate(rate_enum: &RecordingFrameRate) -> Option<f32> {
    match rate_enum {
        RecordingFrameRate::Fps1 => Some(1.0),
        RecordingFrameRate::Fps2 => Some(2.0),
        RecordingFrameRate::Fps5 => Some(5.0),
        RecordingFrameRate::Fps10 => Some(10.0),
        RecordingFrameRate::Fps20 => Some(20.0),
        RecordingFrameRate::Fps25 => Some(25.0),
        RecordingFrameRate::Fps30 => Some(30.0),
        RecordingFrameRate::Fps100 => Some(100.0),
        RecordingFrameRate::Unlimited => None,
    }
}

impl Model {
    fn send_message(&mut self, args: &CallbackType) -> Option<yew::services::fetch::FetchTask> {
        let post_request = Request::post("callback")
            .header("Content-Type", "application/json;charset=UTF-8")
            .body(Json(&args))
            .expect("Failed to build request.");

        let callback =
            self.link
                .callback(move |response: Response<Json<Result<(), anyhow::Error>>>| {
                    if let (meta, Json(Ok(_body))) = response.into_parts() {
                        if meta.status.is_success() {
                            return Msg::Ignore;
                        }
                    }
                    log::error!("failed sending message");
                    Msg::Ignore
                });
        let mut options = FetchOptions::default();
        options.credentials = Some(Credentials::SameOrigin);
        match FetchService::fetch_with_options(post_request, options, callback) {
            Ok(task) => Some(task),
            Err(err) => {
                log::error!("sending message failed with error: {}", err);
                None
            }
        }
    }
}

fn get_strand_cam_name(server_state: Option<&ServerState>) -> &'static str {
    if server_state.map(|x| x.is_braid).unwrap_or(false) {
        "Braid - Strand Cam "
    } else {
        "Strand Cam "
    }
}

fn get_bitrate(bitrate: &ci2_remote_control::MkvCodec) -> Result<BitrateSelection, ()> {
    use crate::BitrateSelection::*;
    let bitrate: u32 = match bitrate {
        ci2_remote_control::MkvCodec::VP8(c) => c.bitrate,
        ci2_remote_control::MkvCodec::VP9(c) => c.bitrate,
        ci2_remote_control::MkvCodec::H264(c) => c.bitrate,
    };
    let x = match bitrate {
        500 => Bitrate500,
        1000 => Bitrate1000,
        2000 => Bitrate2000,
        3000 => Bitrate3000,
        4000 => Bitrate4000,
        5000 => Bitrate5000,
        10000 => Bitrate10000,
        _ => return Err(()),
    };
    Ok(x)
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
enum BitrateSelection {
    Bitrate500,
    Bitrate1000,
    Bitrate2000,
    Bitrate3000,
    Bitrate4000,
    Bitrate5000,
    Bitrate10000,
}

impl BitrateSelection {
    fn to_u32(&self) -> u32 {
        use crate::BitrateSelection::*;
        match self {
            Bitrate500 => 500,
            Bitrate1000 => 1000,
            Bitrate2000 => 2000,
            Bitrate3000 => 3000,
            Bitrate4000 => 4000,
            Bitrate5000 => 5000,
            Bitrate10000 => 10000,
        }
    }
}

impl Default for BitrateSelection {
    fn default() -> BitrateSelection {
        BitrateSelection::Bitrate1000
    }
}

impl std::fmt::Display for BitrateSelection {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.to_u32())
    }
}

impl enum_iter::EnumIter for BitrateSelection {
    fn variants() -> &'static [Self] {
        &[
            BitrateSelection::Bitrate500,
            BitrateSelection::Bitrate1000,
            BitrateSelection::Bitrate2000,
            BitrateSelection::Bitrate3000,
            BitrateSelection::Bitrate4000,
            BitrateSelection::Bitrate5000,
            BitrateSelection::Bitrate10000,
        ]
    }
}

// -------

#[derive(Clone, PartialEq)]
enum CodecSelection {
    VP8,
    VP9,
    H264,
}

impl CodecSelection {
    fn get_codec(&self, old: &ci2_remote_control::MkvCodec) -> ci2_remote_control::MkvCodec {
        use crate::CodecSelection::*;
        let bitrate = match old {
            ci2_remote_control::MkvCodec::VP8(c) => c.bitrate,
            ci2_remote_control::MkvCodec::VP9(c) => c.bitrate,
            ci2_remote_control::MkvCodec::H264(c) => c.bitrate,
        };
        match self {
            VP8 => ci2_remote_control::MkvCodec::VP8(ci2_remote_control::VP8Options { bitrate }),
            VP9 => ci2_remote_control::MkvCodec::VP9(ci2_remote_control::VP9Options { bitrate }),
            H264 => ci2_remote_control::MkvCodec::H264(ci2_remote_control::H264Options {
                bitrate,
                cuda_device: 0,
            }),
        }
    }
}

impl Default for CodecSelection {
    fn default() -> CodecSelection {
        CodecSelection::VP9
    }
}

impl std::fmt::Display for CodecSelection {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let x = match self {
            CodecSelection::VP8 => "VP8",
            CodecSelection::VP9 => "VP9",
            CodecSelection::H264 => "H264",
        };
        write!(f, "{}", x)
    }
}

impl enum_iter::EnumIter for CodecSelection {
    fn variants() -> &'static [Self] {
        &[
            CodecSelection::VP8,
            CodecSelection::VP9,
            CodecSelection::H264,
        ]
    }
}

trait HasAvail {
    fn available_codecs(&self) -> Vec<CodecSelection>;
}

impl HasAvail for ServerState {
    fn available_codecs(&self) -> Vec<CodecSelection> {
        if self.cuda_devices.len() > 0 {
            vec![
                CodecSelection::VP8,
                CodecSelection::VP9,
                CodecSelection::H264,
            ]
        } else {
            vec![CodecSelection::VP8, CodecSelection::VP9]
        }
    }
}

#[wasm_bindgen(start)]
pub fn run_app() {
    wasm_logger::init(wasm_logger::Config::default());
    yew::start_app::<Model>();
}
