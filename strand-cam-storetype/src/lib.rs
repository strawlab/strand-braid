//! Type definitions for [Strand Camera's](https://strawlab.org/strand-cam)
//! state management and browser UI communication.
//!
//! This crate provides core data structures that represent the complete state
//! of a Strand Camera instance, including camera settings, recording status,
//! feature detection configuration, and various processing modes. These types
//! are primarily used for:
//!
//! - Serializing camera state for the web-based user interface
//! - Managing recording sessions across different file formats
//! - Configuring real-time image processing features
//! - Coordinating LED control and Kalman tracking functionality
//!
//! ## Key Components
//!
//! - [StoreType]: The main state container for all camera configuration and
//!       status
//! - Recording management for MP4, FMF, and UFMF formats
//! - Feature detection and tracking configuration
//! - LED control and triggering systems
//! - AprilTag detection and checkerboard calibration support
//!
//! ## Communication
//!
//! The types in this crate support Server-Sent Events for real-time browser
//! updates and remote camera control via HTTP APIs.

#![warn(missing_docs)]
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};

use serde::{Deserialize, Serialize};
use strand_cam_bui_types::RecordingPath;

use strand_http_video_streaming_types::{CircleParams, Shape};

use flydra_feature_detector_types::ImPtDetectCfg;
use strand_cam_remote_control::{BitrateSelection, CodecSelection, RecordingFrameRate, TagFamily};

/// A numeric value with associated metadata for user interface controls.
///
/// This structure represents camera parameters that have a current value within
/// a defined range, along with human-readable name and units. It's commonly used
/// for camera settings like gain, exposure time, and frame rate that can be
/// adjusted through sliders or input controls in the web interface.
///
/// # Examples
///
/// ```rust
/// use strand_cam_storetype::RangedValue;
///
/// let gain = RangedValue {
///     name: "Gain".to_string(),
///     unit: "dB".to_string(),
///     current: 12.5,
///     min: 0.0,
///     max: 30.0,
/// };
/// ```
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RangedValue {
    /// Human-readable name of the parameter (e.g., "Gain", "Exposure Time").
    pub name: String,
    /// Units of measurement (e.g., "dB", "ms", "fps").
    pub unit: String,
    /// Current value of the parameter.
    pub current: f64,
    /// Minimum allowed value.
    pub min: f64,
    /// Maximum allowed value.
    pub max: f64,
}

use strand_led_box_comms::DeviceState;

/// Commands that can be sent to LED control devices.
///
/// Re-exported from `strand_led_box_comms` for convenience.
pub use strand_led_box_comms::ToDevice as ToLedBoxDevice;

// Note: this does not start with a slash because we do not want an absolute
// root path in case we are in a case where we are proxied by braid. I.e. it
// should work at `http://braid/cam-proxy/cam-name/strand-cam-events` as well as
// `http://strand-cam/strand-cam-events`.

/// URL path for Strand Camera's Server-Sent Events endpoint.
///
/// This path is used to establish SSE connections for real-time updates
/// from the camera to web browsers. The path is relative to support
/// both direct connections and proxy scenarios through Braid.
pub const STRAND_CAM_EVENTS_URL_PATH: &str = "strand-cam-events";

/// Event name for Strand Camera SSE messages.
///
/// Used to identify camera state update events in the Server-Sent Events stream.
pub const STRAND_CAM_EVENT_NAME: &str = "strand-cam";

/// Event name for connection key SSE messages.
///
/// Used for session management and authentication in the browser interface.
pub const CONN_KEY_EVENT_NAME: &str = "connection-key";

/// Complete state representation of a Strand Camera instance.
///
/// This is the primary data structure that encapsulates all configuration,
/// status, and capability information for a running camera. It includes
/// everything from basic camera settings to complex processing features
/// like object detection, Kalman tracking, and AprilTag recognition.
///
/// The structure is designed to be serialized and sent to web browsers
/// for real-time monitoring and control of the camera system.
///
/// # Feature Compilation
///
/// Many fields are conditional based on compile-time features:
/// - `has_image_tracker_compiled`: Object detection capabilities
/// - `has_flydratrax_compiled`: Kalman tracking and LED control
/// - `has_checkercal_compiled`: Checkerboard calibration
/// - `apriltag_state`: AprilTag detection (None if not compiled)
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoreType {
    /// Whether we are running inside Braid.
    pub is_braid: bool,
    /// What version of ffmpeg is available to Strand Camera
    pub ffmpeg_version: Option<String>,
    /// Whether we have Nvidia NvEnc encoder available.
    pub is_nvenc_functioning: bool,
    /// Whether we have VideoToolbox
    pub is_videotoolbox_functioning: bool,
    /// is saving MP4 file
    pub is_recording_mp4: Option<RecordingPath>,
    /// is saving FMF file
    pub is_recording_fmf: Option<RecordingPath>,
    /// is saving UFMF file
    pub is_recording_ufmf: Option<RecordingPath>,
    /// Format string template for MP4 filenames.
    pub format_str_mp4: String,
    /// Format string template for FMF filenames.
    pub format_str: String,
    /// Format string template for UFMF filenames.
    pub format_str_ufmf: String,
    /// Name of the camera device.
    pub camera_name: String,
    /// Current gamma correction value applied to the camera.
    pub camera_gamma: Option<f32>,
    /// Base filename for recordings (without extension).
    pub recording_filename: Option<String>,
    /// Maximum frame rate for MP4 recording.
    pub mp4_max_framerate: RecordingFrameRate,
    // pub mp4_recording_config: Mp4RecordingConfig,
    /// Bitrate selection for MP4 encoding.
    pub mp4_bitrate: BitrateSelection,
    /// Video codec selection for MP4 encoding.
    pub mp4_codec: CodecSelection,
    /// CUDA device number (only used if using nvidia encoder)
    pub mp4_cuda_device: String,
    /// Automatic gain control mode.
    pub gain_auto: Option<strand_cam_types::AutoMode>,
    /// Camera gain settings and range.
    pub gain: RangedValue,
    /// Automatic exposure control mode.
    pub exposure_auto: Option<strand_cam_types::AutoMode>,
    /// Camera exposure time settings and range.
    pub exposure_time: RangedValue,
    /// Whether software frame rate limiting is enabled.
    pub frame_rate_limit_enabled: bool,
    /// None when frame_rate_limit is not supported
    pub frame_rate_limit: Option<RangedValue>,
    /// Camera trigger mode (internal, external, etc.).
    pub trigger_mode: strand_cam_types::TriggerMode,
    /// Which trigger input to use.
    pub trigger_selector: strand_cam_types::TriggerSelector,
    /// Width of captured images in pixels.
    pub image_width: u32,
    /// Height of captured images in pixels.
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
    /// Current measured frame rate in frames per second.
    pub measured_fps: f32,
    /// is saving object detection CSV file
    pub is_saving_im_pt_detect_csv: Option<RecordingPath>,
    // used only with image-tracker crate
    /// Configuration for image point detection algorithms.
    pub im_pt_detect_cfg: ImPtDetectCfg,
    /// Whether flydratrax (2D kalman tracking and LED triggering) is compiled.
    pub has_flydratrax_compiled: bool,
    /// Configuration for Kalman tracking of detected objects.
    pub kalman_tracking_config: KalmanTrackingConfig,
    /// Configuration for LED control and triggering.
    pub led_program_config: LedProgramConfig,
    /// Whether connection to LED control device has been lost.
    pub led_box_device_lost: bool,
    /// Current state of the LED control device.
    pub led_box_device_state: Option<DeviceState>,
    /// Path to the LED control device.
    pub led_box_device_path: Option<String>,
    /// Whether checkerboard calibration is compiled.
    pub has_checkercal_compiled: bool,
    /// Current state of checkerboard calibration process.
    pub checkerboard_data: CheckerboardCalState,
    /// Path where debug data is being saved.
    pub checkerboard_save_debug: Option<String>,
    /// Number of frames to buffer for post-trigger recording.
    pub post_trigger_buffer_size: usize,
    /// List of available CUDA devices for hardware acceleration.
    pub cuda_devices: Vec<String>,
    /// This is None if no apriltag support is compiled in. Otherwise Some(_).
    pub apriltag_state: Option<ApriltagState>,
    /// State of image operations processing.
    pub im_ops_state: ImOpsState,
    /// Format string template for AprilTag CSV filenames.
    pub format_str_apriltag_csv: String,
    /// Whether there was an error during frame processing.
    pub had_frame_processing_error: bool,
    /// The camera calibration (does not contain potential information about water)
    pub camera_calibration: Option<braid_mvg::Camera<f64>>,
}

/// State and configuration of AprilTag detection.
///
/// AprilTags are fiducial markers that can be detected in images for
/// tracking and localization purposes. This structure controls whether
/// detection is enabled and manages recording of detection results.
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ApriltagState {
    /// Whether AprilTag detection is currently enabled.
    pub do_detection: bool,
    /// Which AprilTag family to detect (e.g., tag36h11, tag25h9).
    pub april_family: TagFamily,
    /// Path where AprilTag detection results are being saved to CSV.
    pub is_recording_csv: Option<RecordingPath>,
}

/// Configuration for image operations and UDP data streaming.
///
/// This structure configures real-time image processing that detects
/// features and streams results over UDP to external applications.
/// It's used for low-latency tracking applications where processed
/// data needs to be sent immediately to other systems.
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImOpsState {
    /// Whether image operations detection is enabled.
    pub do_detection: bool,
    /// UDP socket address where detection results are sent.
    pub destination: SocketAddr,
    /// The IP address of the socket interface from which the data is sent.
    pub source: IpAddr,
    /// X coordinate of the region center for detection.
    pub center_x: u32,
    /// Y coordinate of the region center for detection.
    pub center_y: u32,
    /// Intensity threshold for feature detection.
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

/// Default filename template for AprilTag CSV recordings.
///
/// This template includes timestamp formatting and camera name substitution:
/// - `%Y%m%d_%H%M%S.%f`: Date and time with microseconds
/// - `{CAMNAME}`: Replaced with actual camera name
/// - `.csv.gz`: Compressed CSV format
pub const APRILTAG_CSV_TEMPLATE_DEFAULT: &str = "apriltags%Y%m%d_%H%M%S.%f_{CAMNAME}.csv.gz";

/// LED triggering modes for controlling external lighting.
///
/// This enum determines how LED lighting systems are controlled
/// in response to tracked object positions.
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum LEDTriggerMode {
    /// LEDs remain in a constant state (off or on).
    Off, // could probably be better named "Unchanging" or "Constant"
    /// LEDs are triggered based on tracked object positions.
    PositionTriggered,
}

/// Configuration for Kalman filter-based object tracking.
///
/// This structure configures 2D tracking of objects detected in the camera
/// image using Kalman filtering. It's designed for tracking small objects
/// like insects or particles within a defined arena.
///
/// # Usage
///
/// Typically used in behavioral experiments where objects need to be
/// tracked continuously for triggering responses or data collection.
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KalmanTrackingConfig {
    /// Whether Kalman tracking is currently enabled.
    pub enabled: bool,
    /// Diameter of the tracking arena in meters.
    ///
    /// Used to scale tracking parameters and validate object positions.
    pub arena_diameter_meters: f32,
    /// Minimum central moment required for object detection.
    ///
    /// Objects with central moments below this threshold are ignored.
    /// This helps filter out noise and very small detections.
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

/// Configuration for LED control and position-based triggering.
///
/// This structure defines how external LED lighting responds to
/// tracked object positions. It supports triggering LEDs when
/// objects enter or exit defined regions of interest.
///
/// # Two-Stage Triggering
///
/// The system supports a two-stage approach:
/// 1. Initial trigger when object enters `led_on_shape_pixels`
/// 2. Secondary trigger based on `led_second_stage_radius`
/// 3. Hysteresis prevents rapid on/off switching
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LedProgramConfig {
    /// How LEDs should respond to object positions.
    pub led_trigger_mode: LEDTriggerMode,
    /// Geometric shape that defines the LED trigger region.
    pub led_on_shape_pixels: Shape,
    /// Which LED channel to control (for multi-channel LED systems).
    pub led_channel_num: u8,
    /// Radius for second-stage LED triggering logic.
    pub led_second_stage_radius: u16,
    /// Hysteresis distance in pixels to prevent rapid switching.
    ///
    /// Objects must move this distance before triggering state changes,
    /// preventing flickering when objects are near trigger boundaries.
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

/// State of checkerboard camera calibration process.
///
/// Checkerboard calibration is used to determine camera intrinsic parameters
/// (focal length, principal point, distortion) by detecting checkerboard
/// patterns at different positions and orientations.
///
/// # Calibration Process
///
/// 1. Enable checkerboard detection
/// 2. Present checkerboard patterns to the camera
/// 3. System automatically detects and collects pattern data
/// 4. Once enough patterns are collected, calibration can be computed
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckerboardCalState {
    /// Whether checkerboard detection and collection is enabled.
    pub enabled: bool,
    /// Number of valid checkerboard patterns collected so far.
    pub num_checkerboards_collected: u32,
    /// Number of internal corners along the width of the checkerboard.
    pub width: u32,
    /// Number of internal corners along the height of the checkerboard.
    pub height: u32,
}

impl Default for CheckerboardCalState {
    fn default() -> Self {
        Self {
            enabled: false,
            num_checkerboards_collected: 0,
            width: 8,
            height: 6,
        }
    }
}

/// Commands and callbacks that can be sent to control camera behavior.
///
/// This enum represents different types of control messages that can be
/// sent to modify camera settings, trigger actions, or control peripheral
/// devices like LED systems.
///
/// # Usage
///
/// These callbacks are typically sent from the web interface or external
/// control systems to modify camera behavior in real-time.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub enum CallbackType {
    /// Camera control commands (exposure, gain, triggering, etc.).
    ToCamera(strand_cam_remote_control::CamArg),
    /// Notification for firehose data streaming connections.
    FirehoseNotify(strand_bui_backend_session_types::ConnectionKey),
    // used only with image-tracker crate
    /// Capture the current image as a background reference.
    ///
    /// Used for background subtraction in object detection algorithms.
    TakeCurrentImageAsBackground,
    // used only with image-tracker crate
    /// Clear the background reference with the specified alpha value.
    ///
    /// The alpha parameter controls the mixing ratio for background updates.
    ClearBackground(f32),
    /// Commands to send to LED control devices.
    ToLedBox(ToLedBoxDevice),
}
