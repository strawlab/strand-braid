use rust_cam_bui_types::RecordingPath;
use serde::{Deserialize, Serialize};

use http_video_streaming_types::FirehoseCallbackInner;
#[cfg(feature = "flydratrax")]
use http_video_streaming_types::{CircleParams, Shape};

use ci2_remote_control::{MkvRecordingConfig, RecordingFrameRate, TagFamily};
use image_tracker_types::ImPtDetectCfg;

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct RangedValue {
    pub name: String,
    pub unit: String,
    pub current: f64,
    pub min: f64,
    pub max: f64,
}

#[cfg(feature = "with_camtrig")]
use camtrig_comms::DeviceState;

#[cfg(not(feature = "with_camtrig"))]
type DeviceState = std::marker::PhantomData<u8>;

#[cfg(feature = "with_camtrig")]
pub use camtrig_comms::ToDevice as ToCamtrigDevice;

#[cfg(not(feature = "with_camtrig"))]
pub type ToCamtrigDevice = std::marker::PhantomData<u8>;

pub const STRAND_CAM_EVENTS_URL_PATH: &'static str = "/strand-cam-events";
pub const STRAND_CAM_EVENT_NAME: &'static str = "strand-cam";

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct StoreType {
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
    #[cfg(feature = "flydratrax")]
    pub kalman_tracking_config: KalmanTrackingConfig,
    #[cfg(feature = "flydratrax")]
    pub led_program_config: LedProgramConfig,
    #[cfg(feature = "with_camtrig")]
    pub camtrig_device_lost: bool,
    pub camtrig_device_state: Option<DeviceState>,
    pub camtrig_device_path: Option<String>,
    #[cfg(feature = "checkercal")]
    pub checkerboard_data: CheckerboardCalState,
    pub post_trigger_buffer_size: usize,
    pub cuda_devices: Vec<String>,
    /// This is None if no apriltag support is compiled in. Otherwise Some(_).
    pub apriltag_state: Option<ApriltagState>,
    pub format_str_apriltag_csv: String,
    pub had_frame_processing_error: bool,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct ApriltagState {
    pub do_detection: bool,
    pub april_family: TagFamily,
    pub is_recording_csv: Option<RecordingPath>,
}

impl Default for ApriltagState {
    fn default() -> Self {
        Self {
            do_detection: false,
            april_family: TagFamily::default(),
            is_recording_csv: None,
        }
    }
}

pub const APRILTAG_CSV_TEMPLATE_DEFAULT: &'static str = "apriltags%Y%m%d_%H%M%S.csv.gz";

#[cfg(feature = "flydratrax")]
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub enum LEDTriggerMode {
    Off, // could probably be better named "Unchanging" or "Constant"
    PositionTriggered,
}

#[cfg(feature = "flydratrax")]
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct KalmanTrackingConfig {
    pub enabled: bool,
    pub arena_diameter_meters: f32,
    pub min_central_moment: f32,
}

#[cfg(feature = "flydratrax")]
impl std::default::Default for KalmanTrackingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            arena_diameter_meters: 0.2,
            min_central_moment: 0.0,
        }
    }
}

#[cfg(feature = "flydratrax")]
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct LedProgramConfig {
    pub led_trigger_mode: LEDTriggerMode,
    pub led_on_shape_pixels: Shape,
    pub led_channel_num: u8,
    pub led_second_stage_radius: u16,
    pub led_hysteresis_pixels: f32,
}

#[cfg(feature = "flydratrax")]
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

#[cfg(feature = "checkercal")]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckerboardCalState {
    pub enabled: bool,
    pub num_checkerboards_collected: u32,
    pub width: u32,
    pub height: u32,
}

#[cfg(feature = "checkercal")]
impl CheckerboardCalState {
    pub fn new() -> Self {
        Self {
            enabled: false,
            num_checkerboards_collected: 0,
            width: 7,
            height: 7,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum CallbackType {
    ToCamera(ci2_remote_control::CamArg),
    FirehoseNotify(FirehoseCallbackInner),
    // used only with image-tracker crate
    TakeCurrentImageAsBackground,
    // used only with image-tracker crate
    ClearBackground(f32),
    ToCamtrig(ToCamtrigDevice),
}
