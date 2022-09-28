#![recursion_limit = "1000"]

use std::{
    cell::RefCell,
    error::Error,
    fmt::{self, Debug, Display, Formatter},
    net::{IpAddr, SocketAddr},
    rc::Rc,
};

use ci2_remote_control::CamArg;

use led_box_comms::ToDevice as ToLedBoxDevice;

use serde::{Deserialize, Serialize};

use gloo_events::EventListener;
use wasm_bindgen::prelude::*;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Event, EventSource, MessageEvent};

use yew::prelude::*;

use ads_webasm::components::{EnumToggle, VecToggle};

use http_video_streaming_types::FirehoseCallbackInner;
use http_video_streaming_types::ToClient as FirehoseImageData;

use strand_cam_storetype::CallbackType;
use strand_cam_storetype::StoreType as ServerState;
use strand_cam_storetype::{KalmanTrackingConfig, LedProgramConfig};
use yew_tincture::components::CheckboxLabel;

use ci2_remote_control::{RecordingFrameRate, TagFamily};
use ci2_types::AutoMode;

use flydra_feature_detector_types::ImPtDetectCfg;
use yew_tincture::components::{TypedInput, TypedInputStorage};

// use yew::services::fetch::{Credentials, FetchOptions, FetchService, FetchTask, Request, Response};

// use yew_event_source::{EventSourceService, EventSourceStatus, EventSourceTask, ReadyState};

use ads_webasm::video_data::VideoData;

mod components;
use crate::components::AutoModeSelect;

use ads_webasm::components::{
    ConfigField, RangedValue, RecordingPathWidget, ReloadButton, Toggle, VideoField,
};
use yew_tincture::components::Button;

use components::LedBoxControl;

const LAST_DETECTED_VALUE_LABEL: &str = "Last detected value: ";

enum Msg {
    NewImageFrame(FirehoseImageData),
    RenderedImage(FirehoseCallbackInner),

    NewServerState(Box<ServerState>),

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

    CamArgSetKalmanTrackingConfig(String),
    CamArgSetLedProgramConfig(String),

    ToggleFmfSave(bool),
    ToggleFmfRecordingFrameRate(RecordingFrameRate),

    // only used when image-tracker crate used
    ToggleUfmfSave(bool),

    ToggleMkvSave(bool),
    ToggleMkvRecordingFrameRate(RecordingFrameRate),
    ToggleMkvBitrate(BitrateSelection),
    ToggleMkvCodec(String),
    ToggleCudaDevice(String),

    // only used when image-tracker crate used
    TakeCurrentImageAsBackground,
    // only used when image-tracker crate used
    ClearBackground(f32),

    LedBoxControlEvent(ToLedBoxDevice),

    ToggleCheckerboardDetection(bool),
    ToggleCheckerboardDebug(bool),
    SetCheckerboardWidth(u32),
    SetCheckerboardHeight(u32),
    PerformCheckerboardCalibration,
    ClearCheckerboards,

    SetPostTriggerBufferSize(usize),
    PostTriggerMkvRecording,

    SendMessageFetchState(FetchState),
}

// -----------------------------------------------------------------------------

pub enum FetchState {
    Fetching,
    Success,
    Failed(FetchError),
}

// -----------------------------------------------------------------------------

/// Something wrong has occurred while fetching an external resource.
#[derive(Debug, Clone, PartialEq)]
pub struct FetchError {
    err: JsValue,
}
impl Display for FetchError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        Debug::fmt(&self.err, f)
    }
}
impl Error for FetchError {}

impl From<JsValue> for FetchError {
    fn from(value: JsValue) -> Self {
        Self { err: value }
    }
}

// -----------------------------------------------------------------------------

struct Model {
    video_data: Rc<RefCell<VideoData>>,

    server_state: Option<Box<ServerState>>,
    json_decode_err: Option<String>,
    html_page_title: Option<String>,
    es: EventSource,
    _data_listener: EventListener,
    _stream_listener: EventListener,

    csv_recording_rate: RecordingFrameRate,
    checkerboard_width: TypedInputStorage<u32>,
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

    fn create(ctx: &Context<Self>) -> Self {
        let es = EventSource::new(strand_cam_storetype::STRAND_CAM_EVENTS_URL_PATH)
            .map_err(|js_value: JsValue| {
                let err: js_sys::Error = js_value.dyn_into().unwrap_throw();
                err
            })
            .unwrap_throw();
        let data_callback =
            ctx.link()
                .callback(|bufstr: String| match serde_json::from_str(&bufstr) {
                    Ok(msg) => Msg::NewServerState(msg),
                    Err(e) => {
                        log::error!("in data callback: {}", e);
                        Msg::FailedCallbackJsonDecode(format!("{}", e))
                    }
                });
        let stream_callback =
            ctx.link()
                .callback(|bufstr: String| match serde_json::from_str(&bufstr) {
                    Ok(image_result) => Msg::NewImageFrame(image_result),
                    Err(e) => {
                        log::error!("in stream callback: {}", e);
                        Msg::FailedCallbackJsonDecode(format!("{}", e))
                    }
                });

        let data_listener = EventListener::new(
            &es,
            strand_cam_storetype::STRAND_CAM_EVENT_NAME,
            move |event: &Event| {
                let event = event.dyn_ref::<MessageEvent>().unwrap_throw();
                let text = event.data().as_string().unwrap_throw();
                data_callback.emit(text);
            },
        );

        let stream_listener = EventListener::new(
            &es,
            http_video_streaming_types::VIDEO_STREAM_EVENT_NAME,
            move |event: &Event| {
                let event = event.dyn_ref::<MessageEvent>().unwrap_throw();
                let text = event.data().as_string().unwrap_throw();
                stream_callback.emit(text);
            },
        );

        // let task = {
        //     // let notification = link.callback(|status| {
        //     //     if status == EventSourceStatus::Error {
        //     //         log::error!("event source error");
        //     //     }
        //     //     Msg::EsCheckState
        //     // });
        //     // let mut task = EventSourceService::new()
        //     //     .connect(
        //     //         strand_cam_storetype::STRAND_CAM_EVENTS_URL_PATH,
        //     //         notification,
        //     //     )
        //     //     .unwrap();
        //     task
        // };

        Self {
            video_data: Rc::new(RefCell::new(VideoData::new(None))),
            server_state: None,
            json_decode_err: None,
            html_page_title: None,
            es,
            _data_listener: data_listener,
            _stream_listener: stream_listener,
            csv_recording_rate: RecordingFrameRate::Unlimited,
            checkerboard_width: TypedInputStorage::empty(),
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

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::SendMessageFetchState(_fetch_state) => {
                return false;
            }
            Msg::NewImageFrame(in_msg) => {
                *self.video_data.borrow_mut() = VideoData::new(Some(in_msg));
            }
            Msg::RenderedImage(fci) => {
                self.send_message(CallbackType::FirehoseNotify(fci), ctx);
            }
            Msg::NewServerState(response) => {
                // Set the html page title once.
                if self.html_page_title.is_none() {
                    let strand_cam_name =
                        get_strand_cam_name(self.server_state.as_ref().map(AsRef::as_ref));
                    let title = format!("{} - {}", response.camera_name, strand_cam_name);
                    web_sys::window()
                        .unwrap()
                        .document()
                        .unwrap()
                        .set_title(&title);
                    self.html_page_title = Some(title);
                }

                // Do this only if user is not focused on field.
                self.checkerboard_width
                    .set_if_not_focused(response.checkerboard_data.width);
                self.checkerboard_height
                    .set_if_not_focused(response.checkerboard_data.height);

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
                self.send_cam_message(
                    CamArg::SetIngoreFutureFrameProcessingErrors(limit_duration),
                    ctx,
                );
                return false; // don't update DOM, do that on return
            }
            Msg::SetIgnoreAllFutureErrors(val) => {
                self.ignore_all_future_frame_processing_errors = val;
            }
            Msg::SetGainAuto(v) => {
                self.send_cam_message(CamArg::SetGainAuto(v), ctx);
                return false; // don't update DOM, do that on return
            }
            Msg::SetGainValue(v) => {
                self.send_cam_message(CamArg::SetGain(v), ctx);
                return false; // don't update DOM, do that on return
            }
            Msg::SetExposureAuto(v) => {
                self.send_cam_message(CamArg::SetExposureAuto(v), ctx);
                return false; // don't update DOM, do that on return
            }
            Msg::SetExposureValue(v) => {
                self.send_cam_message(CamArg::SetExposureTime(v), ctx);
                return false; // don't update DOM, do that on return
            }
            Msg::SetFrameRateLimitEnabled(v) => {
                self.send_cam_message(CamArg::SetFrameRateLimitEnabled(v), ctx);
                return false; // don't update DOM, do that on return
            }
            Msg::SetFrameRateLimit(v) => {
                self.send_cam_message(CamArg::SetFrameRateLimit(v), ctx);
                return false; // don't update DOM, do that on return
            }
            // only used when image-tracker crate used
            Msg::SetObjDetectionConfig(v) => {
                self.send_cam_message(CamArg::SetObjDetectionConfig(v), ctx);
                return false; // don't update DOM, do that on return
            }
            Msg::CamArgSetKalmanTrackingConfig(v) => {
                self.send_cam_message(CamArg::CamArgSetKalmanTrackingConfig(v), ctx);
                return false; // don't update DOM, do that on return
            }
            Msg::CamArgSetLedProgramConfig(v) => {
                self.send_cam_message(CamArg::CamArgSetLedProgramConfig(v), ctx);
                return false; // don't update DOM, do that on return
            }
            // only used when image-tracker crate used
            Msg::ToggleObjDetection(v) => {
                self.send_cam_message(CamArg::SetIsDoingObjDetection(v), ctx);
                return false; // don't update DOM, do that on return
            }
            // only used when image-tracker crate used
            Msg::ToggleObjDetectionSaveCsv(v) => {
                let cfg = if v {
                    ci2_remote_control::CsvSaveConfig::Saving(to_rate(&self.csv_recording_rate))
                } else {
                    ci2_remote_control::CsvSaveConfig::NotSaving
                };
                self.send_cam_message(CamArg::SetIsSavingObjDetectionCsv(cfg), ctx);
                return false; // don't update DOM, do that on return
            }
            // only used when image-tracker crate used
            Msg::ToggleCsvRecordingRate(v) => {
                self.csv_recording_rate = v;
                return false; // don't update DOM, do that on return
            }
            Msg::ToggleTagFamily(v) => {
                self.send_cam_message(CamArg::ToggleAprilTagFamily(v), ctx);
                return false; // don't update DOM, do that on return
            }
            Msg::ToggleAprilTagDetection(v) => {
                self.send_cam_message(CamArg::ToggleAprilTagDetection(v), ctx);
                return false; // don't update DOM, do that on return
            }
            Msg::ToggleAprilTagDetectionSaveCsv(v) => {
                self.send_cam_message(CamArg::SetIsRecordingAprilTagCsv(v), ctx);
                return false; // don't update DOM, do that on return
            }
            Msg::ToggleImOpsDetection(v) => {
                self.send_cam_message(CamArg::ToggleImOpsDetection(v), ctx);
                return false; // don't update DOM, do that on return
            }
            Msg::SetImOpsDestination(v) => {
                self.send_cam_message(CamArg::SetImOpsDestination(v), ctx);
                return false; // don't update DOM, do that on return
            }
            Msg::SetImOpsSource(v) => {
                self.send_cam_message(CamArg::SetImOpsSource(v), ctx);
                return false; // don't update DOM, do that on return
            }
            Msg::SetImOpsCenterX(v) => {
                self.send_cam_message(CamArg::SetImOpsCenterX(v), ctx);
                return false; // don't update DOM, do that on return
            }
            Msg::SetImOpsCenterY(v) => {
                self.send_cam_message(CamArg::SetImOpsCenterY(v), ctx);
                return false; // don't update DOM, do that on return
            }
            Msg::SetImOpsTheshold(v) => {
                self.send_cam_message(CamArg::SetImOpsThreshold(v), ctx);
                return false; // don't update DOM, do that on return
            }
            Msg::ToggleFmfRecordingFrameRate(v) => {
                self.send_cam_message(CamArg::SetRecordingFps(v), ctx);
                return false; // don't update DOM, do that on return
            }
            Msg::ToggleMkvRecordingFrameRate(v) => {
                self.send_cam_message(CamArg::SetMkvRecordingFps(v), ctx);
                return false; // don't update DOM, do that on return
            }
            Msg::ToggleMkvBitrate(v) => {
                let mut old_config = if let Some(ref state) = self.server_state {
                    state.mkv_recording_config.clone()
                } else {
                    ci2_remote_control::MkvRecordingConfig::default()
                };
                match &mut old_config.codec {
                    ci2_remote_control::MkvCodec::Uncompressed => {}
                    ci2_remote_control::MkvCodec::VP8(ref mut o) => o.bitrate = v.to_u32(),
                    ci2_remote_control::MkvCodec::VP9(ref mut o) => o.bitrate = v.to_u32(),
                    ci2_remote_control::MkvCodec::H264(ref mut o) => o.bitrate = v.to_u32(),
                }
                self.send_cam_message(CamArg::SetMkvRecordingConfig(old_config), ctx);
                return false; // don't update DOM, do that on return
            }
            Msg::ToggleMkvCodec(name) => {
                if let Some(ref shared) = self.server_state {
                    let available_codecs = shared.available_codecs();
                    let opt_idx = available_codecs
                        .iter()
                        .position(|c| &format!("{c}") == &name);
                    if let Some(idx) = opt_idx {
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
                            ..Default::default()
                        };
                        self.send_cam_message(CamArg::SetMkvRecordingConfig(cfg), ctx);
                    }
                }
                return false; // don't update DOM, do that on return
            }
            Msg::ToggleCudaDevice(cuda_device) => {
                if let Some(ref shared) = self.server_state {
                    let mut cfg = shared.mkv_recording_config.clone();
                    // TODO: right now, the selected CUDA device is a property
                    // of the H264 codec options. This means that if a different
                    // codec is selected, the user's choice is forgotten.
                    let cuda_device = cuda_device.parse().unwrap();
                    if let ci2_remote_control::MkvCodec::H264(ref mut opts) = &mut cfg.codec {
                        opts.cuda_device = cuda_device;
                    }
                    self.send_cam_message(CamArg::SetMkvRecordingConfig(cfg), ctx);
                }
                return false; // don't update DOM, do that on return
            }
            Msg::ToggleFmfSave(v) => {
                self.send_cam_message(CamArg::SetIsRecordingFmf(v), ctx);
                return false; // don't update DOM, do that on return
            }
            // only used when image-tracker crate used
            Msg::ToggleUfmfSave(v) => {
                self.send_cam_message(CamArg::SetIsRecordingUfmf(v), ctx);
                return false; // don't update DOM, do that on return
            }
            Msg::ToggleMkvSave(v) => {
                self.send_cam_message(CamArg::SetIsRecordingMkv(v), ctx);
                return false; // don't update DOM, do that on return
            }
            // only used when image-tracker crate used
            Msg::TakeCurrentImageAsBackground => {
                self.send_message(CallbackType::TakeCurrentImageAsBackground, ctx);
                return false; // don't update DOM, do that on return
            }
            // only used when image-tracker crate used
            Msg::ClearBackground(value) => {
                self.send_message(CallbackType::ClearBackground(value), ctx);
                return false; // don't update DOM, do that on return
            }
            Msg::LedBoxControlEvent(command) => {
                self.send_message(CallbackType::ToLedBox(command), ctx);
                return false; // don't update DOM, do that on return
            }
            Msg::ToggleCheckerboardDetection(val) => {
                self.send_cam_message(CamArg::ToggleCheckerboardDetection(val), ctx);
                return false;
            }
            Msg::ToggleCheckerboardDebug(val) => {
                self.send_cam_message(CamArg::ToggleCheckerboardDebug(val), ctx);
                return false;
            }
            Msg::SetCheckerboardWidth(val) => {
                self.send_cam_message(CamArg::SetCheckerboardWidth(val), ctx);
                return false;
            }
            Msg::SetCheckerboardHeight(val) => {
                self.send_cam_message(CamArg::SetCheckerboardHeight(val), ctx);
                return false;
            }
            Msg::PerformCheckerboardCalibration => {
                self.send_cam_message(CamArg::PerformCheckerboardCalibration, ctx);
                return false;
            }
            Msg::ClearCheckerboards => {
                self.send_cam_message(CamArg::ClearCheckerboards, ctx);
                return false;
            }

            Msg::SetPostTriggerBufferSize(val) => {
                self.send_cam_message(CamArg::SetPostTriggerBufferSize(val), ctx);
                return false;
            }

            Msg::PostTriggerMkvRecording => {
                self.send_cam_message(CamArg::PostTrigger, ctx);
                return false; // don't update DOM, do that on return
            }
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let strand_cam_name = get_strand_cam_name(self.server_state.as_ref().map(AsRef::as_ref));
        html! {
            <div>
                <h1 style="text-align: center;">{strand_cam_name}<a href="https://strawlab.org/strand-cam/"><span class="infoCircle">{"ℹ"}</span></a></h1>
                <img src="strand-camera-no-text.png" width="521" height="118" class="center logo-img" alt="Strand Camera logo"/>
                { self.disconnected_dialog() }
                { self.frame_processing_error_dialog(ctx) }
                { self.led_box_failed() }
                <div class="wrapper">
                    { self.view_video(ctx) }
                    { self.view_decode_error(ctx) }
                    { self.view_led_box(ctx) }
                    { self.view_led_triggering(ctx) }
                    { self.view_mkv_recording_options(ctx) }
                    { self.view_post_trigger_options(ctx) }
                    { self.point_detection_ui(ctx) }
                    { self.apriltag_detection_ui(ctx) }
                    { self.im_ops_ui(ctx) }
                    { self.checkerboard_calibration_ui(ctx) }

                    <div class="wrap-collapsible">
                        <CheckboxLabel label="Camera Settings" initially_checked=true />
                        <div>
                            <p>{"Set values on the camera itself."}</p>
                        </div>
                        <div>
                            { self.view_gain(ctx) }
                            { self.view_exposure(ctx) }
                            { self.view_frame_rate_limit(ctx) }
                        </div>
                    </div>
                    { self.view_fmf_recording_options(ctx) }
                    { self.view_kalman_tracking(ctx) }
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
    fn send_message(&self, val: CallbackType, ctx: &Context<Self>) {
        ctx.link().send_future(async move {
            match post_message(&val).await {
                Ok(()) => Msg::SendMessageFetchState(FetchState::Success),
                Err(err) => Msg::SendMessageFetchState(FetchState::Failed(err)),
            }
        });
    }

    fn send_cam_message(&self, args: CamArg, ctx: &Context<Self>) {
        self.send_message(CallbackType::ToCamera(args), ctx);
    }

    fn view_decode_error(&self, ctx: &Context<Self>) -> Html {
        if let Some(ref json_decode_err) = self.json_decode_err {
            html! {
                <div>
                    <p>{"Error decoding callback JSON from server: "}{json_decode_err}</p>
                    <p><Button title={"Dismiss"} onsignal={ctx.link().callback(|_| Msg::DismissJsonDecodeError)} /></p>
                </div>
            }
        } else {
            html! {}
        }
    }

    fn view_led_box(&self, ctx: &Context<Self>) -> Html {
        if let Some(ref shared) = self.server_state {
            if let Some(ref device_state) = shared.led_box_device_state {
                return html! {
                    <LedBoxControl
                        device_state={device_state.clone()}
                        onsignal={ctx.link().callback(|x| Msg::LedBoxControlEvent(x))}
                    />
                };
            }
        }
        html! {
            <div>{""}</div>
        }
    }

    fn view_video(&self, ctx: &Context<Self>) -> Html {
        if let Some(ref shared) = self.server_state {
            let title = format!("Live view - {}", shared.camera_name);
            html! {
                <VideoField title={title}
                    video_data={self.video_data.clone()}
                    width={shared.image_width}
                    height={shared.image_height}
                    measured_fps={shared.measured_fps}
                    onrendered={ctx.link().callback(|im_data2| {
                        Msg::RenderedImage(im_data2)
                    })}
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
        // 0: connecting, 1: open, 2: closed
        if self.es.ready_state() != 2 {
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

    fn frame_processing_error_dialog(&self, ctx: &Context<Self>) -> Html {
        if let Some(ref shared) = self.server_state {
            if shared.had_frame_processing_error {
                return {
                    html! {
                    <div class="modal-container">
                        <h1> { "Error: frame processing too slow" } </h1>
                        <p>{"Processing of image frames is taking too long. Reduce the computational cost of image processing."}</p>
                        <p><Toggle
                                label={"Ignore all future errors"}
                                value={self.ignore_all_future_frame_processing_errors}
                                ontoggle={ctx.link().callback(|checked| {
                                    Msg::SetIgnoreAllFutureErrors(checked)
                                })}
                            /></p>
                        <p><Button title={"Dismiss"} onsignal={ctx.link().callback(|_| Msg::DismissProcessingErrorModal)} /></p>
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

    fn led_box_failed(&self) -> Html {
        let led_box_device_lost = if let Some(ref shared) = self.server_state {
            shared.led_box_device_lost
        } else {
            false
        };

        if !led_box_device_lost {
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

    fn view_mkv_recording_options(&self, ctx: &Context<Self>) -> Html {
        if let Some(ref shared) = self.server_state {
            let available_codecs = shared.available_codecs();

            let selected_codec = if let Some(codec) =
                match_avail(&available_codecs, &shared.mkv_recording_config.codec)
            {
                format!("{codec}")
            } else {
                log::warn!(
                    "Could not find codec {:?} among available {:?}",
                    shared.mkv_recording_config.codec,
                    available_codecs
                );
                "".to_string()
            };

            // TODO: should we bother showing devices if only 1?
            let cuda_select_div = if !shared.cuda_devices.is_empty() {
                let selected_cuda = match &shared.mkv_recording_config.codec {
                    ci2_remote_control::MkvCodec::H264(ref opts) => {
                        Some(format!("{}", opts.cuda_device))
                    }
                    _ => None,
                };
                html! {<div>
                    <h5>{"NVIDIA device to use for H264 encoding"}</h5>
                    <VecToggle<String>
                        values={shared.cuda_devices.clone()}
                        selected={selected_cuda}
                        onsignal={ctx.link().callback(|name| Msg::ToggleCudaDevice(name))}
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
                                label={"Record MKV file"}
                                value={shared.is_recording_mkv.clone()}
                                ontoggle={ctx.link().callback(|checked| {Msg::ToggleMkvSave(checked)})}
                                />
                        </div>
                        <div>
                            <h5>{"MKV Max Framerate"}</h5>
                            <EnumToggle<RecordingFrameRate>
                                value={shared.mkv_recording_config.max_framerate.clone()}
                                onsignal={ctx.link().callback(Msg::ToggleMkvRecordingFrameRate)}
                            />
                        </div>

                        <div>
                            <h5>{"MKV Codec"}</h5>
                            <VecToggle<CodecSelection>
                                values={available_codecs}
                                selected={Some(selected_codec)}
                                onsignal={ctx.link().callback(Msg::ToggleMkvCodec)}
                            />
                        </div>

                        <div>
                            <h5>{"MKV Bitrate"}</h5>
                            <EnumToggle<BitrateSelection>
                                value={get_bitrate(&shared.mkv_recording_config.codec).unwrap()}
                                onsignal={ctx.link().callback(Msg::ToggleMkvBitrate)}
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

    fn view_post_trigger_options(&self, ctx: &Context<Self>) -> Html {
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
                            storage={self.post_trigger_buffer_size_local.clone()}
                            on_send_valid={ctx.link().callback(Msg::SetPostTriggerBufferSize)}
                            />
                    </label>

                    <Button title={"Post Trigger MKV Recording"} onsignal={ctx.link().callback(|_| Msg::PostTriggerMkvRecording)}/>
                    {"(Initiates MKV recording starting with buffered frames. MKV recording must be manually stopped.)"}

                </div>
            </div>
        }
    }

    fn view_fmf_recording_options(&self, ctx: &Context<Self>) -> Html {
        if let Some(ref shared) = self.server_state {
            let ufmf_div = if shared.has_image_tracker_compiled {
                html! {
                    <div>
                    <RecordingPathWidget
                        label={"Record µFMF file"}
                        value={shared.is_recording_ufmf.clone()}
                        ontoggle={ctx.link().callback(|checked| {Msg::ToggleUfmfSave(checked)})}
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
                                label={"Record FMF file (warning: huge files)"}
                                value={shared.is_recording_fmf.clone()}
                                ontoggle={ctx.link().callback(|checked| {Msg::ToggleFmfSave(checked)})}
                                />
                        </div>
                        <div>
                            <h5>{"Record FMF Framerate"}</h5>
                            <EnumToggle<RecordingFrameRate>
                                value={shared.recording_framerate.clone()}
                                onsignal={ctx.link().callback(Msg::ToggleFmfRecordingFrameRate)}
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

    fn apriltag_detection_ui(&self, ctx: &Context<Self>) -> Html {
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
                                value={ts.april_family.clone()}
                                onsignal={ctx.link().callback(Msg::ToggleTagFamily)}
                            />
                        </div>
                        <div>

                            <div>
                                <Toggle
                                    label={"Enable detection"}
                                    value={ts.do_detection}
                                    ontoggle={ctx.link().callback(|checked| {Msg::ToggleAprilTagDetection(checked)})}
                                    />
                            </div>

                            <div>
                                <RecordingPathWidget
                                    label={"Record detections to CSV file"}
                                    value={ts.is_recording_csv.clone()}
                                    ontoggle={ctx.link().callback(|checked| {Msg::ToggleAprilTagDetectionSaveCsv(checked)})}
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

    fn im_ops_ui(&self, ctx: &Context<Self>) -> Html {
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
                                label={"Enable detection"}
                                value={shared.im_ops_state.do_detection}
                                ontoggle={ctx.link().callback(|checked| {Msg::ToggleImOpsDetection(checked)})}
                                />
                        </div>

                        <div>
                            <label>{"Destination (IP:Port)"}
                                <TypedInput<SocketAddr>
                                    storage={self.im_ops_destination_local.clone()}
                                    on_send_valid={ctx.link().callback(Msg::SetImOpsDestination)}
                                    />
                            </label>
                        </div>


                        <div>
                            <label>{"Source (IP)"}
                                <TypedInput<IpAddr>
                                    storage={self.im_ops_source_local.clone()}
                                    on_send_valid={ctx.link().callback(Msg::SetImOpsSource)}
                                    />
                            </label>
                        </div>


                        <div>
                            <label>{"Center X"}
                                <TypedInput<u32>
                                    storage={self.im_ops_center_x.clone()}
                                    on_send_valid={ctx.link().callback(Msg::SetImOpsCenterX)}
                                    />
                            </label>
                        </div>

                        <div>
                            <label>{"Center Y"}
                                <TypedInput<u32>
                                    storage={self.im_ops_center_y.clone()}
                                    on_send_valid={ctx.link().callback(Msg::SetImOpsCenterY)}
                                    />
                            </label>
                        </div>

                        <div>
                            <label>{"Threshold"}
                                <TypedInput<u8>
                                    storage={self.im_ops_threshold.clone()}
                                    on_send_valid={ctx.link().callback(Msg::SetImOpsTheshold)}
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

    fn point_detection_ui(&self, ctx: &Context<Self>) -> Html {
        if let Some(ref shared) = self.server_state {
            if shared.has_image_tracker_compiled {
                let cfg_clone = shared.im_pt_detect_cfg.clone();
                return html! {
                    <div class="wrap-collapsible">
                        <CheckboxLabel label="Object Detection" initially_checked=true />
                        <div>

                            <div>
                                <Toggle
                                    label={"Enable object detection"}
                                    value={shared.is_doing_object_detection}
                                    ontoggle={ctx.link().callback(|checked| {Msg::ToggleObjDetection(checked)})}
                                    />
                            </div>

                            <div>
                                <RecordingPathWidget
                                    label={"Record CSV file"}
                                    value={shared.is_saving_im_pt_detect_csv.clone()}
                                    ontoggle={ctx.link().callback(|checked| {Msg::ToggleObjDetectionSaveCsv(checked)})}
                                    />
                            </div>

                            <div>
                                <h5>{"CSV Max Rate"}</h5>
                                <EnumToggle<RecordingFrameRate>
                                    value={self.csv_recording_rate.clone()}
                                    onsignal={ctx.link().callback(Msg::ToggleCsvRecordingRate)}
                                />
                            </div>

                            <div>
                                <Toggle
                                    label={"Update background model"}
                                    value={shared.im_pt_detect_cfg.do_update_background_model}
                                    ontoggle={ctx.link().callback(move |checked| {
                                        let mut cfg_clone2 = cfg_clone.clone();
                                        cfg_clone2.do_update_background_model = checked;
                                        let cfg_str = serde_yaml::to_string(&cfg_clone2).unwrap();
                                        Msg::SetObjDetectionConfig(cfg_str)
                                    })}
                                    />
                            </div>
                            <div>
                                <h5>{"Detailed configuration"}</h5>
                                <ConfigField<ImPtDetectCfg>
                                    server_version={Some(shared.im_pt_detect_cfg.clone())}
                                    rows={16}
                                    onsignal={ctx.link().callback(|cfg| {Msg::SetObjDetectionConfig(cfg)})}
                                    />
                                <div class="reset-background-btn">
                                    <Button title={"Take Current Image As Background"} onsignal={ctx.link().callback(|_| Msg::TakeCurrentImageAsBackground)}/>
                                    <Button title={"Set background to mid-gray"} onsignal={ctx.link().callback(|_| Msg::ClearBackground(127.0))}/>
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

    fn checkerboard_calibration_ui(&self, ctx: &Context<Self>) -> Html {
        if let Some(ref shared) = self.server_state {
            if shared.has_checkercal_compiled {
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

                return html! {
                    <div class="wrap-collapsible">
                        <CheckboxLabel label={"Checkerboard Calibration"} />
                        <div>
                            <p>{"This enables estimation of lens distortion parameters."}</p>
                        </div>
                        <div>

                            <Toggle
                                label={"Enable checkerboard calibration"}
                                value={shared.checkerboard_data.enabled}
                                ontoggle={ctx.link().callback(|checked| {Msg::ToggleCheckerboardDetection(checked)})}
                                />

                            <Toggle
                                label={"Save debug information"}
                                value={shared.checkerboard_save_debug.is_some()}
                                ontoggle={ctx.link().callback(|checked| {Msg::ToggleCheckerboardDebug(checked)})}
                                />

                            <div>{checkerboard_debug}</div>

                            <h2>{"Input: Checkerboard Size"}</h2>
                            <p>{"Enter the size of your checkerboard in number of inner corners (e.g. 7 x 7 for a standard chessboard)."}</p>
                            <label>{"width"}
                                <TypedInput<u32>
                                    storage={self.checkerboard_width.clone()}
                                    on_send_valid={ctx.link().callback(|v| Msg::SetCheckerboardWidth(v))}
                                    />
                            </label>
                            <label>{"height"}
                                <TypedInput<u32>
                                    storage={self.checkerboard_height.clone()}
                                    on_send_valid={ctx.link().callback(|v| Msg::SetCheckerboardHeight(v))}
                                    />
                            </label>

                            <h2>{"Action: Perform Calibration"}</h2>

                            <div>
                                {num_checkerboards_collected}
                            </div>

                            <Button
                                title={"Clear Checkerboards"}
                                onsignal={ctx.link().callback(move |_| Msg::ClearCheckerboards)}
                                />

                            <Button
                                title={"Perform and Save Calibration"}
                                disabled={disabled}
                                is_active={is_active}
                                onsignal={ctx.link().callback(move |_| Msg::PerformCheckerboardCalibration)}
                                />

                        </div>
                    </div>
                };
            }
        }
        return html! {
            <div></div>
        };
    }

    fn view_kalman_tracking(&self, ctx: &Context<Self>) -> Html {
        if let Some(ref shared) = self.server_state {
            if shared.has_flydratrax_compiled {
                return html! {
                    <div class="wrap-collapsible">
                        <CheckboxLabel label="Kalman tracking" initially_checked=false />
                        <div>
                            <div>
                                <h5>{"Kalman tracking configuration"}</h5>
                                <ConfigField<KalmanTrackingConfig>
                                    server_version={Some(shared.kalman_tracking_config.clone())}
                                    rows=5
                                    onsignal={ctx.link().callback(|cfg| {Msg::CamArgSetKalmanTrackingConfig(cfg)})}
                                    />
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

    fn view_led_triggering(&self, ctx: &Context<Self>) -> Html {
        if let Some(ref shared) = self.server_state {
            if shared.has_flydratrax_compiled {
                return html! {
                    <div class="wrap-collapsible">
                        <CheckboxLabel label="Online LED triggering" initially_checked=false />

                            <div>
                                <h5>{"Led program configuration"}</h5>
                                <ConfigField<LedProgramConfig>
                                    server_version={Some(shared.led_program_config.clone())}
                                    rows=7
                                    onsignal={ctx.link().callback(|cfg| {Msg::CamArgSetLedProgramConfig(cfg)})}
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

    fn view_gain(&self, ctx: &Context<Self>) -> Html {
        if let Some(ref shared) = self.server_state {
            if let Some(gain_auto) = shared.gain_auto {
                return html! {
                    <div class={classes!("gain-main","cam-range-main")}>
                        <h3>{ "Gain" }</h3>
                        <div class="cam-range-inner">
                            <AutoModeSelect mode={gain_auto} onsignal={ctx.link().callback(|g| {Msg::SetGainAuto(g)})} />
                            <RangedValue
                                unit={shared.gain.unit.clone()}
                                min={shared.gain.min as f32}
                                max={shared.gain.max as f32}
                                current={shared.gain.current as f32}
                                current_value_label={LAST_DETECTED_VALUE_LABEL}
                                placeholder={shared.gain.name.clone()}
                                onsignal={ctx.link().callback(|v| {Msg::SetGainValue(v as f64)})}
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

    fn view_exposure(&self, ctx: &Context<Self>) -> Html {
        if let Some(ref shared) = self.server_state {
            if let Some(exposure_auto) = shared.exposure_auto {
                return html! {
                    <div class={classes!("exposure-main","cam-range-main")}>
                        <h3>{ "Exposure Time" }</h3>
                        <div class="cam-range-inner">
                            <AutoModeSelect mode={exposure_auto} onsignal={ctx.link().callback(|g| {Msg::SetExposureAuto(g)}) }/>
                            <RangedValue
                                unit={shared.exposure_time.unit.clone()}
                                min={shared.exposure_time.min as f32}
                                max={shared.exposure_time.max as f32}
                                current={shared.exposure_time.current as f32}
                                current_value_label={LAST_DETECTED_VALUE_LABEL}
                                placeholder={shared.exposure_time.name.clone()}
                                onsignal={ctx.link().callback(|v| {Msg::SetExposureValue(v as f64)})}
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

    fn view_frame_rate_limit(&self, ctx: &Context<Self>) -> Html {
        if let Some(ref shared) = self.server_state {
            if let Some(ref frl) = shared.frame_rate_limit {
                html! {
                    <div class={classes!("frame-rate-main","cam-range-main")}>
                        <h3>{ "Maximum Frame Rate" }</h3>
                            <div class="auto-mode-container">
                                <div class="auto-mode-label">
                                    {"Limit Frame Rate: "}
                                </div>
                                <div class="auto-mode-buttons">
                                    <EnumToggle<bool>
                                        value={shared.frame_rate_limit_enabled}
                                        onsignal={ctx.link().callback(Msg::SetFrameRateLimitEnabled)}
                                    />
                                </div>
                            </div>
                        <div class="cam-range-inner">
                            <RangedValue
                                unit={frl.unit.clone()}
                                min={frl.min as f32}
                                max={frl.max as f32}
                                current={frl.current as f32}
                                current_value_label={LAST_DETECTED_VALUE_LABEL}
                                placeholder={frl.name.clone()}
                                onsignal={ctx.link().callback(|v| {Msg::SetFrameRateLimit(v as f64)})}
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

fn to_rate(rate_enum: &RecordingFrameRate) -> Option<f32> {
    match rate_enum {
        RecordingFrameRate::Fps1 => Some(1.0),
        RecordingFrameRate::Fps2 => Some(2.0),
        RecordingFrameRate::Fps5 => Some(5.0),
        RecordingFrameRate::Fps10 => Some(10.0),
        RecordingFrameRate::Fps20 => Some(20.0),
        RecordingFrameRate::Fps25 => Some(25.0),
        RecordingFrameRate::Fps30 => Some(30.0),
        RecordingFrameRate::Fps40 => Some(40.0),
        RecordingFrameRate::Fps50 => Some(50.0),
        RecordingFrameRate::Fps60 => Some(60.0),
        RecordingFrameRate::Fps100 => Some(100.0),
        RecordingFrameRate::Unlimited => None,
    }
}

// impl Model {
//     fn send_message(&mut self, args: &CallbackType) -> Option<yew::services::fetch::FetchTask> {
//         let post_request = Request::post("callback")
//             .header("Content-Type", "application/json;charset=UTF-8")
//             .body(Json(&args))
//             .expect("Failed to build request.");

//         let callback =
//             self.link
//                 .callback(move |response: Response<Json<Result<(), anyhow::Error>>>| {
//                     if let (meta, Json(Ok(_body))) = response.into_parts() {
//                         if meta.status.is_success() {
//                             return Msg::Ignore;
//                         }
//                     }
//                     log::error!("failed sending message");
//                     Msg::Ignore
//                 });
//         let options = FetchOptions {
//             credentials: Some(Credentials::SameOrigin),
//             ..Default::default()
//         };

//         match FetchService::fetch_with_options(post_request, options, callback) {
//             Ok(task) => Some(task),
//             Err(err) => {
//                 log::error!("sending message failed with error: {}", err);
//                 None
//             }
//         }
//     }
// }

// -----------------------------------------------------------------------------

async fn post_message(msg: &CallbackType) -> Result<(), FetchError> {
    use web_sys::{Request, RequestInit, Response};
    let mut opts = RequestInit::new();
    opts.method("POST");
    // opts.mode(web_sys::RequestMode::Cors);
    // opts.headers("Content-Type", "application/json;charset=UTF-8")
    // set SameOrigin
    let buf = serde_json::to_string(&msg).unwrap_throw();
    opts.body(Some(&JsValue::from_str(&buf)));

    let url = "callback";
    let request = Request::new_with_str_and_init(url, &opts)?;

    let window = gloo_utils::window();
    let resp_value = JsFuture::from(window.fetch_with_request(&request)).await?;
    let resp: Response = resp_value.dyn_into().unwrap_throw();

    let text = JsFuture::from(resp.text()?).await?;
    let _text_string = text.as_string().unwrap_throw();
    Ok(())
}

// -----------------------------------------------------------------------------

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
        ci2_remote_control::MkvCodec::Uncompressed => BitrateSelection::default().to_u32(),
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

#[derive(Clone, PartialEq, Debug)]
enum CodecSelection {
    H264,
    VP8,
    VP9,
    Uncompressed,
}

impl CodecSelection {
    fn get_codec(&self, old: &ci2_remote_control::MkvCodec) -> ci2_remote_control::MkvCodec {
        use crate::CodecSelection::*;
        let bitrate = match old {
            ci2_remote_control::MkvCodec::Uncompressed => BitrateSelection::default().to_u32(),
            ci2_remote_control::MkvCodec::VP8(c) => c.bitrate,
            ci2_remote_control::MkvCodec::VP9(c) => c.bitrate,
            ci2_remote_control::MkvCodec::H264(c) => c.bitrate,
        };
        match self {
            Uncompressed => ci2_remote_control::MkvCodec::Uncompressed,
            VP8 => ci2_remote_control::MkvCodec::VP8(ci2_remote_control::VP8Options { bitrate }),
            VP9 => ci2_remote_control::MkvCodec::VP9(ci2_remote_control::VP9Options { bitrate }),
            H264 => ci2_remote_control::MkvCodec::H264(ci2_remote_control::MkvH264Options {
                bitrate,
                cuda_device: 0,
            }),
        }
    }
}

impl std::fmt::Display for CodecSelection {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let x = match self {
            CodecSelection::Uncompressed => "RAW",
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
            CodecSelection::Uncompressed,
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
        if !self.cuda_devices.is_empty() && self.is_nvenc_functioning {
            vec![
                CodecSelection::Uncompressed,
                CodecSelection::VP8,
                CodecSelection::VP9,
                CodecSelection::H264,
            ]
        } else {
            vec![
                CodecSelection::Uncompressed,
                CodecSelection::VP8,
                CodecSelection::VP9,
            ]
        }
    }
}

fn match_avail(
    avail: &[CodecSelection],
    selected: &ci2_remote_control::MkvCodec,
) -> Option<CodecSelection> {
    let result = match selected {
        ci2_remote_control::MkvCodec::Uncompressed => CodecSelection::Uncompressed,
        ci2_remote_control::MkvCodec::VP8(_) => CodecSelection::VP8,
        ci2_remote_control::MkvCodec::VP9(_) => CodecSelection::VP9,
        ci2_remote_control::MkvCodec::H264(_) => CodecSelection::H264,
    };
    if avail.contains(&result) {
        Some(result)
    } else {
        None
    }
}

#[wasm_bindgen(start)]
pub fn run_app() {
    wasm_logger::init(wasm_logger::Config::default());
    yew::start_app::<Model>();
}
