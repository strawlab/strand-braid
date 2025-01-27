// Copyright 2020-2023 Andrew D. Straw.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT
// or http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

#[macro_use]
extern crate bitflags;
#[macro_use]
extern crate static_assertions;

use ordered_float::NotNan;
use rust_cam_bui_types::{ClockModel, RecordingPath};
use std::net::SocketAddr;

use serde::{Deserialize, Deserializer, Serialize};

use bui_backend_session_types::AccessToken;
use withkey::WithKey;

pub const DEFAULT_MODEL_SERVER_ADDR: &str = "0.0.0.0:8397";

// These are the filenames saved during recording. --------------------
//
// Any changes to these names, including additions and removes, should update
// BraidMetadataSchemaTag.
pub const BRAID_SCHEMA: u16 = 3; // BraidMetadataSchemaTag

// CSV files. (These may also exist as .csv.gz)
pub const KALMAN_ESTIMATES_CSV_FNAME: &str = "kalman_estimates.csv";
pub const DATA_ASSOCIATE_CSV_FNAME: &str = "data_association.csv";
pub const DATA2D_DISTORTED_CSV_FNAME: &str = "data2d_distorted.csv";
pub const CAM_INFO_CSV_FNAME: &str = "cam_info.csv";
pub const TRIGGER_CLOCK_INFO_CSV_FNAME: &str = "trigger_clock_info.csv";
pub const EXPERIMENT_INFO_CSV_FNAME: &str = "experiment_info.csv";
pub const TEXTLOG_CSV_FNAME: &str = "textlog.csv";

// Other files
pub const CALIBRATION_XML_FNAME: &str = "calibration.xml";
pub const BRAID_METADATA_YML_FNAME: &str = "braid_metadata.yml";
pub const README_MD_FNAME: &str = "README.md";
pub const IMAGES_DIRNAME: &str = "images";
pub const CAM_SETTINGS_DIRNAME: &str = "cam_settings";
pub const FEATURE_DETECT_SETTINGS_DIRNAME: &str = "feature_detect_settings";
pub const RECONSTRUCT_LATENCY_HLOG_FNAME: &str = "reconstruct_latency_usec.hlog";
pub const REPROJECTION_DIST_HLOG_FNAME: &str = "reprojection_distance_100x_pixels.hlog";

pub const TRIGGERBOX_SYNC_SECONDS: u64 = 3;

// Ideas for future:
//
// Make tracking model and parameters "pluggable" so that other models - with
// different structure - can be easily used.
//
// **statistics cache for data2d_distorted** We could keep a statistics cache as
// we write a braidz file for things like num found points, average and maximum
// values etc. This could be periodically flushed to disk and recomputed anytime
// but would eliminate most needs to iterate over the entire dataset at read
// time.
//
// **statistics cache for kalman_estimates** Same as above but 3D.
//
// Cache the camera pixel sizes. Currently this can be found if images are saved
// or if the a camera calibration is present. The images in theory are always
// there but this is not currently implemented in the strand-cam "flydratrax"
// mode. Even when that is fixed, to simply read the image size that way will
// require parsing an entire image parser.
//
// Replace `TrackingParams.initial_position_std_meters` and
// `TrackingParams.initial_vel_std_meters_per_sec` with a scaled version of the
// process covariance matrix Q. According to this ([p.
// 18](https://www.robots.ox.ac.uk/~ian/Teaching/Estimation/LectureNotes2.pdf)),
// this approach is common with a scale factor of 10.
// --------------------------------------------------------------------

// Changes to this struct should update BraidMetadataSchemaTag.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CamInfoRow {
    /// The index of the camera. This changes from invocation to invocation of Braid.
    pub camn: CamNum,
    /// The name of the camera. This is stable across invocations of Braid.
    ///
    /// Any valid UTF-8 string is possible. (Previously, this was the "ROS name"
    /// of the camera in which, e.g. '-' was replaced with '_'. This is no
    /// longer the case.)
    pub cam_id: String,
}

// Changes to this struct should update BraidMetadataSchemaTag.
#[allow(non_snake_case)]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct KalmanEstimatesRow {
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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DataAssocRow {
    // changes to this struct should update BraidMetadataSchemaTag
    pub obj_id: u32,
    pub frame: SyncFno,
    pub cam_num: CamNum,
    pub pt_idx: u8,
}
impl WithKey<SyncFno> for DataAssocRow {
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
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for RawCamName {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub mod braid_http {
    // URL paths on Braid HTTP server.
    pub const REMOTE_CAMERA_INFO_PATH: &str = "remote-camera-info";
    pub const CAM_PROXY_PATH: &str = "cam-proxy";

    /// Encode camera name, potentially with slashes or spaces, to be a single
    /// URL path component.
    ///
    /// Use percent-encoding, which `axum::extract::Path` automatically decodes.
    pub fn encode_cam_name(cam_name: &crate::RawCamName) -> String {
        percent_encoding::utf8_percent_encode(&cam_name.0, percent_encoding::NON_ALPHANUMERIC)
            .to_string()
    }
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, Default)]
pub enum StartSoftwareFrameRateLimit {
    /// Set the frame_rate limit at a given frame rate.
    Enable(f64),
    /// Disable the frame_rate limit.
    Disabled,
    /// Do not change the frame rate limit.
    #[default]
    NoChange,
}

/// This contains information that Strand Camera needs to start the camera.
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct RemoteCameraInfoResponse {
    /// The destination UDP port to use for low-latency tracking data
    pub camdata_udp_port: u16,
    pub config: BraidCameraConfig,
    pub force_camera_sync_mode: bool,
    pub software_limit_framerate: StartSoftwareFrameRateLimit,
    /// camera triggering configuration (global for all cameras)
    pub trig_config: TriggerType,
}

/// Newtype storing time as number of nanoseconds since Jan 1, 1970 in UTC.
///
/// This is the lower 64 bits of the 80 bit PTP timestamp.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PtpStamp(u64);

impl PtpStamp {
    pub fn new(val: u64) -> Self {
        PtpStamp(val)
    }

    pub fn get(&self) -> u64 {
        self.0
    }

    pub fn duration_since(&self, other: &Self) -> Option<PtpStampDuration> {
        if self.0 >= other.0 {
            Some(PtpStampDuration(self.0 - other.0))
        } else {
            None
        }
    }
}

/// Newtype storing a duration between two [PtpStamp] values.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PtpStampDuration(u64);

impl PtpStampDuration {
    pub fn nanos(&self) -> u64 {
        self.0
    }
}

impl<TZ> TryFrom<chrono::DateTime<TZ>> for PtpStamp
where
    TZ: chrono::TimeZone,
{
    type Error = &'static str;

    fn try_from(orig: chrono::DateTime<TZ>) -> Result<Self, Self::Error> {
        Ok(Self(
            orig.to_utc()
                .timestamp_nanos_opt()
                .ok_or("could not convert DateTime to i64 nanosec")?
                .try_into()
                .map_err(|_| "could not convert i64 nanosec to u64")?,
        ))
    }
}

impl TryFrom<PtpStamp> for chrono::DateTime<chrono::Utc> {
    type Error = &'static str;
    fn try_from(orig: PtpStamp) -> Result<Self, Self::Error> {
        let secs = orig.0 / 1_000_000_000;
        let nsecs = orig.0 % 1_000_000_000;
        chrono::DateTime::from_timestamp(
            secs.try_into()
                .map_err(|_| "could not convert u64 nanosec to i64")?,
            nsecs
                .try_into()
                .map_err(|_| "could not convert u64 nanosec to u32")?,
        )
        .ok_or("could not convert timestamp to DateTime")
    }
}

impl TryFrom<PtpStamp> for chrono::DateTime<chrono::FixedOffset> {
    type Error = &'static str;
    fn try_from(orig: PtpStamp) -> Result<Self, Self::Error> {
        let utc: chrono::DateTime<chrono::Utc> = orig.try_into()?;
        Ok(utc.into())
    }
}

impl TryFrom<PtpStamp> for chrono::DateTime<chrono::Local> {
    type Error = &'static str;
    fn try_from(orig: PtpStamp) -> Result<Self, Self::Error> {
        let utc: chrono::DateTime<chrono::Utc> = orig.try_into()?;
        Ok(utc.into())
    }
}

pub const DEFAULT_ACQUISITION_DURATION_ALLOWED_IMPRECISION_MSEC: Option<f64> = Some(5.0);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BraidCameraConfig {
    /// The name of the camera (e.g. "Basler-22005677")
    ///
    /// (This is the original UTF-8 camera name, not the ROS-encoded camera name
    /// in which certain characters are not allowed.)
    pub name: String,
    /// Filename of vendor-specific camera settings file.
    ///
    /// Can contain shell variables such as `~`, `$A`, or `${B}`.
    pub camera_settings_filename: Option<std::path::PathBuf>,
    /// The pixel format to use.
    pub pixel_format: Option<String>,
    /// Configuration for detecting points.
    #[serde(default = "flydra_pt_detect_cfg::default_absdiff")]
    pub point_detection_config: flydra_feature_detector_types::ImPtDetectCfg,
    /// Which camera backend to use.
    #[serde(default)]
    pub start_backend: StartCameraBackend,
    pub acquisition_duration_allowed_imprecision_msec: Option<f64>,
    /// The SocketAddr on which the strand camera BUI server should run.
    pub http_server_addr: Option<String>,
    /// The interval at which the current image should be sent, in milliseconds.
    #[serde(default = "default_send_current_image_interval_msec")]
    pub send_current_image_interval_msec: u64,

    /// Deprecated, useless old config option (not removed for backwards compatibility)
    #[serde(
        default,
        skip_serializing,
        rename = "raise_grab_thread_priority",
        deserialize_with = "raise_grab_thread_priority_deser"
    )]
    _raise_grab_thread_priority: bool,
}

fn raise_grab_thread_priority_deser<'de, D>(de: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    tracing::error!("The parameter 'raise_grab_thread_priority' is no longer used. Remove this parameter from your configuration.");
    bool::deserialize(de)
}

const fn default_send_current_image_interval_msec() -> u64 {
    2000
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum StartCameraBackend {
    /// Do not start a camera locally. Rather, wait for a remote camera to connect.
    Remote,
    /// Start a Pylon camera locally using `strand-cam-pylon` program.
    #[default]
    Pylon,
    /// Start a Vimba camera locally using `strand-cam-vimba` program.
    Vimba,
}

impl StartCameraBackend {
    pub fn strand_cam_exe_name(&self) -> Option<&str> {
        match self {
            StartCameraBackend::Remote => None,
            StartCameraBackend::Pylon => Some("strand-cam-pylon"),
            StartCameraBackend::Vimba => Some("strand-cam-vimba"),
        }
    }
}

impl BraidCameraConfig {
    pub fn default_absdiff_config(name: String) -> Self {
        Self {
            name,
            camera_settings_filename: None,
            pixel_format: None,
            point_detection_config: flydra_pt_detect_cfg::default_absdiff(),
            _raise_grab_thread_priority: Default::default(),
            start_backend: Default::default(),
            acquisition_duration_allowed_imprecision_msec:
                DEFAULT_ACQUISITION_DURATION_ALLOWED_IMPRECISION_MSEC,
            http_server_addr: None,
            send_current_image_interval_msec: default_send_current_image_interval_msec(),
        }
    }
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct PerCamSaveData {
    pub current_image_png: PngImageData,
    pub cam_settings_data: Option<UpdateCamSettings>,
    pub feature_detect_settings: Option<UpdateFeatureDetectSettings>,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct RegisterNewCamera {
    /// The name of the camera as returned by the camera
    pub raw_cam_name: RawCamName,
    /// Location of the camera control HTTP server.
    pub http_camserver_info: Option<BuiServerInfo>,
    /// The camera settings.
    pub cam_settings_data: Option<UpdateCamSettings>,
    /// The current image.
    pub current_image_png: PngImageData,
    /// The period of the periodic signal generator in the camera.
    /// This is used for PTP-based synchronization.
    pub camera_periodic_signal_period_usec: Option<f64>,
}

#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub struct UpdateImage {
    /// The current image.
    pub current_image_png: PngImageData,
}

#[derive(PartialEq, Eq, Clone, Serialize, Deserialize)]
pub struct PngImageData {
    pub data: Vec<u8>,
}

impl From<Vec<u8>> for PngImageData {
    fn from(data: Vec<u8>) -> Self {
        Self { data }
    }
}

impl PngImageData {
    pub fn as_slice(&self) -> &[u8] {
        self.data.as_slice()
    }
}

impl std::fmt::Debug for PngImageData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PngImageData{{..}}",)
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub struct UpdateCamSettings {
    /// The current camera settings
    pub current_cam_settings_buf: String,
    /// The filename extension for the camera settings
    pub current_cam_settings_extension: String,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct UpdateFeatureDetectSettings {
    /// The current feature detection settings.
    pub current_feature_detect_settings: flydra_feature_detector_types::ImPtDetectCfg,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectedCameraSyncState {
    /// No known reference to other cameras
    Unsynchronized,
    /// This `u64` is frame0, the offset to go from camera frame to sync frame.
    Synchronized(u64),
}

impl ConnectedCameraSyncState {
    pub fn is_synchronized(&self) -> bool {
        match self {
            ConnectedCameraSyncState::Unsynchronized => false,
            ConnectedCameraSyncState::Synchronized(_) => true,
        }
    }
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct BraidHttpApiSharedState {
    pub trigger_type: TriggerType,
    pub needs_clock_model: bool,
    pub clock_model: Option<ClockModel>,
    pub csv_tables_dirname: Option<RecordingPath>,
    // This is "fake" because it only signals if each of the connected computers
    // is recording MKVs.
    pub fake_mp4_recording_path: Option<RecordingPath>,
    pub post_trigger_buffer_size: usize,
    pub calibration_filename: Option<String>,
    pub connected_cameras: Vec<CamInfo>, // TODO: make this a BTreeMap?
    pub model_server_addr: Option<SocketAddr>,
    pub flydra_app_name: String,
    pub all_expected_cameras_are_synced: bool,
}

#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize, Default)]
pub struct RecentStats {
    pub total_frames_collected: usize,
    pub frames_collected: usize,
    pub points_detected: usize,
}

/// Generic HTTP API server information
///
/// This is used for both the Strand Camera BUI and the Braid BUI.
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub enum BuiServerInfo {
    /// No server is present (e.g. prerecorded data).
    NoServer,
    /// A server is available.
    Server(BuiServerAddrInfo),
}

/// HTTP API server access information
///
/// This contains the address and access token.
///
/// This is used for both the Strand Camera BUI and the Braid BUI.
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct BuiServerAddrInfo {
    /// The listen address of the HTTP server.
    ///
    /// Note that this can be unspecified (i.e. `0.0.0.0` for IPv4).
    addr: SocketAddr,
    /// The token for initial connection to the HTTP server.
    token: AccessToken,
}

impl BuiServerAddrInfo {
    pub fn new(addr: SocketAddr, token: AccessToken) -> Self {
        Self { addr, token }
    }

    pub fn addr(&self) -> &SocketAddr {
        &self.addr
    }

    pub fn token(&self) -> &AccessToken {
        &self.token
    }

    #[cfg(feature = "build-urls")]
    pub fn build_urls(&self) -> std::io::Result<Vec<http::Uri>> {
        let query = match &self.token {
            AccessToken::NoToken => "".to_string(),
            AccessToken::PreSharedToken(tok) => format!("?token={tok}"),
        };
        Ok(expand_unspecified_addr(self.addr())?
            .into_iter()
            .map(|specified_addr| {
                let addr = specified_addr.addr();
                http::uri::Builder::new()
                    .scheme("http")
                    .authority(format!("{}:{}", addr.ip(), addr.port()))
                    .path_and_query(format!("/{query}"))
                    .build()
                    .unwrap()
            })
            .collect())
    }

    pub fn parse_url_with_token(url: &str) -> Result<Self, FlydraTypesError> {
        // TODO: replace this ugly implementation...
        let stripped = url
            .strip_prefix("http://")
            .ok_or(FlydraTypesError::UrlParseError)?;
        let first_slash = stripped.find('/');
        let (addr_str, token) = if let Some(slash_idx) = first_slash {
            let path = &stripped[slash_idx..];
            if path == "/" || path == "/?" {
                (&stripped[..slash_idx], AccessToken::NoToken)
            } else {
                let token_str = path[1..]
                    .strip_prefix("?token=")
                    .ok_or(FlydraTypesError::UrlParseError)?;
                (
                    &stripped[..slash_idx],
                    AccessToken::PreSharedToken(token_str.to_string()),
                )
            }
        } else {
            (stripped, AccessToken::NoToken)
        };
        let addr = std::net::ToSocketAddrs::to_socket_addrs(addr_str)?
            .next()
            .ok_or(FlydraTypesError::UrlParseError)?;
        if addr.ip().is_unspecified() {
            // An unspecified IP (e.g. 0.0.0.0) is not a valid remotely visible
            // address.
            return Err(FlydraTypesError::UrlParseError);
        }
        Ok(Self::new(addr, token))
    }

    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }
}

pub fn is_loopback(url: &http::Uri) -> bool {
    let authority = match url.authority() {
        None => return false,
        Some(authority) => authority,
    };
    match authority.host() {
        "127.0.0.1" | "[::1]" => true,
        // should we include "localhost"? only if it actually resolves?
        _ => false,
    }
}

// -----

/// A newtype wrapping a [SocketAddr] which ensures that it is specified.
#[derive(Debug, PartialEq, Clone, Serialize)]
#[serde(transparent)]
pub struct SpecifiedSocketAddr(SocketAddr);

impl std::fmt::Display for SpecifiedSocketAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        std::fmt::Display::fmt(&self.0, f)
    }
}

impl SpecifiedSocketAddr {
    fn make_err() -> std::io::Error {
        std::io::ErrorKind::AddrNotAvailable.into()
    }
    pub fn new(addr: SocketAddr) -> std::io::Result<Self> {
        if addr.ip().is_unspecified() {
            return Err(Self::make_err());
        }
        Ok(Self(addr))
    }
    pub fn ip(&self) -> std::net::IpAddr {
        self.0.ip()
    }
    pub fn addr(&self) -> &std::net::SocketAddr {
        &self.0
    }
}

impl<'de> serde::Deserialize<'de> for SpecifiedSocketAddr {
    fn deserialize<D>(deserializer: D) -> std::result::Result<SpecifiedSocketAddr, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let addr: SocketAddr = std::net::SocketAddr::deserialize(deserializer)?;
        SpecifiedSocketAddr::new(addr).map_err(|_e| serde::de::Error::custom(Self::make_err()))
    }
}

#[cfg(feature = "start-listener")]
pub async fn start_listener(
    address_string: &str,
) -> eyre::Result<(tokio::net::TcpListener, BuiServerAddrInfo)> {
    let socket_addr = std::net::ToSocketAddrs::to_socket_addrs(&address_string)?
        .next()
        .ok_or_else(|| eyre::eyre!("no address found for HTTP server"))?;

    let listener = tokio::net::TcpListener::bind(socket_addr).await?;
    let listener_local_addr = listener.local_addr()?;
    let token_config = if !listener_local_addr.ip().is_loopback() {
        Some(axum_token_auth::TokenConfig::new_token("token"))
    } else {
        None
    };
    let token = match token_config {
        None => bui_backend_session_types::AccessToken::NoToken,
        Some(cfg) => bui_backend_session_types::AccessToken::PreSharedToken(cfg.value.clone()),
    };
    let http_camserver_info = BuiServerAddrInfo::new(listener_local_addr, token);

    Ok((listener, http_camserver_info))
}

// -----

#[cfg(feature = "build-urls")]
fn expand_unspecified_ip(ip: std::net::IpAddr) -> std::io::Result<Vec<std::net::IpAddr>> {
    if ip.is_unspecified() {
        // Get all interfaces if IP is unspecified.
        Ok(if_addrs::get_if_addrs()?
            .iter()
            .filter_map(|x| {
                let this_ip = x.addr.ip();
                // Take only IP addresses from correct family.
                if ip.is_ipv4() == this_ip.is_ipv4() {
                    Some(this_ip)
                } else {
                    None
                }
            })
            .collect())
    } else {
        Ok(vec![ip])
    }
}

#[cfg(feature = "build-urls")]
pub fn expand_unspecified_addr(addr: &SocketAddr) -> std::io::Result<Vec<SpecifiedSocketAddr>> {
    if addr.ip().is_unspecified() {
        expand_unspecified_ip(addr.ip())?
            .into_iter()
            .map(|ip| SpecifiedSocketAddr::new(SocketAddr::new(ip, addr.port())))
            .collect()
    } else {
        Ok(vec![SpecifiedSocketAddr::new(*addr).unwrap()])
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
/// The terminology used is as defined at [the Wikipedia page on the Kalman
/// filter](https://en.wikipedia.org/wiki/Kalman_filter).
///
/// The state estimated is a six component vector with position and velocity
/// **x** = \<x, y, z, x', y', z'\>. The motion model is a constant velocity
/// model with noise term, (see
/// [description](https://webee.technion.ac.il/people/shimkin/Estimation09/ch8_target.pdf)).
///
/// The state covariance matrix **P** is initialized with the value (α is
/// defined in the field [TrackingParams::initial_position_std_meters] and β is
/// defined in the field [TrackingParams::initial_vel_std_meters_per_sec]:<br/>
/// **P**<sub>initial</sub> = [[α<sup>2</sup>, 0, 0, 0, 0, 0],<br/>
/// [0, α<sup>2</sup>, 0, 0, 0, 0],<br/>
/// [0, 0, α<sup>2</sup>, 0, 0, 0],<br/>
/// [0, 0, 0, β<sup>2</sup>, 0, 0],<br/>
/// [0, 0, 0, 0, β<sup>2</sup>, 0],<br/>
/// [0, 0, 0, 0, 0, β<sup>2</sup>]]
///
/// The covariance of the state process update **Q**(τ) is defined as a function
/// of τ, the time interval from the previous update):<br/>
/// **Q**(τ) = [TrackingParams::motion_noise_scale] [[τ<sup>3</sup>/3, 0, 0, τ<sup>2</sup>/2, 0,
/// 0],<br/>
/// [0, τ<sup>3</sup>/3, 0, 0, τ<sup>2</sup>/2, 0],<br/>
/// [0, 0, τ<sup>3</sup>/3, 0, 0, τ<sup>2</sup>/2],<br/>
/// [τ<sup>2</sup>/2, 0, 0, τ, 0, 0],<br/>
/// [0, τ<sup>2</sup>/2, 0, 0, τ, 0],<br/>
/// [0, 0, τ<sup>2</sup>/2, 0, 0, τ]]
///
/// Note that this form of the state process update covariance has the property
/// that 2**Q**(τ) = **Q**(2τ). In other words, two successive additions of this
/// covariance will have an identical effect to a single addtion for twice the
/// time interval.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackingParams {
    /// This is used to scale the state noise covariance matrix **Q** as
    /// described at the struct-level (Kalman filter parameter).
    pub motion_noise_scale: f64,
    /// This is α in the above formula used to build the position terms in the
    /// initial estimate covariance matrix **P** as described at the
    /// struct-level (Kalman filter parameter).
    pub initial_position_std_meters: f64,
    /// This is β in the above formula used to build the velocity terms in the
    /// initial estimate covariance matrix **P** as described at the
    /// struct-level (Kalman filter parameter).
    pub initial_vel_std_meters_per_sec: f64,
    /// The observation noise covariance matrix **R** (Kalman filter
    /// parameter).
    pub ekf_observation_covariance_pixels: f64,
    /// This sets a minimum threshold for using an obervation to update an
    /// object being tracked (data association parameter).
    pub accept_observation_min_likelihood: f64,
    /// This is used to compute the maximum allowable covariance before an
    /// object is "killed" and no longer tracked.
    pub max_position_std_meters: f32,
    /// These are the hypothesis testing parameters used to "birth" a new new
    /// object and start tracking it.
    ///
    /// This is `None` if 2D (flat-3d) tracking.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hypothesis_test_params: Option<HypothesisTestParams>,
    /// This is the minimum number of observations before object becomes
    /// visible.
    #[serde(default = "default_num_observations_to_visibility")]
    pub num_observations_to_visibility: u8,
    /// Parameters defining mini arena configuration.
    ///
    /// This is MiniArenaConfig::NoMiniArena if no mini arena is in use.
    #[serde(skip_serializing_if = "MiniArenaConfig::is_none", default)]
    pub mini_arena_config: MiniArenaConfig,
}

pub struct MiniArenaLocator {
    /// The index number of the mini arena. None if the point is not in a mini arena.
    my_idx: Option<u8>,
}

impl MiniArenaLocator {
    pub fn from_mini_arena_idx(val: u8) -> Self {
        Self { my_idx: Some(val) }
    }

    pub fn new_none() -> Self {
        Self { my_idx: None }
    }

    /// Return the index number of the mini arena. None if the point is not in a
    /// mini arena.
    pub fn idx(&self) -> Option<u8> {
        self.my_idx
    }
}

/// Configuration defining potential mini arenas.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(tag = "type")]
pub enum MiniArenaConfig {
    /// No mini arena is in use.
    #[default]
    NoMiniArena,
    /// A 2D grid arranged along the X and Y axes.
    XYGrid(XYGridConfig),
}

impl MiniArenaConfig {
    pub fn is_none(&self) -> bool {
        self == &Self::NoMiniArena
    }

    pub fn iter_locators(&self) -> impl Iterator<Item = MiniArenaLocator> {
        let res = match self {
            Self::NoMiniArena => vec![MiniArenaLocator::from_mini_arena_idx(0)],
            Self::XYGrid(xy_grid_config) => {
                let sz = xy_grid_config.x_centers.0.len() * xy_grid_config.y_centers.0.len();
                (0..sz)
                    .map(|idx| MiniArenaLocator::from_mini_arena_idx(idx.try_into().unwrap()))
                    .collect()
            }
        };
        res.into_iter()
    }

    pub fn len(&self) -> usize {
        match self {
            Self::NoMiniArena => 1,
            Self::XYGrid(xy_grid_config) => {
                xy_grid_config.x_centers.0.len() * xy_grid_config.y_centers.0.len()
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Sorted(Vec<f64>);

impl Sorted {
    fn new(vals: &[f64]) -> Self {
        assert!(!vals.is_empty());
        let mut vals: Vec<NotNan<f64>> = vals.iter().map(|v| NotNan::new(*v).unwrap()).collect();
        vals.sort();
        let vals = vals.iter().map(|v| v.into_inner()).collect();
        Sorted(vals)
    }
    fn dist_and_argmin(&self, x: f64) -> (f64, usize) {
        let mut best_dist = f64::INFINITY;
        let mut prev_dist = f64::INFINITY;
        let mut best_idx = 0;
        for (i, selfi) in self.0.iter().enumerate() {
            let dist = (selfi - x).abs();
            if dist < best_dist {
                best_dist = dist;
                best_idx = i;
            }
            if dist > prev_dist {
                // short circuit end of loop
                break;
            }
            prev_dist = dist
        }
        (best_dist, best_idx)
    }
}

#[test]
fn test_sorted() {
    let x = Sorted::new(&[1.0, 2.0, 1.0]);

    assert_eq!(x.0, vec![1.0, 1.0, 2.0]);
    assert_eq!(x.dist_and_argmin(1.1).1, 0);

    let x = Sorted::new(&[1.0, 2.0, 1.0, 3.0, 4.0]);
    assert_eq!(x.dist_and_argmin(2.1).1, 2);

    assert_eq!(x.dist_and_argmin(1.9).1, 2);
}

/// Parameters defining a 2D grid of mini arenas arranged along X and Y axes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct XYGridConfig {
    x_centers: Sorted,
    y_centers: Sorted,
    radius: f64,
}

impl XYGridConfig {
    pub fn new(x: &[f64], y: &[f64], radius: f64) -> Self {
        Self {
            x_centers: Sorted::new(x),
            y_centers: Sorted::new(y),
            radius,
        }
    }

    pub fn iter_centers(&self) -> impl Iterator<Item = (f64, f64)> {
        XYGridIter {
            col_centers: self.x_centers.0.clone(),
            row_centers: self.y_centers.0.clone(),
            next_idx: 0,
        }
    }

    pub fn get_arena_index(&self, coords: &[MyFloat; 3]) -> MiniArenaLocator {
        if coords[2] != 0.0 {
            return MiniArenaLocator::new_none();
        }
        let obj_x = coords[0];
        let obj_y = coords[1];

        let (dist_x, idx_x) = self.x_centers.dist_and_argmin(obj_x);
        let (dist_y, idx_y) = self.y_centers.dist_and_argmin(obj_y);

        let dist = (dist_x * dist_x + dist_y * dist_y).sqrt();
        if dist <= self.radius {
            let idx = (idx_y * self.x_centers.0.len() + idx_x).try_into().unwrap();
            MiniArenaLocator::from_mini_arena_idx(idx)
        } else {
            MiniArenaLocator::new_none()
        }
    }
}

struct XYGridIter {
    row_centers: Vec<f64>,
    col_centers: Vec<f64>,
    next_idx: usize,
}

impl Iterator for XYGridIter {
    type Item = (f64, f64);
    fn next(&mut self) -> Option<Self::Item> {
        let (row_idx, col_idx) = num_integer::div_rem(self.next_idx, self.col_centers.len());
        if row_idx >= self.row_centers.len() {
            None
        } else {
            let result = (self.col_centers[col_idx], self.row_centers[row_idx]);
            self.next_idx += 1;
            Some(result)
        }
    }
}

fn default_num_observations_to_visibility() -> u8 {
    // This number should suppress spurious trajectory births but not wait too
    // long before notifying listeners.
    3
}

pub type MyFloat = f64;

pub fn default_tracking_params_full_3d() -> TrackingParams {
    TrackingParams {
        motion_noise_scale: 0.1,
        initial_position_std_meters: 0.1,
        initial_vel_std_meters_per_sec: 1.0,
        accept_observation_min_likelihood: 1e-8,
        ekf_observation_covariance_pixels: 1.0,
        max_position_std_meters: 0.01212,
        hypothesis_test_params: Some(make_hypothesis_test_full3d_default()),
        num_observations_to_visibility: default_num_observations_to_visibility(),
        mini_arena_config: MiniArenaConfig::NoMiniArena,
    }
}

pub fn default_tracking_params_flat_3d() -> TrackingParams {
    TrackingParams {
        motion_noise_scale: 0.0005,
        initial_position_std_meters: 0.001,
        initial_vel_std_meters_per_sec: 0.02,
        accept_observation_min_likelihood: 0.00001,
        ekf_observation_covariance_pixels: 1.0,
        max_position_std_meters: 0.003,
        hypothesis_test_params: None,
        num_observations_to_visibility: 10,
        mini_arena_config: MiniArenaConfig::NoMiniArena,
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
    pub name: RawCamName,
    pub state: ConnectedCameraSyncState,
    pub strand_cam_http_server_info: BuiServerInfo,
    pub recent_stats: RecentStats,
}

/// Messages to Braid
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum BraidHttpApiCallback {
    /// Called from strand-cam to register a camera
    ///
    /// Note this is different than the `cam_info_handler` which only queries
    /// for the appropriate camera configuration.
    NewCamera(RegisterNewCamera),
    /// Called from strand-cam to update the current image
    UpdateCurrentImage(PerCam<UpdateImage>),
    /// Called from strand-cam to update the current camera settings (e.g.
    /// exposure time)
    UpdateCamSettings(PerCam<UpdateCamSettings>),
    /// Called from strand-cam to update the current feature detection settings
    /// (e.g. threshold different)
    UpdateFeatureDetectSettings(PerCam<UpdateFeatureDetectSettings>),
    /// Start or stop recording data (.braid directory with csv tables for later
    /// .braidz file)
    DoRecordCsvTables(bool),
    /// Start or stop recording MKV videos for all cameras
    DoRecordMp4Files(bool),
    /// set uuid in the experiment_info table
    SetExperimentUuid(String),
    /// Set the number of frames to buffer in each camera
    SetPostTriggerBufferSize(usize),
    /// Initiate MKV recording using post trigger
    PostTriggerMp4Recording,
}

#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub struct PerCam<T> {
    pub raw_cam_name: RawCamName,
    pub inner: T,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct FlydraRawUdpPacket {
    /// The name of the camera
    ///
    /// Traditionally this was the ROS camera name (e.g. with '-' converted to
    /// '_'), but have transitioned to allowing any valid UTF-8 string.
    pub cam_name: String,
    /// frame timestamp of trigger pulse start (or None if cannot be determined)
    #[serde(with = "crate::timestamp_opt_f64")]
    pub timestamp: Option<FlydraFloatTimestampLocal<Triggerbox>>,
    /// frame timestamp of camnode program sampling system clock
    #[serde(with = "crate::timestamp_f64")]
    pub cam_received_time: FlydraFloatTimestampLocal<HostClock>,
    /// timestamp from the camera
    pub device_timestamp: Option<u64>,
    /// frame number from the camera
    pub block_id: Option<u64>,
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

mod synced_frame;
pub use synced_frame::SyncFno;

mod cam_num;
pub use cam_num::CamNum;

mod timestamp;
pub use crate::timestamp::{
    triggerbox_time, FlydraFloatTimestampLocal, HostClock, Source, Triggerbox,
};

pub mod timestamp_f64;
pub mod timestamp_opt_f64;

#[cfg(feature = "with-tokio-codec")]
mod tokio_cbor;
#[cfg(feature = "with-tokio-codec")]
pub use crate::tokio_cbor::CborPacketCodec;

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
    #[error("URL parse error")]
    UrlParseError,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TriggerClockInfoRow {
    // changes to this should update BraidMetadataSchemaTag
    #[serde(with = "crate::timestamp_f64")]
    pub start_timestamp: FlydraFloatTimestampLocal<HostClock>,
    pub framecount: i64,
    /// Fraction of full framecount is tcnt/255
    pub tcnt: u8,
    #[serde(with = "crate::timestamp_f64")]
    pub stop_timestamp: FlydraFloatTimestampLocal<HostClock>,
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

/// TriggerboxV1 configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TriggerboxConfig {
    pub device_fname: String,
    pub framerate: f32,
    #[serde(default = "default_query_dt")]
    pub query_dt: std::time::Duration,
    pub max_triggerbox_measurement_error: Option<std::time::Duration>,
}

impl std::default::Default for TriggerboxConfig {
    fn default() -> Self {
        Self {
            device_fname: "/dev/trig1".to_string(),
            framerate: 100.0,
            query_dt: default_query_dt(),
            // Make a relatively long default so that cameras will synchronize
            // even with relatively long delays. Users can always specify
            // tighter precision within a config file.
            max_triggerbox_measurement_error: Some(std::time::Duration::from_millis(20)),
        }
    }
}

const fn default_query_dt() -> std::time::Duration {
    std::time::Duration::from_millis(1500)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PtpSyncConfig {
    /// The period of the periodic signal.
    ///
    /// If this is set, it is transmitted to the cameras.
    pub periodic_signal_period_usec: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FakeSyncConfig {
    pub framerate: f64,
}

impl Default for FakeSyncConfig {
    fn default() -> Self {
        Self { framerate: 95.0 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
#[serde(tag = "trigger_type")]
pub enum TriggerType {
    /// Cameras are synchronized via hardware triggers controlled
    /// via a [Straw Lab triggerbox](https://github.com/strawlab/triggerbox).
    TriggerboxV1(TriggerboxConfig),
    /// Cameras are synchronized using PTP (Precision Time Protocol, IEEE 1588).
    PtpSync(PtpSyncConfig),
    DeviceTimestamp,
    /// Cameras are not synchronized, but we pretend they are.
    FakeSync(FakeSyncConfig),
}

impl Default for TriggerType {
    fn default() -> Self {
        TriggerType::FakeSync(FakeSyncConfig::default())
    }
}

/// Feature detection data in raw camera coordinates.
///
/// Because these are in raw camera coordinates (and thus have not been
/// undistorted with any lens distortion model), they are called "distorted".
///
/// Note that in `.braidz` files, subsequent rows on disk are not in general
/// monotonically increasing in frame number.
///
/// See the "Details about how data are processed online and saved for later
/// analysis" section in the "3D Tracking in Braid" chapter of the [User's
/// Guide](https://strawlab.github.io/strand-braid/) for a description of why
/// these cannot be relied upon in `.braidz` files to be monotonic.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Data2dDistortedRow {
    // changes to this should update BraidMetadataSchemaTag
    // should be kept in sync with Data2dDistortedRowF32
    /// The number of the camera.
    pub camn: CamNum,
    /// The synchronized frame number.
    ///
    /// This is very likely to be different than [Self::block_id], the camera's
    /// internal frame number, because Braid synchronizes the frames so that,
    /// e.g. "frame 10" occurred at the same instant across all cameras.
    pub frame: i64,
    /// This is the trigger timestamp (if available).
    #[serde(with = "crate::timestamp_opt_f64")]
    pub timestamp: Option<FlydraFloatTimestampLocal<Triggerbox>>,
    #[serde(with = "crate::timestamp_f64")]
    pub cam_received_timestamp: FlydraFloatTimestampLocal<HostClock>,
    /// Timestamp from the camera.
    pub device_timestamp: Option<u64>,
    /// Frame number from the camera.
    ///
    /// Note that this is not the synchronized frame number, which is [Self::frame].
    pub block_id: Option<u64>,
    /// The X (horizontal) coordinate of the detection, in camera pixels.
    #[serde(deserialize_with = "invalid_nan")]
    pub x: f64,
    /// The Y (vertical) coordinate of the detection, in camera pixels.
    #[serde(deserialize_with = "invalid_nan")]
    pub y: f64,
    /// The area of the detection, in camera pixels^2.
    #[serde(deserialize_with = "invalid_nan")]
    pub area: f64,
    /// The slope of the detection.
    ///
    /// The orientation, modulo 𝜋, of the detection, is `atan(slope)`.
    #[serde(deserialize_with = "invalid_nan")]
    pub slope: f64,
    /// The eccentricity of the detection.
    #[serde(deserialize_with = "invalid_nan")]
    pub eccentricity: f64,
    /// The index of this particular detection within a given frame.
    ///
    /// Multiple detections can occur within a single frame, and each succesive
    /// detection will have a higher index.
    pub frame_pt_idx: u8,
    pub cur_val: u8,
    #[serde(deserialize_with = "invalid_nan")]
    pub mean_val: f64,
    #[serde(deserialize_with = "invalid_nan")]
    pub sumsqf_val: f64,
}

/// Lower precision version of [Data2dDistortedRow] for saving to disk.
// Note that this matches the precision specified in the old flydra Python
// module `flydra_core.data_descriptions.Info2D`.
#[derive(Debug, Serialize)]
pub struct Data2dDistortedRowF32 {
    // changes to this should update BraidMetadataSchemaTag
    /// The number of the camera.
    pub camn: CamNum,
    /// The synchronized frame number.
    ///
    /// This is very likely to be different than [Self::block_id], the camera's
    /// internal frame number, because Braid synchronizes the frames so that,
    /// e.g. "frame 10" occurred at the same instant across all cameras.
    pub frame: i64,
    /// This is the trigger timestamp (if available).
    #[serde(with = "crate::timestamp_opt_f64")]
    pub timestamp: Option<FlydraFloatTimestampLocal<Triggerbox>>,
    #[serde(with = "crate::timestamp_f64")]
    pub cam_received_timestamp: FlydraFloatTimestampLocal<HostClock>,
    /// timestamp from the camera
    pub device_timestamp: Option<u64>,
    /// Frame number from the camera.
    ///
    /// Note that this is not the synchronized frame number, which is [Self::frame].
    pub block_id: Option<u64>,
    /// The X (horizontal) coordinate of the detection, in camera pixels.
    pub x: f32,
    /// The Y (vertial) coordinate of the detection, in camera pixels.
    pub y: f32,
    /// The area of the detection, in camera pixels^2.
    pub area: f32,
    /// The slope of the detection.
    ///
    /// The orientation, modulo 𝜋, of the detection, is `atan(slope)`.
    pub slope: f32,
    /// The eccentricity of the detection.
    pub eccentricity: f32,
    /// The index of this particular detection within a given frame.
    ///
    /// Multiple detections can occur within a single frame, and each succesive
    /// detection will have a higher index.
    pub frame_pt_idx: u8,
    pub cur_val: u8,
    pub mean_val: f32,
    pub sumsqf_val: f32,
}

impl From<Data2dDistortedRow> for Data2dDistortedRowF32 {
    fn from(orig: Data2dDistortedRow) -> Self {
        Self {
            camn: orig.camn,
            frame: orig.frame,
            timestamp: orig.timestamp,
            cam_received_timestamp: orig.cam_received_timestamp,
            device_timestamp: orig.device_timestamp,
            block_id: orig.block_id,
            x: orig.x as f32,
            y: orig.y as f32,
            area: orig.area as f32,
            slope: orig.slope as f32,
            eccentricity: orig.eccentricity as f32,
            frame_pt_idx: orig.frame_pt_idx,
            cur_val: orig.cur_val,
            mean_val: orig.mean_val as f32,
            sumsqf_val: orig.sumsqf_val as f32,
        }
    }
}

impl WithKey<i64> for Data2dDistortedRow {
    fn key(&self) -> i64 {
        self.frame
    }
}

fn invalid_nan<'de, D>(de: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    f64::deserialize(de).or(
        // TODO: should match on DeserializeError with empty field only,
        // otherwise, return error. The way this is written, anything
        // will return a nan.
        Ok(f64::NAN),
    )
}

pub const BRAID_EVENTS_URL_PATH: &str = "braid-events";
pub const BRAID_EVENT_NAME: &str = "braid";
