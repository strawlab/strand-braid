use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};

use rust_cam_bui_types::RecordingPath;
use serde::{Deserialize, Serialize};

use http_video_streaming_types::FirehoseCallbackInner;
use http_video_streaming_types::{CircleParams, Shape};

use ci2_remote_control::{MkvRecordingConfig, RecordingFrameRate, TagFamily};
use flydra_feature_detector_types::ImPtDetectCfg;

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RangedValue {
    pub name: String,
    pub unit: String,
    pub current: f64,
    pub min: f64,
    pub max: f64,
}

use led_box_comms::DeviceState;

pub use led_box_comms::ToDevice as ToLedBoxDevice;

pub const STRAND_CAM_EVENTS_URL_PATH: &str = "/strand-cam-events";
pub const STRAND_CAM_EVENT_NAME: &str = "strand-cam";

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoreType {
    pub is_braid: bool,
    /// is saving MKV file
    pub is_recording_mkv: Option<RecordingPath>,
    /// is saving FMF file
    pub is_recording_fmf: Option<RecordingPath>,
    /// is saving UFMF file
    pub is_recording_ufmf: Option<RecordingPath>,
    pub format_str_mkv: String,
    pub format_str: String,
    pub format_str_ufmf: String,
    pub camera_name: String,
    pub recording_filename: Option<String>,
    pub recording_framerate: RecordingFrameRate,
    pub mkv_recording_config: MkvRecordingConfig,
    pub gain_auto: Option<ci2_types::AutoMode>,
    pub gain: RangedValue,
    pub exposure_auto: Option<ci2_types::AutoMode>,
    pub exposure_time: RangedValue,
    pub frame_rate_limit_enabled: bool,
    /// None when frame_rate_limit is not supported
    pub frame_rate_limit: Option<RangedValue>,
    pub trigger_mode: ci2_types::TriggerMode,
    pub trigger_selector: ci2_types::TriggerSelector,
    pub image_width: u32,
    pub image_height: u32,
    /// Whether object detection with image-tracker crate is compiled.
    // We could have made this a cargo feature, but this
    // adds complication to the builds. Here, the cost
    // is some extra unused code paths in the compiled
    // code, as well as larger serialized objects.
    pub has_image_tracker_compiled: bool,
    // used only with image-tracker crate
    /// Whether object detection is currently used.
    pub is_doing_object_detection: bool,
    pub measured_fps: f32,
    /// is saving object detection CSV file
    pub is_saving_im_pt_detect_csv: Option<RecordingPath>,
    // used only with image-tracker crate
    pub im_pt_detect_cfg: ImPtDetectCfg,
    /// Whether flydratrax (2D kalman tracking and LED triggering) is compiled.
    pub has_flydratrax_compiled: bool,
    pub kalman_tracking_config: KalmanTrackingConfig,
    pub led_program_config: LedProgramConfig,
    pub led_box_device_lost: bool,
    pub led_box_device_state: Option<DeviceState>,
    pub led_box_device_path: Option<String>,
    /// Whether checkerboard calibration is compiled.
    pub has_checkercal_compiled: bool,
    pub checkerboard_data: CheckerboardCalState,
    /// Path where debug data is being saved.
    pub checkerboard_save_debug: Option<String>,
    pub post_trigger_buffer_size: usize,
    pub cuda_devices: Vec<String>,
    /// This is None if no apriltag support is compiled in. Otherwise Some(_).
    pub apriltag_state: Option<ApriltagState>,
    pub im_ops_state: ImOpsState,
    pub format_str_apriltag_csv: String,
    pub had_frame_processing_error: bool,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ApriltagState {
    pub do_detection: bool,
    pub april_family: TagFamily,
    pub is_recording_csv: Option<RecordingPath>,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImOpsState {
    pub do_detection: bool,
    pub destination: SocketAddr,
    /// The IP address of the socket interface from which the data is sent.
    pub source: IpAddr,
    pub center_x: u32,
    pub center_y: u32,
    pub threshold: u8,
}

impl Default for ImOpsState {
    fn default() -> Self {
        Self {
            do_detection: false,
            destination: SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 8080)),
            source: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            center_x: 0,
            center_y: 0,
            threshold: 0,
        }
    }
}

pub const APRILTAG_CSV_TEMPLATE_DEFAULT: &str = "apriltags%Y%m%d_%H%M%S.%f_{CAMNAME}.csv.gz";

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum LEDTriggerMode {
    Off, // could probably be better named "Unchanging" or "Constant"
    PositionTriggered,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KalmanTrackingConfig {
    pub enabled: bool,
    pub arena_diameter_meters: f32,
    pub min_central_moment: f32,
}

impl std::default::Default for KalmanTrackingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            arena_diameter_meters: 0.2,
            min_central_moment: 0.0,
        }
    }
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LedProgramConfig {
    pub led_trigger_mode: LEDTriggerMode,
    pub led_on_shape_pixels: Shape,
    pub led_channel_num: u8,
    pub led_second_stage_radius: u16,
    pub led_hysteresis_pixels: f32,
}

impl std::default::Default for LedProgramConfig {
    fn default() -> Self {
        Self {
            led_trigger_mode: LEDTriggerMode::Off,
            led_channel_num: 1,
            led_on_shape_pixels: Shape::Circle(CircleParams {
                center_x: 640,
                center_y: 512,
                radius: 50,
            }),
            led_second_stage_radius: 50,
            led_hysteresis_pixels: 3.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckerboardCalState {
    pub enabled: bool,
    pub num_checkerboards_collected: u32,
    pub width: u32,
    pub height: u32,
}

impl CheckerboardCalState {
    pub fn new() -> Self {
        Self {
            enabled: false,
            num_checkerboards_collected: 0,
            width: 8,
            height: 6,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub enum CallbackType {
    ToCamera(ci2_remote_control::CamArg),
    FirehoseNotify(FirehoseCallbackInner),
    // used only with image-tracker crate
    TakeCurrentImageAsBackground,
    // used only with image-tracker crate
    ClearBackground(f32),
    ToLedBox(ToLedBoxDevice),
}
