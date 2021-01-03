#[macro_use]
extern crate bitflags;
#[macro_use]
extern crate static_assertions;

use std::convert::TryFrom;

use ordered_float::NotNan;
use rust_cam_bui_types::{ClockModel, RecordingPath};

use serde::{Deserialize, Deserializer, Serialize};

use bui_backend_types::AccessToken;
use withkey::WithKey;

pub const DEFAULT_MODEL_SERVER_ADDR: &str = "0.0.0.0:8397";

// These are the filenames saved during recording. --------------------
//
// Any changes to these names, including additions and removes, should update
// BraidMetadataSchemaTag.
pub const BRAID_SCHEMA: u16 = 2; // BraidMetadataSchemaTag

// CSV files
pub const KALMAN_ESTIMATES_FNAME: &str = "kalman_estimates";
pub const DATA_ASSOCIATE_FNAME: &str = "data_association";
pub const CALIBRATION_XML_FNAME: &str = "calibration";
pub const DATA2D_DISTORTED_CSV_FNAME: &str = "data2d_distorted";
pub const CAM_INFO_CSV_FNAME: &str = "cam_info";
pub const TRIGGER_CLOCK_INFO: &str = "trigger_clock_info";
pub const EXPERIMENT_INFO: &str = "experiment_info";
pub const TEXTLOG: &str = "textlog";

// Other files
pub const BRAID_METADATA_YML_FNAME: &str = "braid_metadata";
pub const README_WITH_EXT: &str = "README.md";
pub const IMAGES_DIRNAME: &str = "images";
pub const RECONSTRUCT_LATENCY_LOG_FNAME: &str = "reconstruct_latency_usec.hlog";
pub const REPROJECTION_DIST_LOG_FNAME: &str = "reprojection_distance_100x_pixels.hlog";

// --------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CamInfoRow {
    // changes to this should update BraidMetadataSchemaTag
    pub camn: CamNum,
    pub cam_id: String,
    // pub hostname: String,
}

#[allow(non_snake_case)]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct KalmanEstimatesRow {
    // changes to this struct should update BraidMetadataSchemaTag
    pub obj_id: u32,
    pub frame: SyncFno,
    /// The timestamp when the trigger pulse fired.
    ///
    /// Note that calculating this live in braid requires that the clock model
    /// has established itself. Thus, the initial frames immediately after
    /// synchronization will not have a timestamp.
    #[serde(with = "crate::timestamp_opt_f64")]
    pub timestamp: Option<FlydraFloatTimestampLocal<Triggerbox>>,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub xvel: f64,
    pub yvel: f64,
    pub zvel: f64,
    pub P00: f64,
    pub P01: f64,
    pub P02: f64,
    pub P11: f64,
    pub P12: f64,
    pub P22: f64,
    pub P33: f64,
    pub P44: f64,
    pub P55: f64,
}
impl WithKey<SyncFno> for KalmanEstimatesRow {
    fn key(&self) -> SyncFno {
        self.frame
    }
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct FlydraRawUdpPoint {
    pub x0_abs: f64,
    pub y0_abs: f64,
    pub area: f64,
    pub maybe_slope_eccentricty: Option<(f64, f64)>,
    pub cur_val: u8,
    pub mean_val: f64,
    pub sumsqf_val: f64,
}

/// The original camera name from the driver.
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, Eq, PartialOrd, Ord)]
pub struct RawCamName(String);

impl RawCamName {
    pub fn new(s: String) -> Self {
        RawCamName(s)
    }
    pub fn to_ros(&self) -> RosCamName {
        let ros_name: String = self.0.replace("-", "_");
        let ros_name: String = ros_name.replace(" ", "_");
        RosCamName::new(ros_name)
    }
}

/// Name that works as a ROS node name (i.e. no '-' or ' ' chars).
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, Eq, PartialOrd, Ord)]
pub struct RosCamName(String);

impl RosCamName {
    pub fn new(s: String) -> Self {
        RosCamName(s)
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for RosCamName {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::result::Result<(), std::fmt::Error> {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct RegisterNewCamera {
    /// The raw name of the camera as given by the camera itself.
    pub orig_cam_name: RawCamName,
    /// The name of the camera used in ROS (e.g. with '-' converted to '_').
    pub ros_cam_name: RosCamName,
    /// Location of the camera control HTTP server.
    pub http_camserver_info: CamHttpServerInfo,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct UpdateImage {
    // /// The raw name of the camera as given by the camera itself.
    // pub orig_cam_name: RawCamName,
    /// The name of the camera used in ROS (e.g. with '-' converted to '_').
    pub ros_cam_name: RosCamName,
    pub current_image_png: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ConnectedCameraSyncState {
    /// No known reference to other cameras
    Unsynchronized,
    /// This `u64` is frame0, the offset to go from camera frame to sync frame.
    Synchronized(u64),
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct HttpApiShared {
    pub clock_model_copy: Option<ClockModel>,
    pub csv_tables_dirname: Option<RecordingPath>,
    pub calibration_filename: Option<String>,
    pub connected_cameras: Vec<CamInfo>, // TODO: make this a BTreeMap?
    pub model_server_addr: Option<std::net::SocketAddr>,
    pub flydra_app_name: String,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct RecentStats {
    pub total_frames_collected: usize,
    pub frames_collected: usize,
    pub points_detected: usize,
}

impl Default for RecentStats {
    fn default() -> Self {
        Self {
            total_frames_collected: 0,
            frames_collected: 0,
            points_detected: 0,
        }
    }
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub enum CamHttpServerInfo {
    /// No server is present (e.g. prerecorded data).
    NoServer,
    /// A server is available.
    Server(BuiServerInfo),
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct BuiServerInfo {
    /// The address of the camera control HTTP server.
    addr: std::net::SocketAddr,
    /// The token for initial connection to the camera control HTTP server.
    token: AccessToken,
    resolved_addr: String,
}

impl BuiServerInfo {
    #[cfg(feature = "with-dns")]
    pub fn new(addr: std::net::SocketAddr, token: AccessToken) -> Self {
        let resolved_addr = if addr.ip().is_unspecified() {
            format!("{}:{}", dns_lookup::get_hostname().unwrap(), addr.port())
        } else {
            format!("{}", addr)
        };
        Self {
            addr,
            token,
            resolved_addr,
        }
    }

    pub fn guess_base_url_with_token(&self) -> String {
        match self.token {
            AccessToken::NoToken => format!("http://{}/", self.resolved_addr),
            AccessToken::PreSharedToken(ref tok) => {
                format!("http://{}/?token={}", self.resolved_addr, tok)
            }
        }
    }

    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn token(&self) -> &AccessToken {
        &self.token
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TextlogRow {
    // changes to this struct should update BraidMetadataSchemaTag
    pub mainbrain_timestamp: f64,
    pub cam_id: String,
    pub host_timestamp: f64,
    pub message: String,
}

/// Tracking parameters
///
/// This is the implementation for (de)serialization. See
/// `TrackingParamsInner3D` and `TrackingParamsInnerFlat3D` for actual tracking
/// usage. We have these two implementations so that we can have a compile-time
/// switch for 3d vs 2d tracking but a common format for serialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackingParams {
    /// kalman filter parameter
    pub motion_noise_scale: f64,
    /// kalman filter parameter
    pub initial_position_std_meters: f64,
    /// kalman filter parameter
    pub initial_vel_std_meters_per_sec: f64,
    /// kalman filter parameter
    pub ekf_observation_covariance_pixels: f64,
    /// data association parameter
    pub accept_observation_min_likelihood: f64,
    /// data association parameter
    pub max_position_std_meters: f32,
    /// hypothesis testing parameters
    ///
    /// This is `None` if 2D (flat-3d) tracking.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hypothesis_test_params: Option<HypothesisTestParams>,
    /// minimum number of observations before object becomes visible
    #[serde(default = "default_num_observations_to_visibility")]
    pub num_observations_to_visibility: u8,
}

fn default_num_observations_to_visibility() -> u8 {
    // This number should suppress spurious trajectory births but not wait too
    // long before notifying listeners.
    3
}

pub type MyFloat = f64;

/// Tracking parameters actually used for tracking.
#[derive(Debug, Clone)]
pub struct TrackingParamsInner3D {
    /// kalman filter parameter
    pub motion_noise_scale: MyFloat,
    /// kalman filter parameter
    pub initial_position_std_meters: MyFloat,
    /// kalman filter parameter
    pub initial_vel_std_meters_per_sec: MyFloat,
    /// kalman filter parameter
    pub ekf_observation_covariance_pixels: f32,
    /// data association parameter
    pub accept_observation_min_likelihood: f64,
    /// data association parameter
    pub max_position_std_meters: f32,
    /// hypothesis testing parameters
    pub hypothesis_test_params: HypothesisTestParams,
    /// minimum number of observations before object becomes visible
    pub num_observations_to_visibility: u8,
}

impl Into<TrackingParams> for TrackingParamsInner3D {
    fn into(self) -> TrackingParams {
        let hypothesis_test_params = Some(self.hypothesis_test_params);

        TrackingParams {
            motion_noise_scale: self.motion_noise_scale,
            initial_position_std_meters: self.initial_position_std_meters,
            initial_vel_std_meters_per_sec: self.initial_vel_std_meters_per_sec,
            ekf_observation_covariance_pixels: self.ekf_observation_covariance_pixels.into(),
            accept_observation_min_likelihood: self.accept_observation_min_likelihood,
            max_position_std_meters: self.max_position_std_meters,
            hypothesis_test_params,
            num_observations_to_visibility: self.num_observations_to_visibility,
        }
    }
}

impl TryFrom<TrackingParams> for TrackingParamsInner3D {
    type Error = FlydraTypesError;

    fn try_from(orig: TrackingParams) -> Result<Self> {
        TryFrom::try_from(&orig)
    }
}

impl TryFrom<&TrackingParams> for TrackingParamsInner3D {
    type Error = FlydraTypesError;

    fn try_from(orig: &TrackingParams) -> Result<Self> {
        let hypothesis_test_params = match orig.hypothesis_test_params {
            Some(ref o) => o.clone(),
            None => make_hypothesis_test_full3d_default(),
        };

        Ok(Self {
            motion_noise_scale: orig.motion_noise_scale,
            initial_position_std_meters: orig.initial_position_std_meters,
            initial_vel_std_meters_per_sec: orig.initial_vel_std_meters_per_sec,
            ekf_observation_covariance_pixels: orig.ekf_observation_covariance_pixels as f32,
            accept_observation_min_likelihood: orig.accept_observation_min_likelihood,
            max_position_std_meters: orig.max_position_std_meters,
            num_observations_to_visibility: orig.num_observations_to_visibility,
            hypothesis_test_params,
        })
    }
}

impl Default for TrackingParamsInner3D {
    fn default() -> Self {
        Self {
            motion_noise_scale: 0.1,
            initial_position_std_meters: 0.1,
            initial_vel_std_meters_per_sec: 1.0,
            accept_observation_min_likelihood: 1e-8,
            ekf_observation_covariance_pixels: 1.0,
            max_position_std_meters: 0.01212,
            hypothesis_test_params: make_hypothesis_test_full3d_default(),
            num_observations_to_visibility: default_num_observations_to_visibility(),
        }
    }
}

/// Tracking parameters actually used for tracking.
#[derive(Debug, Clone)]
pub struct TrackingParamsInnerFlat3D {
    /// kalman filter parameter
    pub motion_noise_scale: MyFloat,
    /// kalman filter parameter
    pub initial_position_std_meters: MyFloat,
    /// kalman filter parameter
    pub initial_vel_std_meters_per_sec: MyFloat,
    /// kalman filter parameter
    pub ekf_observation_covariance_pixels: f32,
    /// data association parameter
    pub accept_observation_min_likelihood: f64,
    /// data association parameter
    pub max_position_std_meters: f32,
    /// minimum number of observations before object becomes visible
    pub num_observations_to_visibility: u8,
}

impl Into<TrackingParams> for TrackingParamsInnerFlat3D {
    fn into(self) -> TrackingParams {
        let hypothesis_test_params = None;

        TrackingParams {
            motion_noise_scale: self.motion_noise_scale,
            initial_position_std_meters: self.initial_position_std_meters,
            initial_vel_std_meters_per_sec: self.initial_vel_std_meters_per_sec,
            ekf_observation_covariance_pixels: self.ekf_observation_covariance_pixels.into(),
            accept_observation_min_likelihood: self.accept_observation_min_likelihood,
            max_position_std_meters: self.max_position_std_meters,
            hypothesis_test_params,
            num_observations_to_visibility: self.num_observations_to_visibility,
        }
    }
}

impl TryFrom<TrackingParams> for TrackingParamsInnerFlat3D {
    type Error = FlydraTypesError;

    fn try_from(orig: TrackingParams) -> Result<Self> {
        if orig.hypothesis_test_params.is_some() {
            return Err(FlydraTypesError::UnexpectedHypothesisTestingParameters);
        }

        Ok(Self {
            motion_noise_scale: orig.motion_noise_scale,
            initial_position_std_meters: orig.initial_position_std_meters,
            initial_vel_std_meters_per_sec: orig.initial_vel_std_meters_per_sec,
            ekf_observation_covariance_pixels: orig.ekf_observation_covariance_pixels as f32,
            accept_observation_min_likelihood: orig.accept_observation_min_likelihood,
            max_position_std_meters: orig.max_position_std_meters,
            num_observations_to_visibility: orig.num_observations_to_visibility,
        })
    }
}

impl Default for TrackingParamsInnerFlat3D {
    fn default() -> Self {
        Self {
            motion_noise_scale: 10.0,
            initial_position_std_meters: 0.001,
            initial_vel_std_meters_per_sec: 1.0,
            accept_observation_min_likelihood: 1e-8,
            ekf_observation_covariance_pixels: 10.0,
            max_position_std_meters: 0.2,
            num_observations_to_visibility: default_num_observations_to_visibility(),
        }
    }
}

/// Hypothesis testing parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HypothesisTestParams {
    pub minimum_number_of_cameras: u8,
    pub hypothesis_test_max_acceptable_error: f64,
    pub minimum_pixel_abs_zscore: f64,
}

pub fn make_hypothesis_test_full3d_default() -> HypothesisTestParams {
    HypothesisTestParams {
        minimum_number_of_cameras: 2,
        hypothesis_test_max_acceptable_error: 5.0,
        minimum_pixel_abs_zscore: 0.0,
    }
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct CamInfo {
    pub name: RosCamName,
    pub state: ConnectedCameraSyncState,
    pub http_camserver_info: CamHttpServerInfo,
    pub recent_stats: RecentStats,
}

/// Messages to the mainbrain
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum HttpApiCallback {
    /// Called from strand-cam to register a camera
    NewCamera(RegisterNewCamera),
    /// Called from strand-cam to update the current image
    UpdateCurrentImage(UpdateImage),
    /// Trigger synchronization of the cameras
    DoSyncCameras,
    /// Start or stop recording data (csv tables)
    DoRecordCsvTables(bool),
    /// set uuid in the experiment_info table
    SetExperimentUuid(String),
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct FlydraRawUdpPacket {
    pub cam_name: String,
    /// frame timestamp of trigger pulse start (or None if cannot be determined)
    #[serde(with = "crate::timestamp_opt_f64")]
    pub timestamp: Option<FlydraFloatTimestampLocal<Triggerbox>>,
    /// frame timestamp of camnode program sampling system clock
    #[serde(with = "crate::timestamp_f64")]
    pub cam_received_time: FlydraFloatTimestampLocal<HostClock>,
    pub framenumber: i32,
    pub n_frames_skipped: u32,
    /// this will always be 0.0 for flydra1 custom serialized packets
    pub done_camnode_processing: f64,
    /// this will always be 0.0 for flydra1 custom serialized packets
    pub preprocess_stamp: f64,
    /// this will always be 0 for flydra1 custom serialized packets
    pub image_processing_steps: ImageProcessingSteps,
    pub points: Vec<FlydraRawUdpPoint>,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct FlydraRawUdpPacketHeader {
    pub cam_name: String,
    /// frame timestamp of trigger pulse start (or 0.0 if cannot be determined)
    #[serde(with = "crate::timestamp_opt_f64")]
    pub timestamp: Option<FlydraFloatTimestampLocal<Triggerbox>>,
    /// frame timestamp of camnode program sampling system clock
    #[serde(with = "crate::timestamp_f64")]
    pub cam_received_time: FlydraFloatTimestampLocal<HostClock>,
    pub framenumber: i32,
    pub n_frames_skipped: u32,
    /// this will always be 0.0 for flydra1 custom serialized packets
    pub done_camnode_processing: f64,
    /// this will always be 0.0 for flydra1 custom serialized packets
    pub preprocess_stamp: f64,
    /// this will always be 0 for flydra1 custom serialized packets
    pub image_processing_steps: ImageProcessingSteps,
    pub len_points: usize,
}

impl FlydraRawUdpPacket {
    pub fn from_header_and_points(
        header: FlydraRawUdpPacketHeader,
        points: std::vec::Vec<FlydraRawUdpPoint>,
    ) -> Self {
        assert!(header.len_points == points.len());
        Self {
            cam_name: header.cam_name,
            timestamp: header.timestamp,
            cam_received_time: header.cam_received_time,
            framenumber: header.framenumber,
            n_frames_skipped: header.n_frames_skipped,
            done_camnode_processing: header.done_camnode_processing,
            preprocess_stamp: header.preprocess_stamp,
            image_processing_steps: header.image_processing_steps,
            points,
        }
    }
}

mod synced_frame;
pub use synced_frame::SyncFno;

mod cam_num;
pub use cam_num::CamNum;

mod timestamp;
pub use crate::timestamp::{FlydraFloatTimestampLocal, HostClock, Source, Triggerbox};

pub mod timestamp_f64;
pub mod timestamp_opt_f64;

mod serialize;
pub use crate::serialize::{
    deserialize_packet, deserialize_point, serialize_packet, serialize_point, ReadFlydraExt,
    CBOR_MAGIC, FLYDRA1_PACKET_HEADER_SIZE, FLYDRA1_PER_POINT_PAYLOAD_SIZE,
};

#[cfg(feature = "with-tokio-codec")]
mod tokio_flydra1;
#[cfg(feature = "with-tokio-codec")]
pub use crate::tokio_flydra1::FlydraPacketCodec;

#[cfg(feature = "with-tokio-codec")]
mod tokio_cbor;
#[cfg(feature = "with-tokio-codec")]
pub use crate::tokio_cbor::CborPacketCodec;

type Result<M> = std::result::Result<M, FlydraTypesError>;

#[derive(thiserror::Error, Debug)]
pub enum FlydraTypesError {
    #[error("CBOR data")]
    CborDataError,
    #[error("serde error")]
    SerdeError,
    #[error("unexpected hypothesis testing parameters")]
    UnexpectedHypothesisTestingParameters,
    #[error("input too long")]
    InputTooLong,
    #[error("long string not implemented")]
    LongStringNotImplemented,
    #[error("{0}")]
    IoError(#[from] std::io::Error),
    #[error("{0}")]
    Utf8Error(#[from] std::str::Utf8Error),
}

#[derive(Deserialize, Serialize, Debug)]
pub struct AddrInfoUnixDomainSocket {
    pub filename: String,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct AddrInfoIP {
    inner: std::net::SocketAddr,
}

impl AddrInfoIP {
    pub fn from_socket_addr(src: &std::net::SocketAddr) -> Self {
        Self { inner: *src }
    }
    pub fn to_socket_addr(&self) -> std::net::SocketAddr {
        self.inner
    }
    pub fn ip(&self) -> std::net::IpAddr {
        self.inner.ip()
    }
    pub fn port(&self) -> u16 {
        self.inner.port()
    }
}

#[derive(Debug)]
pub enum RealtimePointsDestAddr {
    UnixDomainSocket(AddrInfoUnixDomainSocket),
    IpAddr(AddrInfoIP),
}

impl RealtimePointsDestAddr {
    pub fn into_string(self) -> String {
        match self {
            RealtimePointsDestAddr::UnixDomainSocket(uds) => format!("file://{}", uds.filename),
            RealtimePointsDestAddr::IpAddr(ip) => format!("http://{}:{}", ip.ip(), ip.port()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MainbrainBuiLocation(pub BuiServerInfo);

#[derive(Debug, Serialize, Deserialize)]
pub struct TriggerClockInfoRow {
    // changes to this should update BraidMetadataSchemaTag
    #[serde(with = "crate::timestamp_f64")]
    pub start_timestamp: FlydraFloatTimestampLocal<HostClock>,
    pub framecount: i64,
    pub tcnt: u8,
    #[serde(with = "crate::timestamp_f64")]
    pub stop_timestamp: FlydraFloatTimestampLocal<HostClock>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct StaticMainbrainInfo {
    pub name: String,
    pub version: String,
}

bitflags! {
    #[derive(Serialize, Deserialize)]
    pub struct ImageProcessingSteps: u8 {
        const BGINIT    = 0b00000001;
        const BGSTARTUP = 0b00000010;
        const BGCLEARED = 0b00000100;
        const BGUPDATE  = 0b00001000;
        const BGNORMAL  = 0b00010000;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriggerboxConfig {
    pub device_fname: String,
    pub framerate: f32,
    pub query_dt: std::time::Duration,
}

impl std::default::Default for TriggerboxConfig {
    fn default() -> Self {
        Self {
            device_fname: "/dev/trig1".to_string(),
            framerate: 100.0,
            query_dt: std::time::Duration::from_millis(1500),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Data2dDistortedRow {
    // changes to this should update BraidMetadataSchemaTag
    // should be kept in sync with Data2dDistortedRowF32
    pub camn: CamNum,
    pub frame: i64,
    /// This is the trigger timestamp (if available).
    #[serde(with = "crate::timestamp_opt_f64")]
    pub timestamp: Option<FlydraFloatTimestampLocal<Triggerbox>>,
    #[serde(with = "crate::timestamp_f64")]
    pub cam_received_timestamp: FlydraFloatTimestampLocal<HostClock>,
    #[serde(deserialize_with = "invalid_nan")]
    pub x: f64,
    #[serde(deserialize_with = "invalid_nan")]
    pub y: f64,
    #[serde(deserialize_with = "invalid_nan")]
    pub area: f64,
    #[serde(deserialize_with = "invalid_nan")]
    pub slope: f64,
    #[serde(deserialize_with = "invalid_nan")]
    pub eccentricity: f64,
    pub frame_pt_idx: u8,
    pub cur_val: u8,
    #[serde(deserialize_with = "invalid_nan")]
    pub mean_val: f64,
    #[serde(deserialize_with = "invalid_nan")]
    pub sumsqf_val: f64,
}

// Lower precision version of the above for saving to disk.
// Note that this matches the precision specified in
// `flydra_core.data_descriptions.Info2D`.
#[derive(Debug, Serialize)]
pub struct Data2dDistortedRowF32 {
    // changes to this should update BraidMetadataSchemaTag
    pub camn: CamNum,
    pub frame: i64,
    /// This is the trigger timestamp (if available).
    #[serde(with = "crate::timestamp_opt_f64")]
    pub timestamp: Option<FlydraFloatTimestampLocal<Triggerbox>>,
    #[serde(with = "crate::timestamp_f64")]
    pub cam_received_timestamp: FlydraFloatTimestampLocal<HostClock>,
    pub x: f32,
    pub y: f32,
    pub area: f32,
    pub slope: f32,
    pub eccentricity: f32,
    pub frame_pt_idx: u8,
    pub cur_val: u8,
    pub mean_val: f32,
    pub sumsqf_val: f32,
}

impl WithKey<i64> for Data2dDistortedRow {
    fn key(&self) -> i64 {
        self.frame
    }
}

fn invalid_nan<'de, D>(de: D) -> std::result::Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    f64::deserialize(de).or(
        // TODO: should match on DeserializeError with empty field only,
        // otherwise, return error. The way this is written, anything
        // will return a nan.
        Ok(std::f64::NAN),
    )
}

pub const BRAID_EVENTS_URL_PATH: &str = "braid-events";
pub const BRAID_EVENT_NAME: &str = "braid";
