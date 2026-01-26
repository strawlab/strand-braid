// TODO: if camera not available, launch alternate UI indicating such and
// waiting for it to become available?

// TODO: add quit app button to UI.

// TODO: UI automatically reconnect to app after app restart.

use async_change_tracker::ChangeTracker;
use event_stream_types::{
    AcceptsEventStream, ConnectionEvent, ConnectionEventType, ConnectionSessionKey,
    EventBroadcaster, TolerantJson,
};
use futures::{sink::SinkExt, stream::StreamExt};
use http::StatusCode;
use strand_http_video_streaming as video_streaming;

use hyper_rustls::HttpsConnector;

use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use machine_vision_formats as formats;
#[allow(unused_imports)]
use preferences_serde1::{AppInfo, Preferences};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::error::SendError;
use tower_http::trace::TraceLayer;
use tracing::{debug, error, info, trace, warn};

use ci2::{Camera, CameraInfo, CameraModule, DynamicFrameWithInfo};
use ci2_async::AsyncCamera;
use fmf::FMFWriter;
use formats::PixFmt;
use strand_bui_backend_session_types::{AccessToken, BuiServerAddrInfo, ConnectionKey};
use strand_dynamic_frame::DynamicFrame;

use video_streaming::AnnotatedFrame;

use std::{path::PathBuf, result::Result as StdResult};

#[cfg(feature = "checkercal")]
use std::fs::File;

#[cfg(feature = "flydra_feat_detect")]
use strand_cam_remote_control::CsvSaveConfig;
use strand_cam_remote_control::{
    CamArg, CodecSelection, FfmpegRecordingConfig, Mp4Codec, Mp4RecordingConfig, NvidiaH264Options,
    RecordingFrameRate,
};

use braid_types::{BuiServerInfo, RawCamName, StartSoftwareFrameRateLimit, TriggerType};

use flydra_feature_detector_types::ImPtDetectCfg;

#[cfg(feature = "flydra_feat_detect")]
use strand_cam_csv_config_types::CameraCfgFview2_0_26;

#[cfg(feature = "fiducial")]
use strand_cam_storetype::ApriltagState;
use strand_cam_storetype::{
    CallbackType, ImOpsState, RangedValue, StoreType, ToLedBoxDevice, STRAND_CAM_EVENT_NAME,
};

use strand_cam_bui_types::RecordingPath;
use strand_cam_storetype::{KalmanTrackingConfig, LedProgramConfig};

use std::{
    io::Write,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket},
    sync::{Arc, RwLock},
};

pub const APP_INFO: AppInfo = AppInfo {
    name: "strand-cam",
    author: "AndrewStraw",
};

#[cfg(feature = "imtrack-absdiff")]
pub use flydra_pt_detect_cfg::default_absdiff as default_im_pt_detect;
#[cfg(feature = "imtrack-dark-circle")]
pub use flydra_pt_detect_cfg::default_dark_circle as default_im_pt_detect;

#[cfg(feature = "bundle_files")]
static ASSETS_DIR: include_dir::Dir<'static> =
    include_dir::include_dir!("$CARGO_MANIFEST_DIR/yew_frontend/dist");

#[cfg(feature = "flydratrax")]
const KALMAN_TRACKING_PREFS_KEY: &'static str = "kalman-tracking";

#[cfg(feature = "flydratrax")]
const LED_PROGRAM_PREFS_KEY: &'static str = "led-config";

const COOKIE_SECRET_KEY: &str = "cookie-secret-base64";
const BRAID_COOKIE_KEY: &str = "braid-cookie";

#[cfg(feature = "flydratrax")]
mod flydratrax_handle_msg;

mod clock_model;
mod datagram_socket;
mod post_trigger_buffer;

#[cfg(feature = "eframe-gui")]
mod gui_app;

mod frame_process_task;
use frame_process_task::frame_process_task;

#[cfg(feature = "eframe-gui")]
#[derive(Default)]
struct GuiShared {
    ctx: Option<eframe::egui::Context>,
    url: Option<String>,
}

#[cfg(feature = "eframe-gui")]
type ArcMutGuiSingleton = Arc<std::sync::Mutex<GuiShared>>;

#[cfg(not(feature = "eframe-gui"))]
type ArcMutGuiSingleton = ();

pub mod cli_app;

const LED_BOX_HEARTBEAT_INTERVAL_MSEC: u64 = 5000;

use eyre::{eyre, Result, WrapErr};

pub(crate) enum Msg {
    StartMp4,
    StopMp4,
    StartFMF((String, RecordingFrameRate)),
    StopFMF,
    #[cfg(feature = "flydra_feat_detect")]
    StartUFMF(String),
    #[cfg(feature = "flydra_feat_detect")]
    StopUFMF,
    #[cfg(feature = "flydra_feat_detect")]
    SetTracking(bool),
    PostTriggerStartMp4,
    SetPostTriggerBufferSize(usize),
    Mframe(DynamicFrameWithInfo),
    #[cfg(feature = "flydra_feat_detect")]
    SetIsSavingObjDetectionCsv(CsvSaveConfig),
    #[cfg(feature = "flydra_feat_detect")]
    SetExpConfig(ImPtDetectCfg),
    Store(Arc<RwLock<ChangeTracker<StoreType>>>),
    #[cfg(feature = "flydra_feat_detect")]
    TakeCurrentImageAsBackground,
    #[cfg(feature = "flydra_feat_detect")]
    ClearBackground(f32),
    SetFrameOffset(u64),
    SetTriggerboxClockModel(Option<strand_cam_bui_types::ClockModel>),
    StartAprilTagRec(String),
    StopAprilTagRec,
}

impl std::fmt::Debug for Msg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> StdResult<(), std::fmt::Error> {
        write!(f, "strand_cam::Msg{{..}}")
    }
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, Default)]
pub enum FrameProcessingErrorState {
    #[default]
    NotifyAll,
    IgnoreUntil(chrono::DateTime<chrono::Utc>),
    IgnoreAll,
}

#[cfg(feature = "flydra_feat_detect")]
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub enum Tracker {
    NoTracker,
    BackgroundSubtraction(ImPtDetectCfg),
}

/// calculates a framerate every n frames
pub struct FpsCalc {
    prev: Option<(usize, chrono::DateTime<chrono::Utc>)>,
    frames_to_average: usize,
}

impl FpsCalc {
    /// create a new FpsCalc instance
    pub fn new(frames_to_average: usize) -> Self {
        Self {
            prev: None,
            frames_to_average,
        }
    }
    /// return a newly computed fps value whenever available.
    pub fn update(&mut self, fi: &ci2::HostTimingInfo) -> Option<f64> {
        let fno = fi.fno;
        let stamp = fi.datetime;
        let mut reset_previous = true;
        let mut result = None;
        if let Some((prev_frame, ref prev_stamp)) = self.prev {
            let n_frames = fno - prev_frame;
            if n_frames < self.frames_to_average {
                reset_previous = false;
            } else {
                let dur_nsec = stamp.signed_duration_since(*prev_stamp).num_nanoseconds();
                if let Some(nsec) = dur_nsec {
                    result = Some(n_frames as f64 / nsec as f64 * 1.0e9);
                }
            }
        }
        if reset_previous {
            self.prev = Some((fno, stamp));
        }
        result
    }
}

struct FmfWriteInfo<T>
where
    T: std::io::Write + std::io::Seek,
{
    writer: FMFWriter<T>,
    recording_framerate: RecordingFrameRate,
    last_saved_stamp: Option<chrono::DateTime<chrono::Utc>>,
}

impl<T> FmfWriteInfo<T>
where
    T: std::io::Write + std::io::Seek,
{
    fn new(writer: FMFWriter<T>, recording_framerate: RecordingFrameRate) -> Self {
        Self {
            writer,
            recording_framerate,
            last_saved_stamp: None,
        }
    }
}

#[cfg(feature = "checkercal")]
type CollectedCornersArc = Arc<RwLock<Vec<Vec<(f32, f32)>>>>;

async fn convert_stream(
    raw_cam_name: RawCamName,
    mut transmit_feature_detect_settings_rx: tokio::sync::mpsc::Receiver<
        flydra_feature_detector_types::ImPtDetectCfg,
    >,
    transmit_msg_tx: tokio::sync::mpsc::Sender<braid_types::BraidHttpApiCallback>,
) -> Result<()> {
    while let Some(val) = transmit_feature_detect_settings_rx.recv().await {
        let msg =
            braid_types::BraidHttpApiCallback::UpdateFeatureDetectSettings(braid_types::PerCam {
                raw_cam_name: raw_cam_name.clone(),
                inner: braid_types::UpdateFeatureDetectSettings {
                    current_feature_detect_settings: val,
                },
            });
        transmit_msg_tx.send(msg).await?;
    }
    Ok(())
}

fn open_braid_destination_addr(camdata_udp_addr: &SocketAddr) -> Result<UdpSocket> {
    info!(
        "Sending detected coordinates via UDP to: {}",
        camdata_udp_addr
    );

    let timeout = std::time::Duration::new(0, 1);

    let src_ip = if !camdata_udp_addr.ip().is_loopback() {
        // Let OS choose what IP to use, but preserve V4 or V6.
        match camdata_udp_addr {
            SocketAddr::V4(_) => IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
            SocketAddr::V6(_) => IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 0)),
        }
    } else {
        match camdata_udp_addr {
            SocketAddr::V4(_) => IpAddr::V4(Ipv4Addr::LOCALHOST),
            SocketAddr::V6(_) => IpAddr::V6(Ipv6Addr::LOCALHOST),
        }
    };
    // Let OS choose what port to use.
    let src_addr = SocketAddr::new(src_ip, 0);

    let sock = UdpSocket::bind(src_addr)?;
    sock.set_write_timeout(Some(timeout))?;
    sock.connect(camdata_udp_addr)?;
    Ok(sock)
}

#[cfg(feature = "flydra_feat_detect")]
fn get_intensity(device_state: &strand_led_box_comms::DeviceState, chan_num: u8) -> u16 {
    let ch: &strand_led_box_comms::ChannelState = match chan_num {
        1 => &device_state.ch1,
        2 => &device_state.ch2,
        3 => &device_state.ch3,
        c => panic!("unknown channel {c}"),
    };
    match ch.on_state {
        strand_led_box_comms::OnState::Off => 0,
        strand_led_box_comms::OnState::ConstantOn => ch.intensity,
    }
}

/// Ignore a send error.
///
/// During shutdown, the receiver can disappear before the sender is closed.
/// According to the docs of the `send` method of [tokio::sync::mpsc::Sender],
/// this is the only way we can get a [tokio::sync::mpsc::error::SendError].
/// Therefore, we ignore the error.
trait IgnoreSendError {
    fn ignore_send_error(self);
}

impl<T: std::fmt::Debug> IgnoreSendError for StdResult<(), tokio::sync::mpsc::error::SendError<T>> {
    fn ignore_send_error(self) {
        match self {
            Ok(()) => {}
            Err(e) => {
                debug!("Ignoring send error ({}:{}): {:?}", file!(), line!(), e)
            }
        }
    }
}

#[derive(Clone)]
struct StrandCamCallbackSenders {
    firehose_callback_tx: tokio::sync::mpsc::Sender<ConnectionKey>,
    cam_args_tx: tokio::sync::mpsc::Sender<CamArg>,
    led_box_tx_std: tokio::sync::mpsc::Sender<ToLedBoxDevice>,
    #[allow(dead_code)]
    tx_frame: tokio::sync::mpsc::Sender<Msg>,
}

#[derive(Clone)]
struct StrandCamAppState {
    cam_name: String,
    event_broadcaster: EventBroadcaster<ConnectionSessionKey>,
    callback_senders: StrandCamCallbackSenders,
    tx_new_connection: tokio::sync::mpsc::Sender<event_stream_types::ConnectionEvent>,
    shared_store_arc: Arc<RwLock<ChangeTracker<StoreType>>>,
}

type MyBody = http_body_util::combinators::BoxBody<bytes::Bytes, strand_bui_backend_session::Error>;

fn body_from_buf(body_buf: &[u8]) -> MyBody {
    let body = http_body_util::Full::new(bytes::Bytes::from(body_buf.to_vec()));
    use http_body_util::BodyExt;
    MyBody::new(body.map_err(|_: std::convert::Infallible| unreachable!()))
}

async fn check_version(
    client: Client<
        HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>,
        MyBody,
        // http_body_util::Empty<bytes::Bytes>,
    >,
    known_version: Arc<RwLock<semver::Version>>,
    app_name: &'static str,
) -> Result<()> {
    let url = format!("https://version-check.strawlab.org/{app_name}");
    let url = url.parse::<hyper::Uri>().unwrap();
    let agent = format!("{}/{}", app_name, *known_version.read().unwrap());

    let req = hyper::Request::builder()
        .uri(&url)
        .header(hyper::header::USER_AGENT, agent.as_str())
        .body(body_from_buf(b""))
        .unwrap();

    #[derive(Debug, Deserialize, PartialEq, Clone)]
    struct VersionResponse {
        available: semver::Version,
        message: String,
    }

    let known_version2 = known_version.clone();

    let res = client.request(req).await?;

    if res.status() != hyper::StatusCode::OK {
        // should return error?
        return Ok(());
    }

    let known_version3 = known_version2.clone();

    let body = res.into_body();
    let chunks: StdResult<http_body_util::Collected<bytes::Bytes>, _> = {
        use http_body_util::BodyExt;
        body.collect().await
    };
    let data = chunks?.to_bytes();

    let version: VersionResponse = match serde_json::from_slice(&data) {
        Ok(version) => version,
        Err(e) => {
            warn!("Could not parse version response JSON from {}: {}", url, e);
            return Ok(());
        }
    };
    let mut known_v = known_version3.write().unwrap();
    if version.available > *known_v {
        info!(
            "New version of {} is available: {}. {}",
            app_name, version.available, version.message
        );
        *known_v = version.available;
    }

    Ok(())
}

fn display_qr_url(url: &str) -> Result<()> {
    use qrcode::render::unicode;
    use qrcode::QrCode;
    use std::io::stdout;

    let qr = QrCode::new(url)?;

    let image = qr.render::<unicode::Dense1x2>().build();

    let stdout = stdout();
    let mut stdout_handle = stdout.lock();
    writeln!(stdout_handle)?;
    stdout_handle.write_all(image.as_bytes())?;
    writeln!(stdout_handle)?;
    Ok(())
}

#[derive(Debug, Clone)]
/// Defines whether runtime changes from the user are persisted to disk.
///
/// If they are persisted to disk, upon program re-start, the disk
/// is checked and preferences are loaded from there. If they cannot
/// be loaded, the defaults are used.
pub enum ImPtDetectCfgSource {
    ChangesNotSavedToDisk(ImPtDetectCfg),
    ChangedSavedToDisk((&'static AppInfo, String)),
}

#[cfg(feature = "flydra_feat_detect")]
impl Default for ImPtDetectCfgSource {
    fn default() -> Self {
        ImPtDetectCfgSource::ChangesNotSavedToDisk(default_im_pt_detect())
    }
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, Clone)]
pub enum TimestampSource {
    BraidTrigger, // TODO: rename to CleverComputation or similar
    HostAcquiredTimestamp,
}

const MOMENT_CENTROID_SCHEMA_VERSION: u8 = 2;

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
pub struct MomentCentroid {
    pub schema_version: u8,
    pub framenumber: u64,
    pub timestamp_source: TimestampSource,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub mu00: f32,
    pub mu01: f32,
    pub mu10: f32,
    pub center_x: u32,
    pub center_y: u32,
    #[serde(default)]
    pub cam_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
enum CentroidToDevice {
    Centroid(MomentCentroid),
}

/// CLI args for the case when we will connect to Braid.
///
/// Prior to the connection, we don't know much about what our configuration
/// should be. This is very limited because most configuration should be done in
/// the Braid configuration .toml file.
#[derive(Debug, Default, Clone)]
pub struct BraidArgs {
    pub braid_url: String,
    pub camera_name: String,
}

/// CLI args for the case when we run standalone.
#[derive(Debug, Clone, Default)]
pub struct StandaloneArgs {
    pub camera_name: Option<String>,
    /// The HTTP socket address for the Strand Cam BUI.
    pub http_server_addr: Option<String>,
    pub pixel_format: Option<String>,
    /// If set, camera acquisition will external trigger.
    pub force_camera_sync_mode: bool,
    /// If enabled, limit framerate (FPS) at startup.
    ///
    /// Despite the name ("software"), this actually sets the hardware
    /// acquisition rate via the `AcquisitionFrameRate` camera parameter.
    pub software_limit_framerate: StartSoftwareFrameRateLimit,
    /// Threshold duration before logging error (msec).
    ///
    /// If the image acquisition timestamp precedes the computed trigger
    /// timestamp, clearly an error has happened. This error must lie in the
    /// computation of the trigger timestamp. This specifies the threshold error
    /// at which an error is logged. (The underlying source of such errors
    /// remains unknown.)
    pub acquisition_duration_allowed_imprecision_msec: Option<f64>,
    /// Filename of vendor-specific camera settings file.
    pub camera_settings_filename: Option<std::path::PathBuf>,
    #[cfg(feature = "flydra_feat_detect")]
    pub tracker_cfg_src: ImPtDetectCfgSource,
}

#[derive(Debug)]
pub enum StandaloneOrBraid {
    Standalone(StandaloneArgs),
    Braid(BraidArgs),
}

impl Default for StandaloneOrBraid {
    fn default() -> Self {
        Self::Standalone(Default::default())
    }
}

#[derive(Debug)]
pub struct StrandCamArgs {
    /// Is Strand Cam running inside Braid context?
    pub standalone_or_braid: StandaloneOrBraid,
    /// base64 encoded secret. minimum 256 bits.
    pub secret: Option<String>,
    pub no_browser: bool,
    pub mp4_filename_template: String,
    pub fmf_filename_template: String,
    pub ufmf_filename_template: String,
    pub disable_console: bool,
    pub csv_save_dir: String,
    pub led_box_device_path: Option<String>,
    #[cfg(feature = "flydratrax")]
    pub save_empty_data2d: SaveEmptyData2dType,
    #[cfg(feature = "flydratrax")]
    pub model_server_addr: std::net::SocketAddr,
    #[cfg(feature = "flydratrax")]
    pub flydratrax_calibration_source: CalSource,
    #[cfg(feature = "fiducial")]
    pub apriltag_csv_filename_template: String,
    #[cfg(feature = "flydratrax")]
    pub write_buffer_size_num_messages: usize,
    #[cfg(target_os = "linux")]
    v4l2loopback: Option<PathBuf>,
    data_dir: Option<PathBuf>,
}

pub type SaveEmptyData2dType = bool;

#[derive(Debug)]
pub enum CalSource {
    /// Use circular tracking region to create calibration
    PseudoCal,
    /// Use flydra .xml file with single camera for calibration
    XmlFile(std::path::PathBuf),
    /// Use pymvg .json file with single camera for calibration
    PymvgJsonFile(std::path::PathBuf),
}

impl Default for StrandCamArgs {
    fn default() -> Self {
        Self {
            standalone_or_braid: Default::default(),
            secret: None,
            no_browser: true,
            mp4_filename_template: "movie%Y%m%d_%H%M%S.%f_{CAMNAME}.mp4".to_string(),
            fmf_filename_template: "movie%Y%m%d_%H%M%S.%f_{CAMNAME}.fmf".to_string(),
            ufmf_filename_template: "movie%Y%m%d_%H%M%S.%f_{CAMNAME}.ufmf".to_string(),
            disable_console: false,
            #[cfg(feature = "fiducial")]
            apriltag_csv_filename_template: strand_cam_storetype::APRILTAG_CSV_TEMPLATE_DEFAULT
                .to_string(),
            csv_save_dir: "/dev/null".to_string(),
            led_box_device_path: None,
            #[cfg(feature = "flydratrax")]
            flydratrax_calibration_source: CalSource::PseudoCal,
            #[cfg(feature = "flydratrax")]
            save_empty_data2d: true,
            #[cfg(feature = "flydratrax")]
            model_server_addr: braid_types::DEFAULT_MODEL_SERVER_ADDR.parse().unwrap(),
            #[cfg(feature = "flydratrax")]
            write_buffer_size_num_messages:
                braid_config_data::default_write_buffer_size_num_messages(),
            #[cfg(target_os = "linux")]
            v4l2loopback: None,
            data_dir: Default::default(),
        }
    }
}

fn test_nvenc_save(frame: DynamicFrame) -> Result<bool> {
    let cfg = Mp4RecordingConfig {
        codec: Mp4Codec::H264NvEnc(NvidiaH264Options {
            bitrate: None,
            cuda_device: 0,
        }),
        h264_metadata: None,
        max_framerate: RecordingFrameRate::Fps30,
    };
    let mut nv_cfg_test = cfg.clone();

    let libs = match nvenc::Dynlibs::new() {
        Ok(libs) => libs,
        Err(e) => {
            debug!("nvidia NvEnc library could not be loaded: {:?}", e);
            return Ok(false);
        }
    };

    let opts = NvidiaH264Options {
        bitrate: None,
        ..Default::default()
    };

    nv_cfg_test.codec = strand_cam_remote_control::Mp4Codec::H264NvEnc(opts);

    // Temporary variable to hold file data. This will be dropped
    // at end of scope.
    let mut buf = std::io::Cursor::new(Vec::new());

    let nv_enc = match nvenc::NvEnc::new(&libs) {
        Ok(nv_enc) => nv_enc,
        Err(e) => {
            debug!("nvidia NvEnc could not be initialized: {:?}", e);
            return Ok(false);
        }
    };

    let mut mp4_writer = mp4_writer::Mp4Writer::new(&mut buf, nv_cfg_test, Some(nv_enc))?;
    match mp4_writer.write_dynamic(&frame, chrono::Local::now()) {
        Ok(()) => {}
        Err(e) => {
            debug!("nvidia NvEnc could not be initialized: {:?}", e);
            return Ok(false);
        }
    }
    mp4_writer.finish()?;

    debug!("MP4 video with nvenc h264 encoding succeeded.");

    // When `buf` goes out of scope, it will be dropped.
    Ok(true)
}

fn to_event_chunk(state: &StoreType) -> String {
    let buf = serde_json::to_string(&state).unwrap();
    format!("event: {STRAND_CAM_EVENT_NAME}\ndata: {buf}\n\n")
}

/// Handle a new connection to the event stream.
///
/// This creates a new channel which sends events to the new connection. The
/// receiver side is simply the http body passed to axum. The sender side is
/// initially started with a couple messages and then is ultimately sent to a
/// "global event sender" which will send ongoing events to all connections.
async fn events_handler(
    axum::extract::State(app_state): axum::extract::State<StrandCamAppState>,
    session_key: axum_token_auth::SessionKey,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<SocketAddr>,
    _: AcceptsEventStream,
) -> impl axum::response::IntoResponse {
    session_key.is_present();
    tracing::trace!("events");
    // Connection wants to subscribe to event stream.

    let key = ConnectionSessionKey::new(session_key.0, addr);

    // Create a new channel in which the receiver is used to send responses to
    // the new connection. The sender is sent to the app where it is stored in a
    // per-connection map. Changes for this connection will then be sent to the
    // stored sender.
    let (conn_tx, body) = app_state.event_broadcaster.new_connection(key);

    // Send the first message, the connection key.
    {
        let chunk = format!(
            "event: {}\ndata: {}\n\n",
            strand_cam_storetype::CONN_KEY_EVENT_NAME,
            addr
        );
        match conn_tx.send(http_body::Frame::data(chunk.into())).await {
            Ok(()) => {}
            Err(tokio::sync::mpsc::error::SendError(_)) => {
                // The receiver was dropped because the connection closed. Should probably do more here.
                tracing::debug!("initial send error");
            }
        }
    }

    // Send the second message, a copy of our state.
    {
        let shared_store = app_state.shared_store_arc.read().unwrap().as_ref().clone();
        let chunk = to_event_chunk(&shared_store);
        match conn_tx.send(http_body::Frame::data(chunk.into())).await {
            Ok(()) => {}
            Err(tokio::sync::mpsc::error::SendError(_)) => {
                // The receiver was dropped because the connection closed. Should probably do more here.
                tracing::debug!("initial send error");
            }
        }
    }

    // Finally, send `tx`, the sender of the newly created channel, to the
    // "global event sender" which will send further events to the connection.
    {
        let typ = ConnectionEventType::Connect(conn_tx);
        let connection_key = ConnectionKey { addr };

        match app_state
            .tx_new_connection
            .send(ConnectionEvent {
                typ,
                connection_key,
            })
            .await
        {
            Ok(()) => Ok(body),
            Err(_) => Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "sending new connection failed",
            )),
        }
    }
}

async fn cam_name_handler(
    axum::extract::State(app_state): axum::extract::State<StrandCamAppState>,
    session_key: axum_token_auth::SessionKey,
) -> impl axum::response::IntoResponse {
    session_key.is_present();
    app_state.cam_name.clone()
}

async fn callback_handler(
    axum::extract::State(app_state): axum::extract::State<StrandCamAppState>,
    session_key: axum_token_auth::SessionKey,
    TolerantJson(payload): TolerantJson<CallbackType>,
) -> impl axum::response::IntoResponse {
    session_key.is_present();
    tracing::trace!("callback");
    match payload {
        CallbackType::ToCamera(cam_arg) => {
            debug!("in cb: {:?}", cam_arg);
            app_state
                .callback_senders
                .cam_args_tx
                .send(cam_arg)
                .await
                .ignore_send_error();
        }
        CallbackType::FirehoseNotify(ck) => {
            app_state
                .callback_senders
                .firehose_callback_tx
                .send(ck)
                .await
                .ignore_send_error();
        }
        CallbackType::TakeCurrentImageAsBackground => {
            #[cfg(feature = "flydra_feat_detect")]
            app_state
                .callback_senders
                .tx_frame
                .send(Msg::TakeCurrentImageAsBackground)
                .await
                .ignore_send_error();
        }
        CallbackType::ClearBackground(value) => {
            #[cfg(feature = "flydra_feat_detect")]
            app_state
                .callback_senders
                .tx_frame
                .send(Msg::ClearBackground(value))
                .await
                .ignore_send_error();
            #[cfg(not(feature = "flydra_feat_detect"))]
            let _ = value;
        }
        CallbackType::ToLedBox(led_box_arg) => futures::executor::block_on(async {
            info!("in led_box callback: {:?}", led_box_arg);
            app_state
                .callback_senders
                .led_box_tx_std
                .send(led_box_arg)
                .await
                .ignore_send_error();
        }),
    }
    Ok::<_, axum::extract::rejection::JsonRejection>(axum::Json(()))
}

async fn handle_auth_error(err: tower::BoxError) -> (StatusCode, &'static str) {
    match err.downcast::<axum_token_auth::ValidationErrors>() {
        Ok(err) => {
            tracing::error!(
                "Validation error(s): {:?}",
                err.errors().collect::<Vec<_>>()
            );
            (StatusCode::UNAUTHORIZED, "Request is not authorized")
        }
        Err(orig_err) => {
            tracing::error!("Unhandled internal error: {orig_err}");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal server error")
        }
    }
}

/// Information acquired from Braid when the HTTP session is established.
#[derive(Debug)]
struct BraidInfo {
    mainbrain_session: braid_http_session::MainbrainSession,
    /// The address to which low-latency tracking data should be sent.
    ///
    /// Neither the IP nor the port are unspecified.
    camdata_udp_addr: SocketAddr,
    #[cfg_attr(not(feature = "flydra_feat_detect"), expect(dead_code))]
    tracker_cfg_src: ImPtDetectCfgSource,
    config_from_braid: braid_types::RemoteCameraInfoResponse,
}

/// Wrapper to enforce that first message is fixed to be
/// [braid_types::RegisterNewCamera].
struct FirstMsgForced {
    tx: tokio::sync::mpsc::Sender<braid_types::BraidHttpApiCallback>,
}

impl FirstMsgForced {
    /// Wrap a sender.
    fn new(tx: tokio::sync::mpsc::Sender<braid_types::BraidHttpApiCallback>) -> Self {
        Self { tx }
    }

    /// Send the first message and return the Sender.
    async fn send_first_msg(
        self,
        new_cam_data: braid_types::RegisterNewCamera,
    ) -> std::result::Result<
        tokio::sync::mpsc::Sender<braid_types::BraidHttpApiCallback>,
        tokio::sync::mpsc::error::SendError<braid_types::BraidHttpApiCallback>,
    > {
        self.tx
            .send(braid_types::BraidHttpApiCallback::NewCamera(new_cam_data))
            .await?;
        Ok(self.tx)
    }
}

// -----------

/// top-level function once args are parsed from CLI.
pub fn run_strand_cam_app<M, C, G>(
    mymod: ci2_async::ThreadedAsyncCameraModule<M, C, G>,
    args: StrandCamArgs,
    app_name: &'static str,
) -> Result<ci2_async::ThreadedAsyncCameraModule<M, C, G>>
where
    M: ci2::CameraModule<CameraType = C, Guard = G> + 'static,
    C: 'static + ci2::Camera + Send,
    G: Send + 'static,
{
    let (log_dir, data_dir) = if let Some(data_dir) = &args.data_dir {
        (data_dir.clone(), data_dir.clone())
    } else {
        (
            // default log_dir is home.
            home::home_dir().ok_or_else(|| {
                eyre::eyre!("Could not determine home directory and data directory not set.")
            })?,
            // default data_dir is pwd.
            PathBuf::from("."),
        )
    };

    // Initial log file name has process ID in case multiple cameras are
    // launched simultaneously. The (still open) log file gets renamed later to
    // include the camera name. We need to start logging as soon as possible
    // (before we necessarily know the camera name) because we may need to debug
    // connectivity problems to Braid or problems starting the camera.
    let log_file_time = chrono::Local::now();
    let initial_log_file_name = log_file_time
        .format(".strand-cam-%Y%m%d_%H%M%S.%f")
        .to_string()
        + &format!("-{}.log", std::process::id());
    let initial_log_file_name = log_dir.join(&initial_log_file_name);
    // TODO: delete log files older than, e.g. one week.

    #[cfg(feature = "eframe-gui")]
    let disable_console = true;

    #[cfg(not(feature = "eframe-gui"))]
    let disable_console = args.disable_console;

    let initial_log_file_name2 = initial_log_file_name.clone();

    let _guard =
        env_tracing_logger::initiate_logging(Some(&initial_log_file_name2), disable_console)
            .map_err(|e| eyre!("error initiating logging: {e}"))?;

    // create tokio runtime
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(4)
        .thread_name("strand-cam-runtime")
        .thread_stack_size(3 * 1024 * 1024)
        .build()?;

    let log_file_info = LogFileInfo {
        initial_log_file_name,
        log_dir,
        data_dir,
        log_file_time,
    };

    #[cfg(feature = "eframe-gui")]
    {
        let (quit_tx, quit_rx) = tokio::sync::mpsc::channel(1);

        let gui_singleton = Arc::new(std::sync::Mutex::new(GuiShared::default()));
        let gui_singleton2 = gui_singleton.clone();

        let (frame_tx, frame_rx) = tokio::sync::watch::channel(Arc::new(
            strand_dynamic_frame::DynamicFrameOwned::from_static(
                formats::owned::OImage::<formats::pixel_format::Mono8>::zeros(0, 0, 0).unwrap(),
            ),
        ));
        let (egui_ctx_tx, egui_ctx_rx) = std::sync::mpsc::channel();

        let gui_app_stuff = Some(GuiAppStuff {
            quit_rx,
            frame_tx,
            egui_ctx_rx,
        });

        // Move tokio runtime to new thread to keep GUI event loop on initial thread.
        let tokio_thread_jh = std::thread::Builder::new()
            .name("tokio-thread".to_string())
            .spawn(move || {
                let mymod = runtime.block_on(run_after_maybe_connecting_to_braid(
                    mymod,
                    args,
                    app_name,
                    log_file_info,
                    gui_app_stuff,
                    gui_singleton2,
                ))?;

                info!("done");
                Ok(mymod)
            })
            .map_err(|e| eyre::anyhow!("runtime failed with error {e}"))?;

        let native_options = Default::default();

        eframe::run_native(
            "Strand Camera",
            native_options,
            Box::new(move |cc| {
                Ok(Box::new(gui_app::StrandCamEguiApp::new(
                    quit_tx,
                    cc,
                    gui_singleton,
                    frame_rx,
                    egui_ctx_tx,
                )))
            }),
        )
        .map_err(|e| eyre::anyhow!("running failed with error {e}"))?;

        // Block until tokio done.
        tokio_thread_jh.join().unwrap()
    }

    #[cfg(not(feature = "eframe-gui"))]
    {
        let gui_singleton = ();

        let mymod = runtime.block_on(run_after_maybe_connecting_to_braid(
            mymod,
            args,
            app_name,
            log_file_info,
            None,
            gui_singleton,
        ))?;

        info!("done");
        Ok(mymod)
    }
}

struct GuiAppStuff {
    quit_rx: tokio::sync::mpsc::Receiver<()>,
    #[cfg(feature = "eframe-gui")]
    frame_tx: tokio::sync::watch::Sender<gui_app::ImType>,
    #[cfg(feature = "eframe-gui")]
    egui_ctx_rx: std::sync::mpsc::Receiver<eframe::egui::Context>,
}

async fn connect_to_braid(braid_args: &BraidArgs) -> Result<BraidInfo> {
    info!("Will connect to braid at \"{}\"", braid_args.braid_url);
    let mainbrain_bui_loc = BuiServerAddrInfo::parse_url_with_token(&braid_args.braid_url)?;

    let jar: cookie_store::CookieStore = match Preferences::load(&APP_INFO, BRAID_COOKIE_KEY) {
        Ok(jar) => {
            tracing::debug!("loaded cookie store {BRAID_COOKIE_KEY}");
            jar
        }
        Err(e) => {
            tracing::debug!("cookie store {BRAID_COOKIE_KEY} not loaded: {e} {e:?}");
            cookie_store::CookieStore::new(None)
        }
    };
    let jar = Arc::new(RwLock::new(jar));
    let mut mainbrain_session =
        braid_http_session::create_mainbrain_session(mainbrain_bui_loc.clone(), jar.clone())
            .await?;
    tracing::debug!("Opened HTTP session with Braid.");
    {
        // We have the cookie from braid now, so store it to disk.
        let jar = jar.read().unwrap();
        Preferences::save(&*jar, &APP_INFO, BRAID_COOKIE_KEY)?;
        tracing::debug!("saved cookie store {BRAID_COOKIE_KEY}");
    }

    let camera_name = braid_types::RawCamName::new(braid_args.camera_name.clone());

    let config_from_braid: braid_types::RemoteCameraInfoResponse =
        mainbrain_session.get_remote_info(&camera_name).await?;

    let camdata_udp_ip = mainbrain_bui_loc.addr().ip();
    let camdata_udp_port = config_from_braid.camdata_udp_port;
    let camdata_udp_addr = SocketAddr::new(camdata_udp_ip, camdata_udp_port);

    let tracker_cfg_src = crate::ImPtDetectCfgSource::ChangesNotSavedToDisk(
        config_from_braid.config.point_detection_config.clone(),
    );

    Ok(BraidInfo {
        mainbrain_session,
        config_from_braid,
        camdata_udp_addr,
        tracker_cfg_src,
    })
}

struct LogFileInfo {
    initial_log_file_name: PathBuf,
    /// where log files are saved
    log_dir: PathBuf,
    /// where movies are saved
    data_dir: PathBuf,
    log_file_time: chrono::DateTime<chrono::Local>,
}

/// First, connect to Braid if requested, then run.
async fn run_after_maybe_connecting_to_braid<M, C, G>(
    mymod: ci2_async::ThreadedAsyncCameraModule<M, C, G>,
    args: StrandCamArgs,
    app_name: &'static str,
    log_file_info: LogFileInfo,
    gui_app_stuff: Option<GuiAppStuff>,
    gui_singleton: ArcMutGuiSingleton,
) -> Result<ci2_async::ThreadedAsyncCameraModule<M, C, G>>
where
    M: ci2::CameraModule<CameraType = C, Guard = G>,
    C: 'static + ci2::Camera + Send,
    G: Send,
{
    // If connecting to braid, do it here.
    let braid_info = {
        match &args.standalone_or_braid {
            StandaloneOrBraid::Braid(braid_args) => Some(connect_to_braid(braid_args).await?),
            StandaloneOrBraid::Standalone(_) => None,
        }
    };

    let strand_cam_bui_http_address_string = match &args.standalone_or_braid {
        StandaloneOrBraid::Braid(braid_args) => {
            let braid_info = match &braid_info {
                Some(braid_info) => braid_info,
                None => {
                    eyre::bail!("requested braid, but no braid config");
                }
            };
            let http_server_addr = braid_info.config_from_braid.config.http_server_addr.clone();
            let braid_info = BuiServerAddrInfo::parse_url_with_token(&braid_args.braid_url)?;

            if braid_info.addr().ip().is_loopback() {
                http_server_addr.unwrap_or_else(|| "127.0.0.1:0".to_string())
            } else {
                http_server_addr.unwrap_or_else(|| "0.0.0.0:0".to_string())
            }
        }
        StandaloneOrBraid::Standalone(standalone_args) => standalone_args
            .http_server_addr
            .clone()
            .unwrap_or_else(|| "127.0.0.1:3440".to_string()),
    };
    tracing::debug!("Strand Camera HTTP server: {strand_cam_bui_http_address_string}");

    let target_feature_string = target::features().join(", ");
    info!("Compiled with features: {}", target_feature_string);

    if !imops::COMPILED_WITH_SIMD_SUPPORT {
        warn!("Package 'imops' was not compiled with simd support. Image processing with imops will be slow.");
    }

    let requested_camera_name = match &args.standalone_or_braid {
        StandaloneOrBraid::Standalone(args) => args.camera_name.clone(),
        StandaloneOrBraid::Braid(args) => Some(args.camera_name.clone()),
    };

    debug!("Request for camera \"{requested_camera_name:?}\"");

    // -----------------------------------------------

    info!("camera module: {}", mymod.name());

    let cam_infos = mymod.camera_infos()?;
    if cam_infos.is_empty() {
        eyre::bail!("No cameras found.");
    }

    for cam_info in cam_infos.iter() {
        info!("  camera {:?} detected", cam_info.name());
    }

    let use_camera_name = match requested_camera_name {
        Some(ref name) => name,
        None => cam_infos[0].name(),
    };

    // Rename the log file (which is open and being written to) so that the name
    // includes the camera name.
    let new_log_file_name = log_file_info
        .log_file_time
        .format(".strand-cam-%Y%m%d_%H%M%S.%f")
        .to_string()
        + &format!("-{}.log", use_camera_name);
    let new_log_file_name = log_file_info.log_dir.join(&new_log_file_name);

    tracing::debug!(
        "Renaming log file \"{}\" -> \"{}\"",
        log_file_info.initial_log_file_name.display(),
        new_log_file_name.display()
    );
    std::fs::rename(&log_file_info.initial_log_file_name, &new_log_file_name).with_context(
        || {
            format!(
                "Renaming log file \"{}\" -> \"{}\"",
                log_file_info.initial_log_file_name.display(),
                new_log_file_name.display()
            )
        },
    )?;

    run(
        mymod,
        args,
        app_name,
        braid_info,
        strand_cam_bui_http_address_string,
        use_camera_name,
        gui_app_stuff,
        gui_singleton,
        log_file_info.data_dir,
    )
    .await
}

// -----------

/// This is the main function where we spend all time after parsing startup args
/// and, in case of connecting to braid, getting the inital connection
/// information.
///
/// This function is way too huge and should be refactored.
#[tracing::instrument(skip(
    mymod,
    args,
    app_name,
    braid_info,
    strand_cam_bui_http_address_string,
    gui_app_stuff,
    gui_singleton,
    data_dir
))]
async fn run<M, C, G>(
    mut mymod: ci2_async::ThreadedAsyncCameraModule<M, C, G>,
    args: StrandCamArgs,
    app_name: &'static str,
    braid_info: Option<BraidInfo>,
    strand_cam_bui_http_address_string: String,
    cam: &str,
    gui_app_stuff: Option<GuiAppStuff>,
    gui_singleton: ArcMutGuiSingleton,
    data_dir: PathBuf,
) -> Result<ci2_async::ThreadedAsyncCameraModule<M, C, G>>
where
    M: ci2::CameraModule<CameraType = C, Guard = G>,
    C: 'static + ci2::Camera + Send,
    G: Send,
{
    let use_camera_name = cam; // simple arg name important for tracing::instrument
    let settings_file_ext = mymod.settings_file_extension().to_string();

    let (quit_rx, gui_stuff2) = if let Some(gas) = gui_app_stuff {
        let quit_rx = gas.quit_rx;

        #[cfg(feature = "eframe-gui")]
        let gui_stuff2 = {
            let frame_tx = gas.frame_tx;
            let egui_ctx_rx = gas.egui_ctx_rx;

            // Wait for egui context.
            let egui_ctx: eframe::egui::Context = egui_ctx_rx.recv().unwrap();
            let gui_stuff2 = Some((frame_tx, egui_ctx));
            gui_stuff2
        };

        #[cfg(not(feature = "eframe-gui"))]
        let gui_stuff2: Option<()> = None;

        (Some(quit_rx), gui_stuff2)
    } else {
        (None, None)
    };

    let mut cam = match mymod.threaded_async_camera(use_camera_name) {
        Ok(cam) => cam,
        Err(e) => {
            let msg = format!("{e}");
            error!("{}", msg);
            return Err(e.into());
        }
    };

    let raw_name = cam.name().to_string();
    info!("  got camera {}", raw_name);
    let raw_cam_name = RawCamName::new(raw_name);

    let camera_gamma = cam
        .feature_float("Gamma")
        .map_err(|e| warn!("Ignoring error getting gamma: {}", e))
        .ok()
        .map(|x: f64| x as f32);

    // Use `Result` as an enum with two options. It's not the case that one is a
    // non-error and the other an error condition. We just use `Result` rather
    // than defining our own enum type here.
    let res_braid = match (&braid_info, &args.standalone_or_braid) {
        (Some(bi), StandaloneOrBraid::Braid(_)) => Ok(bi),
        (None, StandaloneOrBraid::Standalone(a)) => Err(a),
        (Some(_), StandaloneOrBraid::Standalone(_)) | (None, StandaloneOrBraid::Braid(_)) => {
            unreachable!()
        }
    };

    let camera_settings_filename = match &res_braid {
        Ok(bi) => bi.config_from_braid.config.camera_settings_filename.clone(),
        Err(a) => a.camera_settings_filename.clone(),
    };

    let pixel_format = match &res_braid {
        Ok(bi) => bi.config_from_braid.config.pixel_format.clone(),
        Err(a) => a.pixel_format.clone(),
    };

    let send_image_to_braid_interval = res_braid.as_ref().ok().map(|bi| {
        std::time::Duration::from_millis(
            bi.config_from_braid.config.send_current_image_interval_msec,
        )
    });

    let acquisition_duration_allowed_imprecision_msec = match &res_braid {
        Ok(bi) => {
            bi.config_from_braid
                .config
                .acquisition_duration_allowed_imprecision_msec
        }
        Err(a) => a.acquisition_duration_allowed_imprecision_msec,
    };
    #[cfg(not(feature = "flydra_feat_detect"))]
    let _ = acquisition_duration_allowed_imprecision_msec;

    let (frame_rate_limit_supported, mut frame_rate_limit_enabled) = if let Some(fname) =
        &camera_settings_filename
    {
        let settings = std::fs::read_to_string(fname).with_context(|| {
            format!(
                "Failed to read camera settings from file \"{}\"",
                fname.display()
            )
        })?;

        cam.node_map_load(&settings)?;
        info!("loaded camera settings file \"{}\"", fname.display());
        (false, false)
    } else {
        for pixfmt in cam.possible_pixel_formats()?.iter() {
            debug!("  possible pixel format: {}", pixfmt);
        }

        if let Some(ref pixfmt_str) = pixel_format {
            use std::str::FromStr;
            let pixfmt = PixFmt::from_str(pixfmt_str).map_err(|e: &str| eyre!(e.to_string()))?;
            info!("  setting pixel format: {}", pixfmt);
            cam.set_pixel_format(pixfmt)?;
        }

        debug!("  current pixel format: {}", cam.pixel_format()?);

        let (frame_rate_limit_supported, frame_rate_limit_enabled) = {
            // This entire section should be removed and converted to a query
            // of the cameras capabilities.

            // Save the value of whether the frame rate limiter is enabled.
            let frame_rate_limit_enabled = cam.acquisition_frame_rate_enable()?;
            debug!("frame_rate_limit_enabled {}", frame_rate_limit_enabled);

            // Check if we can set the frame rate, first by setting a limit to be on.
            let frame_rate_limit_supported = match cam.set_acquisition_frame_rate_enable(true) {
                Ok(()) => {
                    debug!("set set_acquisition_frame_rate_enable true");
                    // Then by setting a limit to be off.
                    match cam.set_acquisition_frame_rate_enable(false) {
                        Ok(()) => {
                            debug!("{}:{}", file!(), line!());
                            true
                        }
                        Err(e) => {
                            debug!("err {} {}:{}", e, file!(), line!());
                            false
                        }
                    }
                }
                Err(e) => {
                    debug!("err {} {}:{}", e, file!(), line!());
                    false
                }
            };

            if frame_rate_limit_supported {
                // Restore the state of the frame rate limiter.
                cam.set_acquisition_frame_rate_enable(frame_rate_limit_enabled)?;
                debug!("set frame_rate_limit_enabled {}", frame_rate_limit_enabled);
            }

            (frame_rate_limit_supported, frame_rate_limit_enabled)
        };

        match cam.feature_enum_set("AcquisitionMode", "Continuous") {
            Ok(()) => {}
            Err(e) => {
                debug!("Ignoring error when setting AcquisitionMode: {}", e);
            }
        }
        (frame_rate_limit_supported, frame_rate_limit_enabled)
    };

    let settings_on_start = cam.node_map_save()?;

    let res_braid = match (&braid_info, &args.standalone_or_braid) {
        (Some(bi), StandaloneOrBraid::Braid(_)) => Ok(bi),
        (None, StandaloneOrBraid::Standalone(a)) => Err(a),
        (Some(_), StandaloneOrBraid::Standalone(_)) | (None, StandaloneOrBraid::Braid(_)) => {
            unreachable!()
        }
    };

    let force_camera_sync_mode = match &res_braid {
        Ok(bi) => bi.config_from_braid.force_camera_sync_mode,
        Err(a) => a.force_camera_sync_mode,
    };

    let camdata_udp_addr = match &res_braid {
        Ok(bi) => Some(bi.camdata_udp_addr),
        Err(_a) => None,
    };

    let software_limit_framerate = match &res_braid {
        Ok(bi) => bi.config_from_braid.software_limit_framerate.clone(),
        Err(a) => a.software_limit_framerate.clone(),
    };

    #[cfg(feature = "flydra_feat_detect")]
    let tracker_cfg_src = match &res_braid {
        Ok(bi) => bi.tracker_cfg_src.clone(),
        Err(a) => a.tracker_cfg_src.clone(),
    };

    // Here we just create some default, it does not matter what, because it
    // will not be used for anything.
    #[cfg(not(feature = "flydra_feat_detect"))]
    let im_pt_detect_cfg = flydra_pt_detect_cfg::default_absdiff();

    #[cfg(feature = "flydra_feat_detect")]
    let im_pt_detect_cfg = match &tracker_cfg_src {
        ImPtDetectCfgSource::ChangedSavedToDisk(src) => {
            // Retrieve the saved preferences
            let (app_info, ref prefs_key) = src;
            match ImPtDetectCfg::load(app_info, prefs_key) {
                Ok(cfg) => cfg,
                Err(e) => {
                    info!(
                        "Failed loading image detection config ({}), using defaults.",
                        e
                    );
                    default_im_pt_detect()
                }
            }
        }
        ImPtDetectCfgSource::ChangesNotSavedToDisk(cfg) => cfg.clone(),
    };

    let (mut mainbrain_session, trigger_type) = match braid_info {
        Some(bi) => (
            Some(bi.mainbrain_session),
            Some(bi.config_from_braid.trig_config),
        ),
        None => (None, None),
    };

    // Setup PTP and let clocks converge prior to starting acquisition.
    if let Some(TriggerType::PtpSync(ptpcfg)) = &trigger_type {
        let mut clock_sync_threshold_usecs = None;
        if let Some(period_usec) = ptpcfg.periodic_signal_period_usec {
            let period_usec_int = period_usec as i64;
            if period_usec - period_usec_int as f64 > 1.0 {
                eyre::bail!("period cannot be specified to sub-microsecond precision");
            }
            clock_sync_threshold_usecs = Some(period_usec_int / 2);
            if cam.feature_float(PERIOD_NAME)? != period_usec {
                cam.feature_float_set(PERIOD_NAME, period_usec)?;
                tracing::debug!(
                    "Set camera parameter {PERIOD_NAME} to {period_usec} microseconds."
                );
            }
        };
        if !cam.feature_bool("PtpEnable")? {
            tracing::debug!("Enabling PTP.");
            cam.feature_bool_set("PtpEnable", true)?;
        }
        // If period not set, default to 1 millisecond.
        let clock_sync_threshold_nanos = clock_sync_threshold_usecs.unwrap_or(1_000) * 1_000;
        loop {
            cam.command_execute("PtpDataSetLatch", true)?;
            let ptp_offset_from_master = cam.feature_int("PtpOffsetFromMaster")?;
            // Basler docs: "PtpOffsetFromMaster: Indicates the estimated
            // temporal offset between the master clock and the clock of the
            // current PTP device in ticks (1 tick = 1 nanosecond)."
            tracing::debug!("PTP clock offset {ptp_offset_from_master} nanoseconds.");
            if ptp_offset_from_master.abs() < clock_sync_threshold_nanos {
                // if within threshold from master, call it good enough.
                break;
            }
            tracing::info!(
                "PTP clock offset {ptp_offset_from_master} nanoseconds (threshold \
                        {clock_sync_threshold_nanos}), waiting 1 second for convergence."
            );
            tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
        }
        tracing::info!(
            "PTP clock within threshold {clock_sync_threshold_nanos} nanoseconds from master."
        );

        if cam.feature_enum("TriggerMode")? != "On" {
            cam.feature_enum_set("TriggerMode", "On")?;
        }
        if cam.feature_enum("TriggerSource")? != "PeriodicSignal1" {
            cam.feature_enum_set("TriggerSource", "PeriodicSignal1")?;
        }
    };

    // Start the camera.
    cam.acquisition_start()?;
    // Buffer 20 frames to be processed before dropping them.
    let (tx_frame, rx_frame) = tokio::sync::mpsc::channel::<Msg>(20);
    let tx_frame2 = tx_frame.clone();

    // Get initial frame to determine width, height and pixel_format.
    debug!("  started acquisition, waiting for first frame");
    let frame = cam.next_frame()?;
    info!(
        "  acquired first frame: {}x{}",
        frame.width(),
        frame.height()
    );

    #[cfg(target_os = "linux")]
    let v4l_out_stream = {
        let frame_image = frame.image.borrow();
        use machine_vision_formats::Stride;
        if let Some(v4l_device) = &args.v4l2loopback {
            use machine_vision_formats::ImageData;
            let frame = frame_image
                .as_static::<formats::pixel_format::Mono8>()
                .ok_or_else(|| {
                    eyre::eyre!(
                        "Currently unsupported pixel format for v4l2loopback: {:?}",
                        frame.pixel_format()
                    )
                })?;
            tracing::info!("Using v4l2loopback device {}", v4l_device.display());
            let out = v4l::device::Device::with_path(v4l_device).with_context(|| {
                format!("opening V4L2 loopback device {}", v4l_device.display())
            })?;
            let source_fmt = v4l::format::Format {
                width: frame.width().try_into()?,
                height: frame.height().try_into()?,
                stride: frame.stride().try_into()?,
                field_order: v4l::format::field::FieldOrder::Progressive,
                flags: 0.into(),
                size: u32::try_from(frame.stride())? * frame.height(),
                quantization: v4l::format::quantization::Quantization::FullRange,
                transfer: v4l::format::transfer::TransferFunction::None,
                fourcc: v4l::format::fourcc::FourCC::new(b"GREY"),
                colorspace: v4l::format::colorspace::Colorspace::RAW,
            };
            tracing::info!("Setting v4l2loopback format: {:?}", source_fmt);
            v4l::video::Output::set_format(&out, &source_fmt)?;

            let mut v4l_out_stream =
                v4l::io::mmap::stream::Stream::new(&out, v4l::buffer::Type::VideoOutput)?;

            let (buf_out, buf_out_meta) = v4l::io::traits::OutputStream::next(&mut v4l_out_stream)?;
            let buf_in = frame.image_data();
            let bytesused = buf_in.len().try_into()?;

            let buf_out = &mut buf_out[0..buf_in.len()];
            buf_out.copy_from_slice(buf_in);
            buf_out_meta.field = 0;
            buf_out_meta.bytesused = bytesused;
            Some(v4l_out_stream)
        } else {
            None
        }
    };

    let (firehose_tx, firehose_rx) = tokio::sync::mpsc::channel::<AnnotatedFrame>(5);

    // Put first frame in channel.
    firehose_tx
        .send(AnnotatedFrame {
            frame: frame.image.clone(),
            found_points: vec![],
            valid_display: None,
            annotations: vec![],
        })
        .await
        .unwrap();
    // .map_err(|e| anhow::anyhow!("failed to send frame"))?;

    let image_width = frame.width();
    let image_height = frame.height();

    let current_image_png = frame
        .image
        .borrow()
        .to_encoded_buffer(convert_image::EncoderOptions::Png)?;

    // spawn channel to send data to mainbrain
    let (mainbrain_msg_tx, mut mainbrain_msg_rx) = tokio::sync::mpsc::channel(10);

    let first_msg_tx = if mainbrain_session.is_some() {
        // Wrap Sender to force a specific first message type.
        Some(FirstMsgForced::new(mainbrain_msg_tx))
    } else {
        // Drop Sender as we'll never need it.
        None
    };

    let mainbrain_transmitter_fut = async move {
        while let Some(msg) = mainbrain_msg_rx.recv().await {
            if let Some(ref mut mainbrain_session) = &mut mainbrain_session {
                match mainbrain_session.post_callback_message(msg).await {
                    Ok(()) => {}
                    Err(e) => {
                        tracing::error!("failed sending message to mainbrain: {e}");
                        break;
                    }
                }
            }
        }
    };

    const PERIOD_NAME: &str = "BslPeriodicSignalPeriod";

    let mut local_remote = Vec::new();
    let mut local_time0 = None;
    let mut cam_time0 = None;
    let mut device_clock_model = None;

    if trigger_type == Some(TriggerType::DeviceTimestamp) {
        // Attempt to relate camera timestamps to our clock
        tracing::info!("Reading camera timestamps to fit initial clock model.");

        let n_pts = 5;
        let mut tmp_debug_device_timestamp = None;
        for i in 0..n_pts {
            let (local, cam_time) = measure_times(&cam)?;
            tmp_debug_device_timestamp.get_or_insert(cam_time);
            let local_time_nanos = braid_types::PtpStamp::try_from(local).unwrap().get();
            local_time0.get_or_insert(local_time_nanos);
            let cam_time_ts = braid_types::PtpStamp::new(cam_time.try_into().unwrap()).get();
            cam_time0.get_or_insert(cam_time_ts);

            let this_local_time0 = local_time0.as_ref().unwrap();
            let this_cam_time0 = cam_time0.as_ref().unwrap();
            // dbg!(&local_time_nanos);
            // dbg!(&local_time_secs);
            let local_elapsed_nanos = local_time_nanos - this_local_time0;
            let device_elapsed_nanos = cam_time_ts - this_cam_time0;
            local_remote.push((device_elapsed_nanos as f64, local_elapsed_nanos as f64));
            // local_remote.push((cam_time_ts as f64, local_ts as f64));
            if i < n_pts - 1 {
                tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
            }
        }
        let (gain, offset, residuals) = clock_model::fit_time_model(&local_remote)?;
        dbg!((gain, offset, residuals));

        let cm = strand_cam_bui_types::ClockModel {
            gain,
            offset,
            residuals,
            n_measurements: local_remote.len().try_into().unwrap(),
        };

        let device_timestamp: u64 = tmp_debug_device_timestamp.unwrap().try_into().unwrap();
        let this_cam_time0 = cam_time0.as_ref().unwrap();
        let device_elapsed_nanos = device_timestamp - this_cam_time0;
        let local_estimate_elapsed_nanos: f64 = (device_elapsed_nanos as f64) * cm.gain + cm.offset;

        dbg!((local_estimate_elapsed_nanos, device_timestamp, &cm));
        device_clock_model = Some(cm);
    }

    let local_and_cam_time0 = if let Some(ct0) = cam_time0 {
        let local_time0 = local_time0.as_ref().unwrap();
        Some((*local_time0, ct0))
    } else {
        None
    };

    let camera_periodic_signal_period_usec = {
        match cam.feature_float(PERIOD_NAME) {
            Ok(value) => {
                tracing::debug!("Camera parameter {PERIOD_NAME}: {value} microseconds");
                Some(value)
            }
            Err(e) => {
                tracing::debug!("Could not read feature {PERIOD_NAME}: {e}");
                None
            }
        }
    };

    let (cam_args_tx, cam_args_rx) = tokio::sync::mpsc::channel(100);
    let (led_box_tx_std, mut led_box_rx) = tokio::sync::mpsc::channel(20);

    let led_box_heartbeat_update_arc = Arc::new(RwLock::new(None));

    let gain_ranged = RangedValue {
        name: "gain".into(),
        unit: "dB".into(),
        min: cam.gain_range()?.0,
        max: cam.gain_range()?.1,
        current: cam.gain()?,
    };
    let exposure_ranged = RangedValue {
        name: "exposure time".into(),
        unit: "μsec".into(),
        min: cam.exposure_time_range()?.0,
        max: cam.exposure_time_range()?.1,
        current: cam.exposure_time()?,
    };
    let gain_auto = cam.gain_auto().ok();
    let exposure_auto = cam.exposure_auto().ok();

    let mut frame_rate_limit = if frame_rate_limit_supported {
        let (min, max) = cam.acquisition_frame_rate_range()?;
        Some(RangedValue {
            name: "frame rate".into(),
            unit: "Hz".into(),
            min,
            max,
            current: cam.acquisition_frame_rate()?,
        })
    } else {
        None
    };

    let current_cam_settings_extension = settings_file_ext.to_string();

    let (listener, http_camserver_info) =
        braid_types::start_listener(&strand_cam_bui_http_address_string).await?;
    let listen_addr = listener.local_addr()?;

    let mut transmit_msg_tx = None;
    if let Some(first_msg_tx) = first_msg_tx {
        let new_cam_data = braid_types::RegisterNewCamera {
            raw_cam_name: raw_cam_name.clone(),
            http_camserver_info: Some(BuiServerInfo::Server(http_camserver_info.clone())),
            cam_settings_data: Some(braid_types::UpdateCamSettings {
                current_cam_settings_buf: settings_on_start,
                current_cam_settings_extension: settings_file_ext,
            }),
            current_image_png: current_image_png.into(),
            camera_periodic_signal_period_usec,
        };

        // Get the generic sender back.
        transmit_msg_tx = Some(first_msg_tx.send_first_msg(new_cam_data).await?);
        tracing::info!("Registered camera with Braid.");
    }

    if force_camera_sync_mode {
        cam.start_default_external_triggering().unwrap();
        if let Some(transmit_msg_tx) = &transmit_msg_tx {
            send_cam_settings_to_braid(
                &cam.node_map_save()?,
                transmit_msg_tx,
                &current_cam_settings_extension,
                &raw_cam_name,
            )
            .await?;
        }
    }

    if camera_settings_filename.is_none() {
        if let StartSoftwareFrameRateLimit::Enable(fps_limit) = &software_limit_framerate {
            // Set the camera.
            cam.set_software_frame_rate_limit(*fps_limit).unwrap();
            // Store the values we set.
            if let Some(ref mut ranged) = frame_rate_limit {
                ranged.current = cam.acquisition_frame_rate()?;
            } else {
                panic!("cannot set software frame rate limit");
            }
            frame_rate_limit_enabled = cam.acquisition_frame_rate_enable()?;
        }
    }

    let trigger_mode = cam.trigger_mode()?;
    let trigger_selector = cam.trigger_selector()?;
    debug!("  got camera values");

    #[cfg(feature = "flydra_feat_detect")]
    let camera_cfg = CameraCfgFview2_0_26 {
        vendor: cam.vendor().into(),
        model: cam.model().into(),
        serial: cam.serial().into(),
        width: cam.width()?,
        height: cam.height()?,
    };

    #[cfg(feature = "flydratrax")]
    let kalman_tracking_config = {
        if let ImPtDetectCfgSource::ChangedSavedToDisk(ref src) = tracker_cfg_src {
            // Retrieve the saved preferences
            let (ref app_info, ref _im_pt_detect_prefs_key) = src;
            match KalmanTrackingConfig::load(app_info, KALMAN_TRACKING_PREFS_KEY) {
                Ok(cfg) => cfg,
                Err(e) => {
                    info!(
                        "Failed loading kalman tracking config ({}), using defaults.",
                        e
                    );
                    KalmanTrackingConfig::default()
                }
            }
        } else {
            panic!("flydratrax requires saving changes to disk");
        }
    };

    #[cfg(not(feature = "flydratrax"))]
    let kalman_tracking_config = KalmanTrackingConfig::default();

    #[cfg(feature = "flydratrax")]
    let led_program_config = {
        if let ImPtDetectCfgSource::ChangedSavedToDisk(ref src) = tracker_cfg_src {
            // Retrieve the saved preferences
            let (ref app_info, ref _im_pt_detect_prefs_key) = src;
            match LedProgramConfig::load(app_info, LED_PROGRAM_PREFS_KEY) {
                Ok(cfg) => cfg,
                Err(e) => {
                    info!("Failed loading LED config ({}), using defaults.", e);
                    LedProgramConfig::default()
                }
            }
        } else {
            panic!("flydratrax requires saving changes to disk");
        }
    };
    #[cfg(not(feature = "flydratrax"))]
    let led_program_config = LedProgramConfig::default();

    let cuda_devices = match nvenc::Dynlibs::new() {
        Ok(libs) => {
            match nvenc::NvEnc::new(&libs) {
                Ok(nv_enc) => {
                    let n = nv_enc.cuda_device_count()?;
                    let r: Result<Vec<String>> = (0..n)
                        .map(|i| {
                            let dev = nv_enc.new_cuda_device(i)?;
                            Ok(dev.name().map_err(nvenc::NvEncError::from)?)
                        })
                        .collect();
                    r?
                }
                Err(e) => {
                    info!(
                        "CUDA and nvidia-encode libraries loaded, but \
                        error during initialization: {}",
                        e,
                    );
                    // empty vector
                    Vec::new()
                }
            }
        }
        Err(e) => {
            // no cuda library, no libs
            info!("CUDA and nvidia-encode libraries not loaded: {}", e);
            // empty vector
            Vec::new()
        }
    };
    let mp4_cuda_device = if !cuda_devices.is_empty() {
        cuda_devices[0].as_str()
    } else {
        ""
    }
    .into();

    #[cfg(not(feature = "fiducial"))]
    let apriltag_state = None;

    #[cfg(feature = "fiducial")]
    let apriltag_state = Some(ApriltagState::default());

    let im_ops_state = ImOpsState::default();

    #[cfg(feature = "flydra_feat_detect")]
    let has_image_tracker_compiled = true;

    #[cfg(not(feature = "flydra_feat_detect"))]
    let has_image_tracker_compiled = false;

    let is_braid = match &args.standalone_or_braid {
        StandaloneOrBraid::Braid(_) => true,
        StandaloneOrBraid::Standalone(_) => false,
    };

    // -----------------------------------------------
    // Check if we can use nv h264 and, if so, set that as default.

    let ffmpeg_version = match ffmpeg_writer::ffmpeg_version() {
        Ok(ffmpeg_version) => Some(ffmpeg_version),
        Err(err) => {
            tracing::warn!("Could not identify ffmpeg version. {err}");
            None
        }
    };

    let is_nvenc_functioning = test_nvenc_save(frame.image.borrow())?;

    let mp4_codec = match is_nvenc_functioning {
        true => CodecSelection::H264Nvenc,
        false => CodecSelection::H264OpenH264,
    };

    #[cfg(target_os = "macos")]
    let is_videotoolbox_functioning = true;

    #[cfg(not(target_os = "macos"))]
    let is_videotoolbox_functioning = false;

    // -----------------------------------------------

    let mp4_filename_template = args
        .mp4_filename_template
        .replace("{CAMNAME}", raw_cam_name.as_str());
    let fmf_filename_template = args
        .fmf_filename_template
        .replace("{CAMNAME}", raw_cam_name.as_str());
    let ufmf_filename_template = args
        .ufmf_filename_template
        .replace("{CAMNAME}", raw_cam_name.as_str());

    #[cfg(feature = "fiducial")]
    let format_str_apriltag_csv = args
        .apriltag_csv_filename_template
        .replace("{CAMNAME}", use_camera_name);

    #[cfg(not(feature = "fiducial"))]
    let format_str_apriltag_csv = "".into();

    #[cfg(feature = "flydratrax")]
    let has_flydratrax_compiled = true;

    #[cfg(not(feature = "flydratrax"))]
    let has_flydratrax_compiled = false;

    let shared_store = ChangeTracker::new(StoreType {
        is_braid,
        ffmpeg_version,
        is_nvenc_functioning,
        is_videotoolbox_functioning,
        is_recording_mp4: None,
        is_recording_fmf: None,
        is_recording_ufmf: None,
        format_str_apriltag_csv,
        format_str_mp4: mp4_filename_template,
        format_str: fmf_filename_template,
        format_str_ufmf: ufmf_filename_template,
        camera_name: cam.name().into(),
        camera_gamma,
        recording_filename: None,
        mp4_bitrate: Default::default(),
        mp4_codec,
        mp4_max_framerate: Default::default(),
        mp4_cuda_device,
        gain: gain_ranged,
        gain_auto,
        exposure_time: exposure_ranged,
        exposure_auto,
        frame_rate_limit_enabled,
        frame_rate_limit,
        trigger_mode,
        trigger_selector,
        image_width,
        image_height,
        is_doing_object_detection: false,
        measured_fps: 0.0,
        is_saving_im_pt_detect_csv: None,
        has_image_tracker_compiled,
        im_pt_detect_cfg: im_pt_detect_cfg.clone(),
        has_flydratrax_compiled,
        kalman_tracking_config,
        led_program_config,
        led_box_device_lost: false,
        led_box_device_state: None,
        led_box_device_path: args.led_box_device_path.clone(),
        #[cfg(feature = "checkercal")]
        has_checkercal_compiled: true,
        #[cfg(not(feature = "checkercal"))]
        has_checkercal_compiled: false,
        checkerboard_data: strand_cam_storetype::CheckerboardCalState::default(),
        checkerboard_save_debug: None,
        post_trigger_buffer_size: 0,
        cuda_devices,
        apriltag_state,
        im_ops_state,
        had_frame_processing_error: false,
        camera_calibration: None,
    });

    let frame_processing_error_state = Arc::new(RwLock::new(FrameProcessingErrorState::default()));

    // let mut config = get_default_config();
    // config.cookie_name = "strand-camclient".to_string();

    let mut shared_store_changes_rx = shared_store.get_changes(1);

    // A channel for the data sent from the client browser.
    let (firehose_callback_tx, firehose_callback_rx) = tokio::sync::mpsc::channel(10);

    let callback_senders = StrandCamCallbackSenders {
        cam_args_tx: cam_args_tx.clone(),
        firehose_callback_tx,
        led_box_tx_std: led_box_tx_std.clone(),
        tx_frame: tx_frame.clone(),
    };

    let (tx_new_connection, rx_new_connection) = tokio::sync::mpsc::channel(10);

    let shared_state = Arc::new(RwLock::new(shared_store));
    let shared_store_arc = shared_state.clone();

    // Create our app state.
    let app_state = StrandCamAppState {
        cam_name: cam.name().to_string(),
        event_broadcaster: Default::default(),
        callback_senders,
        tx_new_connection,
        shared_store_arc,
    };

    let shared_store_arc = shared_state.clone();

    // This future will send state updates to all connected event listeners.
    let event_broadcaster = app_state.event_broadcaster.clone();
    let send_updates_future = async move {
        while let Some((_prev_state, next_state)) = shared_store_changes_rx.next().await {
            let chunk = to_event_chunk(&next_state);
            event_broadcaster.broadcast_frame(chunk).await;
        }
    };

    #[cfg(feature = "bundle_files")]
    let serve_dir = tower_serve_static::ServeDir::new(&ASSETS_DIR);

    #[cfg(feature = "serve_files")]
    let serve_dir = tower_http::services::fs::ServeDir::new(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("yew_frontend")
            .join("dist"),
    );

    let persistent_secret_base64 = if let Some(secret) = &args.secret {
        secret.clone()
    } else {
        match String::load(&APP_INFO, COOKIE_SECRET_KEY) {
            Ok(secret_base64) => secret_base64,
            Err(_) => {
                tracing::debug!("No secret loaded from preferences file, generating new.");
                let persistent_secret = cookie::Key::generate();
                let persistent_secret_base64 = base64::encode(persistent_secret.master());
                persistent_secret_base64.save(&APP_INFO, COOKIE_SECRET_KEY)?;
                persistent_secret_base64
            }
        }
    };

    let persistent_secret = base64::decode(persistent_secret_base64)?;
    let persistent_secret = cookie::Key::try_from(persistent_secret.as_slice())?;

    // Setup our auth layer.
    let token_config = match http_camserver_info.token() {
        AccessToken::PreSharedToken(value) => Some(axum_token_auth::TokenConfig {
            name: "token".to_string(),
            value: value.clone(),
        }),
        AccessToken::NoToken => None,
    };
    let cfg = axum_token_auth::AuthConfig {
        token_config,
        persistent_secret,
        cookie_name: "strand-cam-session",
        cookie_expires: Some(std::time::Duration::from_secs(60 * 60 * 24 * 400)), // 400 days
    };

    let auth_layer = cfg.into_layer();
    // Create axum router.
    let router = axum::Router::new()
        .route("/strand-cam-events", axum::routing::get(events_handler))
        .route("/cam-name", axum::routing::get(cam_name_handler))
        .route("/callback", axum::routing::post(callback_handler))
        .fallback_service(serve_dir)
        .layer(
            tower::ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                // Auth layer will produce an error if the request cannot be
                // authorized so we must handle that.
                .layer(axum::error_handling::HandleErrorLayer::new(
                    handle_auth_error,
                ))
                .layer(auth_layer),
        )
        .with_state(app_state);

    // create future for our app
    let http_serve_future = {
        use std::future::IntoFuture;
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .into_future()
    };

    let urls = strand_bui_backend_session::build_urls(&http_camserver_info)?;

    #[cfg(feature = "eframe-gui")]
    {
        // Loop until GUI from other thread is available.
        loop {
            {
                // scope for holding lock
                let mut my_guard = gui_singleton.lock().unwrap();

                // http_camserver_info
                // Set URL
                my_guard.url = Some(format!("{}", urls[0]));

                // Ensure URL is drawn
                if let Some(ctx_ref) = my_guard.ctx.as_ref() {
                    ctx_ref.request_repaint();
                    // We have GUI, exit wait loop.
                    break;
                }
            }
            // Wait a bit and check if GUI has launched again.
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }

    #[cfg(not(feature = "eframe-gui"))]
    #[allow(clippy::let_unit_value)]
    let _ = gui_singleton;

    // Display where we are listening.
    if is_braid {
        debug!("Strand Cam listening at {listen_addr}");
    } else {
        info!("Strand Cam listening at {listen_addr}");

        for url in urls.iter() {
            info!(" * predicted URL {url}");
            if !braid_types::is_loopback(url) {
                println!("QR code for {url}");
                display_qr_url(&format!("{url}"))?;
            }
        }
    }

    #[cfg(feature = "checkercal")]
    let collected_corners_arc: CollectedCornersArc = Arc::new(RwLock::new(Vec::new()));

    let frame_process_task_fut = {
        #[cfg(feature = "flydra_feat_detect")]
        let csv_save_dir = args.csv_save_dir.clone();

        #[cfg(feature = "flydratrax")]
        let model_server_addr = args.model_server_addr.clone();

        #[cfg(feature = "flydratrax")]
        let led_box_tx_std = led_box_tx_std.clone();
        #[cfg(feature = "flydratrax")]
        let http_camserver_info2 = http_camserver_info.clone();
        let led_box_heartbeat_update_arc2 = led_box_heartbeat_update_arc.clone();
        #[cfg(feature = "flydratrax")]
        let model_server_data_tx = {
            info!("send_pose server at {model_server_addr}");
            let (model_server_data_tx, data_rx) = tokio::sync::mpsc::channel(50);
            let model_server_future = flydra2::new_model_server(data_rx, model_server_addr);
            tokio::spawn(async { model_server_future.await });
            model_server_data_tx
        };

        let cam_name2 = raw_cam_name.clone();
        frame_process_task(
            #[cfg(feature = "flydratrax")]
            model_server_data_tx,
            cam_name2,
            #[cfg(feature = "flydra_feat_detect")]
            camera_cfg,
            #[cfg(feature = "flydra_feat_detect")]
            image_width,
            #[cfg(feature = "flydra_feat_detect")]
            image_height,
            rx_frame,
            #[cfg(feature = "flydra_feat_detect")]
            im_pt_detect_cfg,
            #[cfg(feature = "flydra_feat_detect")]
            std::path::Path::new(&csv_save_dir).to_path_buf(),
            firehose_tx,
            #[cfg(feature = "flydratrax")]
            led_box_tx_std,
            #[cfg(feature = "flydratrax")]
            http_camserver_info2,
            transmit_msg_tx.clone(),
            camdata_udp_addr,
            led_box_heartbeat_update_arc2,
            #[cfg(feature = "checkercal")]
            collected_corners_arc.clone(),
            #[cfg(feature = "flydratrax")]
            &args,
            #[cfg(feature = "flydra_feat_detect")]
            acquisition_duration_allowed_imprecision_msec,
            #[cfg(feature = "flydra_feat_detect")]
            app_name,
            device_clock_model,
            local_and_cam_time0,
            trigger_type,
            #[cfg(target_os = "linux")]
            v4l_out_stream,
            data_dir,
        )
    };
    debug!("frame_process_task spawned");

    tx_frame
        .send(Msg::Store(shared_store_arc.clone()))
        .await
        .unwrap();

    debug!("installing frame stream handler");

    // install frame handling
    let n_buffered_frames = 100;
    let mut frame_stream = cam.frames(n_buffered_frames)?;
    let cam_stream_future = {
        let shared_store_arc = shared_store_arc.clone();
        let frame_processing_error_state = frame_processing_error_state.clone();
        let mut transmit_msg_tx = transmit_msg_tx.clone();
        let raw_cam_name = raw_cam_name.clone();
        async move {
            let mut send_image_to_braid_timer = std::time::Instant::now();
            let mut send_image_to_braid_duration = std::time::Duration::from_millis(0);
            while let Some(frame_msg) = frame_stream.next().await {
                match &frame_msg {
                    ci2_async::FrameResult::Frame(fframe) => {
                        {
                            let frame: &DynamicFrame = &fframe.image.borrow();
                            trace!(
                                "  got frame {}: {}x{}",
                                fframe.host_timing.fno,
                                frame.width(),
                                frame.height()
                            );
                        }

                        #[cfg(not(feature = "eframe-gui"))]
                        let _ = gui_stuff2.as_ref();

                        #[cfg(feature = "eframe-gui")]
                        {
                            if let Some((gui_frame_tx, egui_ctx)) = gui_stuff2.as_ref() {
                                let arc_clone = fframe.image.clone(); // copy pointer and increment refcount

                                match gui_frame_tx.send(arc_clone) {
                                    Ok(()) => {
                                        egui_ctx.request_repaint();
                                    }
                                    Err(_arc_clone) => {
                                        eyre::bail!("GUI disconnected");
                                    }
                                }
                            }
                        }

                        if tx_frame.capacity() == 0 {
                            let mut tracker = shared_store_arc.write().unwrap();
                            tracker.modify(|tracker| {
                                let mut state = frame_processing_error_state.write().unwrap();
                                {
                                    match &*state {
                                        FrameProcessingErrorState::IgnoreAll => {}
                                        FrameProcessingErrorState::IgnoreUntil(ignore_until) => {
                                            let now = chrono::Utc::now();
                                            if now >= *ignore_until {
                                                tracker.had_frame_processing_error = true;
                                                *state = FrameProcessingErrorState::NotifyAll;
                                            }
                                        }
                                        FrameProcessingErrorState::NotifyAll => {
                                            tracker.had_frame_processing_error = true;
                                        }
                                    }
                                }
                            });
                            error!("Channel full sending frame to process thread. Dropping frame data.");
                        } else {
                            tx_frame
                                .send(Msg::Mframe(fframe.clone()))
                                .await
                                .map_err(to_eyre)?;
                        }
                    }
                    ci2_async::FrameResult::SingleFrameError(s) => {
                        error!("SingleFrameError({})", s);
                    }
                }

                if let ci2_async::FrameResult::Frame(frame) = &frame_msg {
                    if let Some(transmit_msg_tx) = transmit_msg_tx.as_mut() {
                        // Check if we need to send this frame to braid because our timer elapsed.
                        if send_image_to_braid_timer.elapsed() >= send_image_to_braid_duration {
                            // If yes, encode frame to png buffer.
                            let current_image_png = frame
                                .image
                                .borrow()
                                .to_encoded_buffer(convert_image::EncoderOptions::Png)
                                .unwrap();

                            // Prepare and send message to Braid.
                            let msg = braid_types::BraidHttpApiCallback::UpdateCurrentImage(
                                braid_types::PerCam {
                                    raw_cam_name: raw_cam_name.clone(),
                                    inner: braid_types::UpdateImage {
                                        current_image_png: current_image_png.into(),
                                    },
                                },
                            );
                            transmit_msg_tx.send(msg).await?;

                            // Update timer for next iteration.
                            send_image_to_braid_timer = std::time::Instant::now();
                            if let Some(dur) = send_image_to_braid_interval {
                                send_image_to_braid_duration = dur;
                            }
                        }
                    }
                }
            }
            debug!("cam_stream_future future done {}:{}", file!(), line!());
            Ok::<_, eyre::Report>(())
        }
    };

    let do_version_check = match std::env::var_os("DISABLE_VERSION_CHECK") {
        Some(v) => &v == "0",
        None => true,
    };

    // This is quick-and-dirtry version checking. It can be cleaned up substantially...
    if do_version_check {
        let app_version: semver::Version = {
            let mut my_version = semver::Version::parse(env!("CARGO_PKG_VERSION")).unwrap();
            my_version.build = semver::BuildMetadata::new(env!("GIT_HASH").to_string().as_str())?;
            my_version
        };

        info!(
            "Welcome to {} {}. For more details \
            contact Andrew Straw <straw@bio.uni-freiburg.de>. This program will check for new \
            versions automatically. To disable printing this message and checking for new \
            versions, set the environment variable DISABLE_VERSION_CHECK=1.",
            app_name, app_version,
        );

        // TODO I just used Arc and RwLock to code this quickly. Convert to single-threaded
        // versions later.
        let known_version = Arc::new(RwLock::new(app_version));

        // Create a stream to call our closure now and every 30 minutes.
        let interval_stream = tokio::time::interval(std::time::Duration::from_secs(1800));

        let mut interval_stream = tokio_stream::wrappers::IntervalStream::new(interval_stream);

        let known_version2 = known_version;
        let stream_future = async move {
            while interval_stream.next().await.is_some() {
                let https = hyper_rustls::HttpsConnectorBuilder::new()
                    .with_webpki_roots()
                    .https_only()
                    .enable_http1()
                    .build();
                let client = Client::builder(TokioExecutor::new()).build::<_, MyBody>(https);

                let r = check_version(client, known_version2.clone(), app_name).await;
                match r {
                    Ok(()) => {}
                    Err(e) => {
                        error!("error checking version: {}", e);
                    }
                }
            }
            debug!("version check future done {}:{}", file!(), line!());
        };
        tokio::spawn(Box::pin(stream_future));
        debug!("version check future spawned {}:{}", file!(), line!());
    }

    tokio::spawn(Box::pin(cam_stream_future));
    debug!("cam_stream_future future spawned {}:{}", file!(), line!());

    let cam_arg_future = {
        let shared_store_arc = shared_store_arc.clone();

        #[cfg(feature = "checkercal")]
        let cam_name2 = raw_cam_name.clone();

        let mut cam_args_rx = tokio_stream::wrappers::ReceiverStream::new(cam_args_rx);

        async move {
            // We do not put cam_args_rx behind a stream_cancel::Valve because
            // it is the top-level controller for quitting everything - if
            // a DoQuit message is received, then this while loop will end
            // and all the cleanup below will fire. It is done this way because
            // we need to be able to quit Strand Cam as a standalone program in
            // which case it catches its own Ctrl-C and then fires a DoQuit message,
            // or if it is run within Braid, in which Braid will send it a DoQuit
            // message. Finally, when other threads panic, they should also send a
            // DoQuit message.
            while let Some(cam_args) = cam_args_rx.next().await {
                debug!("handling camera command {:?}", cam_args);
                #[allow(unused_variables)]
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
                                    let when = chrono::Utc::now()
                                        + chrono::Duration::try_seconds(val).unwrap();
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
                    CamArg::SetFrameRateLimitEnabled(v) => {
                        match cam.set_acquisition_frame_rate_enable(v) {
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
                                tracker.modify(|shared| {
                                match cam.acquisition_frame_rate_enable() {
                                    Ok(latest) => {
                                        shared.frame_rate_limit_enabled = latest;
                                    },
                                    Err(e) => {
                                        error!("after setting frame_rate_limit_enabled, error getting: {:?}",e);
                                    }
                                }
                            });
                            }
                            Err(e) => {
                                error!("setting frame_rate_limit_enabled: {:?}", e);
                            }
                        }
                    }
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
                                    error!(
                                        "after setting frame_rate_limit, error getting: {:?}",
                                        e
                                    );
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
                                Some(recording_path) => {
                                    Msg::StartAprilTagRec(recording_path.path())
                                }
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
                                    let local: chrono::DateTime<chrono::Local> =
                                        chrono::Local::now();
                                    let filename =
                                        local.format(format_str_ufmf.as_str()).to_string();
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

                                if let ImPtDetectCfgSource::ChangedSavedToDisk(ref src) =
                                    tracker_cfg_src
                                {
                                    let (app_info, ref prefs_key) = src;
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
                                    error!(
                                        "ignoring KalmanTrackingConfig with parse error: {:?}",
                                        e
                                    )
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
                                        let (ref app_info, _) = src;
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
                                        let (ref app_info, _) = src;
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

                                        let local: chrono::DateTime<chrono::Local> =
                                            chrono::Local::now();
                                        let format_str = "checkerboard_debug_%Y%m%d_%H%M%S";
                                        let stamped = local.format(&format_str).to_string();
                                        let dirname = basedir.join(stamped);
                                        info!(
                                            "Saving checkerboard debug data to: {}",
                                            dirname.display()
                                        );
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
                                let checkerboard_save_debug =
                                    shared.checkerboard_save_debug.clone();
                                (n_rows, n_cols, checkerboard_save_debug)
                            };

                            let goodcorners: Vec<camcal::CheckerBoardData> = {
                                let collected_corners = collected_corners_arc.read().unwrap();
                                collected_corners
                                    .iter()
                                    .map(|corners| {
                                        let x: Vec<(f64, f64)> = corners
                                            .iter()
                                            .map(|x| (x.0 as f64, x.1 as f64))
                                            .collect();
                                        camcal::CheckerBoardData::new(
                                            n_rows as usize,
                                            n_cols as usize,
                                            &x,
                                        )
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
                                serde_yaml::to_writer(f, &debug_data)
                                    .expect("serde_yaml::to_writer");
                            }

                            let size =
                                camcal::PixelSize::new(image_width as usize, image_height as usize);

                            match camcal::compute_intrinsics_with_raw_opencv::<f64>(
                                size,
                                &goodcorners,
                            ) {
                                Ok(raw_opencv_cal) => {
                                    let cal_dir = directories::BaseDirs::new()
                                        .as_ref()
                                        .map(|bd| {
                                            bd.config_dir().join(APP_INFO.name).join("camera_info")
                                        })
                                        .unwrap();

                                    if !cal_dir.exists() {
                                        std::fs::create_dir_all(&cal_dir)?;
                                    }

                                    info!(
                                        "Using calibration directory at \"{}\"",
                                        cal_dir.display()
                                    );

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

            // We get here iff DoQuit broke us out of infinite loop.

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
            if let Some((control, join_handle)) = cam.control_and_join_handle() {
                control.stop();
                while !control.is_done() {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                info!("camera thread stopped");
                join_handle.join().expect("join camera thread");
                info!("camera thread joined");
            } else {
                error!("camera thread not running!?");
            }

            info!("cam_args_rx future is resolved");
            Ok::<_, eyre::Report>(())
        }
    };

    let (launched_tx, mut launched_rx) = tokio::sync::watch::channel(());

    #[cfg(not(feature = "eframe-gui"))]
    let no_browser = args.no_browser;

    // Never launch browser automatically in GUI mode.
    #[cfg(feature = "eframe-gui")]
    let no_browser = true;

    if !no_browser {
        tokio::spawn(async move {
            // Let the webserver start before opening browser.
            launched_rx.changed().await.unwrap();
            open_browser(format!("{}", urls[0])).unwrap();
        });
    }

    let firehose_task_join_handle = tokio::spawn(async {
        // The first thing this task does is pop a frame from firehose_rx, so we
        // should ensure there is one present.
        video_streaming::firehose_task(rx_new_connection, firehose_rx, firehose_callback_rx)
            .await
            .unwrap();
    });

    debug!("  running forever");

    {
        // run LED Box stuff here

        use tokio_serial::SerialPortBuilderExt;
        use tokio_util::codec::Decoder;

        use json_lines::codec::JsonLinesCodec;
        use strand_led_box_comms::{ChannelState, DeviceState, OnState};

        let start_led_box_instant = std::time::Instant::now();

        // enqueue initial message
        {
            fn make_chan(num: u8, on_state: OnState) -> ChannelState {
                let intensity = strand_led_box_comms::MAX_INTENSITY;
                ChannelState {
                    num,
                    intensity,
                    on_state,
                }
            }

            let first_led_box_state = DeviceState {
                ch1: make_chan(1, OnState::Off),
                ch2: make_chan(2, OnState::Off),
                ch3: make_chan(3, OnState::Off),
                ch4: make_chan(4, OnState::Off),
            };

            led_box_tx_std
                .send(ToLedBoxDevice::DeviceState(first_led_box_state))
                .await
                .unwrap();
        }

        // open serial port
        let port = {
            let tracker = shared_store_arc.read().unwrap();
            let shared = tracker.as_ref();
            if let Some(serial_device) = shared.led_box_device_path.as_ref() {
                info!("opening LED box \"{}\"", serial_device);
                // open with default settings 9600 8N1
                #[allow(unused_mut)]
                let mut port = tokio_serial::new(serial_device, strand_led_box_comms::BAUD_RATE)
                    .open_native_async()
                    .unwrap();

                #[cfg(unix)]
                port.set_exclusive(false)
                    .expect("Unable to set serial port exclusive to false");
                Some(port)
            } else {
                None
            }
        };

        if let Some(port) = port {
            // wrap port with codec
            let (mut writer, mut reader) = JsonLinesCodec::default().framed(port).split();

            // Clear potential initially present bytes from stream...
            let _ = tokio::time::timeout(std::time::Duration::from_millis(50), reader.next()).await;

            writer
                .send(strand_led_box_comms::ToDevice::VersionRequest)
                .await?;

            match tokio::time::timeout(std::time::Duration::from_millis(50), reader.next()).await {
                Ok(Some(Ok(msg))) => match msg {
                    strand_led_box_comms::FromDevice::VersionResponse(
                        strand_led_box_comms::COMM_VERSION,
                    ) => {
                        info!(
                            "Connected to firmware version {}",
                            strand_led_box_comms::COMM_VERSION
                        );
                    }
                    msg => {
                        eyre::bail!("Unexpected response from LED Box {:?}. Is your firmware version correct? (Needed version: {})",
                            msg, strand_led_box_comms::COMM_VERSION);
                    }
                },
                Err(_elapsed) => {
                    eyre::bail!("Timeout connecting to LED Box. Is your firmware version correct? (Needed version: {})",
                        strand_led_box_comms::COMM_VERSION);
                }
                Ok(None) | Ok(Some(Err(_))) => {
                    eyre::bail!("Failed connecting to LED Box. Is your firmware version correct? (Needed version: {})",
                          strand_led_box_comms::COMM_VERSION);
                }
            }

            // handle messages from the device
            let from_device_task = async move {
                debug!("awaiting message from LED box");
                while let Some(msg) = tokio_stream::StreamExt::next(&mut reader).await {
                    match msg {
                        Ok(strand_led_box_comms::FromDevice::EchoResponse8(d)) => {
                            let buf = [d.0, d.1, d.2, d.3, d.4, d.5, d.6, d.7];
                            let sent_millis: u64 =
                                byteorder::ReadBytesExt::read_u64::<byteorder::LittleEndian>(
                                    &mut std::io::Cursor::new(buf),
                                )
                                .unwrap();

                            let now = start_led_box_instant.elapsed();
                            let now_millis: u64 =
                                (now.as_millis() % (u64::MAX as u128)).try_into().unwrap();
                            debug!("LED box round trip time: {} msec", now_millis - sent_millis);

                            // elsewhere check if this happens every LED_BOX_HEARTBEAT_INTERVAL_MSEC or so.
                            let mut led_box_heartbeat_update =
                                led_box_heartbeat_update_arc.write().unwrap();
                            *led_box_heartbeat_update = Some(std::time::Instant::now());
                        }
                        Ok(strand_led_box_comms::FromDevice::StateWasSet) => {}
                        Ok(msg) => {
                            todo!("Did not handle {:?}", msg);
                            // error!("unknown message received: {:?}", msg);
                        }
                        Err(e) => {
                            panic!("unexpected error: {e}: {e:?}");
                        }
                    }
                }
            };
            tokio::spawn(from_device_task); // todo: keep join handle

            // handle messages to the device
            let to_device_task = async move {
                while let Some(msg) = led_box_rx.recv().await {
                    // send message to device
                    writer.send(msg).await.unwrap();
                    // copy new device state and store it to our cache
                    if let ToLedBoxDevice::DeviceState(new_state) = msg {
                        let mut tracker = shared_store_arc.write().unwrap();
                        tracker.modify(|shared| {
                            shared.led_box_device_state = Some(new_state);
                        })
                    };
                }
            };
            tokio::spawn(to_device_task); // todo: keep join handle

            // heartbeat task
            let heartbeat_task = async move {
                let mut interval_stream = tokio::time::interval(std::time::Duration::from_millis(
                    LED_BOX_HEARTBEAT_INTERVAL_MSEC,
                ));
                loop {
                    interval_stream.tick().await;

                    let now = start_led_box_instant.elapsed();
                    let now_millis: u64 =
                        (now.as_millis() % (u64::MAX as u128)).try_into().unwrap();
                    let mut d = vec![];
                    {
                        use byteorder::WriteBytesExt;
                        d.write_u64::<byteorder::LittleEndian>(now_millis).unwrap();
                    }
                    let msg = ToLedBoxDevice::EchoRequest8((
                        d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7],
                    ));
                    debug!("sending: {:?}", msg);

                    led_box_tx_std.send(msg).await.unwrap();
                }
            };
            tokio::spawn(heartbeat_task); // todo: keep join handle
        }
    }
    // _dummy_tx is not dropped until after `select!` below. It will never send.
    let (_dummy_tx, dummy_rx) = tokio::sync::mpsc::channel(1);
    let mut quit_rx = match quit_rx {
        None => dummy_rx,
        Some(fut) => fut,
    };

    // Now run until first future returns, then exit.
    info!("Strand Cam launched.");
    launched_tx.send(())?;
    tokio::select! {
        res = http_serve_future => {res?},
        res = cam_arg_future => {res?},
        _ = mainbrain_transmitter_fut => {},
        _ = send_updates_future => {},
        res = frame_process_task_fut => {res?},
        res = firehose_task_join_handle => {res?},
        _ = quit_rx.recv() => {},
    }
    info!("Strand Cam ending nicely. :)");

    Ok(mymod)
}

fn measure_times<C>(cam: &C) -> Result<(chrono::DateTime<chrono::Utc>, i64)>
where
    C: ci2::Camera,
{
    let start = chrono::Utc::now();
    cam.command_execute("TimestampLatch", true)?;
    let remote = cam.feature_int("TimestampLatchValue")?;
    let stop = chrono::Utc::now();
    let max_err = stop - start;
    // assume symmetric delay
    let remote_offset_symmetric = max_err / 2;
    let remote_in_local = start + remote_offset_symmetric;
    tracing::debug!("Camera timestamp: {remote_in_local} {remote} {max_err}.");
    Ok((remote_in_local, remote))
}

fn open_browser(url: String) -> Result<()> {
    // Spawn a new thread because xdg-open blocks forever
    // if it must open a new browser.
    std::thread::Builder::new()
        .name("browser opener".to_string())
        .spawn(move || {
            // ignore browser
            info!("Opening browser at {}", url);
            match webbrowser::open(&url) {
                Ok(_) => trace!("Browser opened"),
                Err(e) => error!("Error opening brower: {:?}", e),
            };
            debug!("browser thread done {}:{}", file!(), line!());
        })?;
    Ok(())
}

async fn send_cam_settings_to_braid(
    cam_settings: &str,
    transmit_msg_tx: &tokio::sync::mpsc::Sender<braid_types::BraidHttpApiCallback>,
    current_cam_settings_extension: &str,
    raw_cam_name: &RawCamName,
) -> StdResult<(), tokio::sync::mpsc::error::SendError<braid_types::BraidHttpApiCallback>> {
    let current_cam_settings_buf = cam_settings.to_string();
    let current_cam_settings_extension = current_cam_settings_extension.to_string();
    let raw_cam_name = raw_cam_name.clone();
    let transmit_msg_tx = transmit_msg_tx.clone();

    let msg = braid_types::BraidHttpApiCallback::UpdateCamSettings(braid_types::PerCam {
        raw_cam_name,
        inner: braid_types::UpdateCamSettings {
            current_cam_settings_buf,
            current_cam_settings_extension,
        },
    });
    transmit_msg_tx.send(msg).await
}

fn bitrate_to_u32(br: &strand_cam_remote_control::BitrateSelection) -> Option<u32> {
    use strand_cam_remote_control::BitrateSelection::*;
    Some(match br {
        Bitrate500 => 500,
        Bitrate1000 => 1000,
        Bitrate2000 => 2000,
        Bitrate3000 => 3000,
        Bitrate4000 => 4000,
        Bitrate5000 => 5000,
        Bitrate10000 => 10000,
        BitrateUnlimited => return None,
    })
}

struct FinalMp4RecordingConfig {
    final_cfg: strand_cam_remote_control::RecordingConfig,
}

impl FinalMp4RecordingConfig {
    fn new(shared: &StoreType, creation_time: chrono::DateTime<chrono::Local>) -> Self {
        let mp4_codec = match shared.mp4_codec {
            CodecSelection::H264Nvenc => {
                let cuda_device = shared
                    .cuda_devices
                    .iter()
                    .position(|x| x == &shared.mp4_cuda_device)
                    .unwrap_or(0);
                let cuda_device = cuda_device.try_into().unwrap();
                Some(Mp4Codec::H264NvEnc(NvidiaH264Options {
                    bitrate: bitrate_to_u32(&shared.mp4_bitrate),
                    cuda_device,
                }))
            }
            CodecSelection::H264OpenH264 => {
                let preset = strand_cam_remote_control::OpenH264Preset::AllFrames;
                if shared.mp4_bitrate
                    != strand_cam_remote_control::BitrateSelection::BitrateUnlimited
                {
                    warn!("ignoring mp4 bitrate with OpenH264 codec");
                }
                Some(Mp4Codec::H264OpenH264(
                    strand_cam_remote_control::OpenH264Options {
                        debug: false,
                        preset,
                    },
                ))
            }
            _ => None,
        };
        let h264_metadata = {
            let mut h264_metadata =
                strand_cam_remote_control::H264Metadata::new("strand-cam", creation_time.into());
            h264_metadata.camera_name = Some(shared.camera_name.clone());
            h264_metadata.gamma = shared.camera_gamma;
            Some(h264_metadata)
        };
        let final_cfg = if let Some(codec) = mp4_codec {
            let final_cfg = Mp4RecordingConfig {
                codec,
                max_framerate: shared.mp4_max_framerate.clone(),
                h264_metadata,
            };
            strand_cam_remote_control::RecordingConfig::Mp4(final_cfg)
        } else {
            use strand_cam_remote_control::CodecSelection::*;
            let codec = match &shared.mp4_codec {
                H264Nvenc | H264OpenH264 => {
                    unreachable!();
                }
                Ffmpeg(args) => args.clone(),
            };
            strand_cam_remote_control::RecordingConfig::Ffmpeg(FfmpegRecordingConfig {
                codec_args: codec,
                max_framerate: shared.mp4_max_framerate.clone(),
                h264_metadata,
            })
        };
        FinalMp4RecordingConfig { final_cfg }
    }
}

fn to_eyre<T>(e: SendError<T>) -> eyre::Report {
    eyre!("SendError: {e} {e:?}")
}
