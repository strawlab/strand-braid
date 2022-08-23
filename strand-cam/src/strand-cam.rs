// TODO: if camera not available, launch alternate UI indicating such and
// waiting for it to become available?

// TODO: add quit app button to UI.

// TODO: UI automatically reconnect to app after app restart.

#![cfg_attr(
    feature = "backtrace",
    feature(error_generic_member_access, provide_any)
)]

#[cfg(feature = "backtrace")]
use std::backtrace::Backtrace;

#[macro_use]
extern crate log;

use anyhow::Context;

#[cfg(feature = "fiducial")]
use ads_apriltag as apriltag;

use http_video_streaming as video_streaming;
use machine_vision_formats as formats;

#[cfg(feature = "flydratrax")]
use nalgebra as na;

#[cfg(feature = "fiducial")]
use libflate::finish::AutoFinishUnchecked;
#[cfg(feature = "fiducial")]
use libflate::gzip::Encoder;

use futures::{channel::mpsc, sink::SinkExt, stream::StreamExt};
use serde::{Deserialize, Serialize};

use hyper_tls::HttpsConnector;
#[allow(unused_imports)]
use preferences::{AppInfo, Preferences};

use ci2::{Camera, CameraInfo, CameraModule};
use ci2_async::AsyncCamera;
use fmf::FMFWriter;

use async_change_tracker::ChangeTracker;
use basic_frame::{match_all_dynamic_fmts, DynamicFrame};
use formats::PixFmt;
use timestamped_frame::ExtraTimeData;

use bui_backend::highlevel::{create_bui_app_inner, BuiAppInner};
use bui_backend::{AccessControl, CallbackHandler};
use bui_backend_types::CallbackDataAndSession;

#[cfg(feature = "flydratrax")]
use http_video_streaming_types::{DrawableShape, StrokeStyle};

use video_streaming::{AnnotatedFrame, FirehoseCallback};

use std::{error::Error as StdError, future::Future, path::Path, pin::Pin};

#[cfg(feature = "flydra_feat_detect")]
use ci2_remote_control::CsvSaveConfig;
use ci2_remote_control::{CamArg, MkvRecordingConfig, RecordingFrameRate};
use flydra_types::{
    BuiServerInfo, CamHttpServerInfo, MainbrainBuiLocation, RawCamName, RealtimePointsDestAddr,
    RosCamName, StartSoftwareFrameRateLimit,
};

use flydra_feature_detector_types::ImPtDetectCfg;

#[cfg(feature = "flydra_feat_detect")]
use flydra_feature_detector::{FlydraFeatureDetector, UfmfState};

#[cfg(feature = "flydra_feat_detect")]
use strand_cam_csv_config_types::CameraCfgFview2_0_26;
#[cfg(feature = "flydra_feat_detect")]
use strand_cam_csv_config_types::{FullCfgFview2_0_26, SaveCfgFview2_0_25};

#[cfg(feature = "fiducial")]
use strand_cam_storetype::ApriltagState;
use strand_cam_storetype::ToLedBoxDevice;
use strand_cam_storetype::{CallbackType, ImOpsState, RangedValue, StoreType};

use strand_cam_storetype::{KalmanTrackingConfig, LedProgramConfig};

#[cfg(feature = "flydratrax")]
use flydra_types::{FlydraFloatTimestampLocal, HostClock, SyncFno, Triggerbox};

#[cfg(feature = "flydratrax")]
use strand_cam_pseudo_cal::PseudoCameraCalibrationData;

use rust_cam_bui_types::RecordingPath;

use parking_lot::RwLock;
use std::fs::File;
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::Arc;

/// default strand-cam HTTP port when not running in Braid.
const DEFAULT_HTTP_ADDR: &str = "127.0.0.1:3440";

pub const DEBUG_ADDR_DEFAULT: &str = "127.0.0.1:8877";

pub const APP_INFO: AppInfo = AppInfo {
    name: "strand-cam",
    author: "AndrewStraw",
};

#[cfg(all(
    not(feature = "flydra_feat_detect"),
    feature = "flydra-feature-detector"
))]
compile_error!("do not enable 'flydra-feature-detector' except with 'flydra_feat_detect' feature");

#[cfg(feature = "flydratrax")]
use flydra2::{CoordProcessor, CoordProcessorConfig, CoordProcessorControl, MyFloat, StreamItem};

#[cfg(feature = "imtrack-absdiff")]
pub use flydra_pt_detect_cfg::default_absdiff as default_im_pt_detect;
#[cfg(feature = "imtrack-dark-circle")]
pub use flydra_pt_detect_cfg::default_dark_circle as default_im_pt_detect;

include!(concat!(env!("OUT_DIR"), "/frontend.rs")); // Despite slash, this does work on Windows.

#[cfg(feature = "flydratrax")]
const KALMAN_TRACKING_PREFS_KEY: &'static str = "kalman-tracking";

#[cfg(feature = "flydratrax")]
const LED_PROGRAM_PREFS_KEY: &'static str = "led-config";

#[cfg(feature = "flydratrax")]
mod flydratrax_handle_msg;

mod datagram_socket;
mod post_trigger_buffer;
use datagram_socket::DatagramSocket;

pub mod cli_app;

const LED_BOX_HEARTBEAT_INTERVAL_MSEC: u64 = 5000;

pub type Result<M> = std::result::Result<M, StrandCamError>;

#[derive(Debug, thiserror::Error)]
pub enum StrandCamError {
    // #[error("other error")]
    // OtherError,
    #[error("string error: {0}")]
    StringError(String),
    #[error("error: {0}")]
    AnyhowError(#[from] anyhow::Error),
    #[error("no cameras found")]
    NoCamerasFound,
    #[error("IncompleteSend")]
    IncompleteSend(#[cfg(feature = "backtrace")] Backtrace),
    #[error("unix domain sockets not supported")]
    UnixDomainSocketsNotSupported(#[cfg(feature = "backtrace")] Backtrace),
    #[error("conversion to socket address failed")]
    SocketAddressConversionFailed(#[cfg(feature = "backtrace")] Backtrace),
    #[error("ConvertImageError: {0}")]
    ConvertImageError(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        convert_image::Error,
    ),
    #[error("FMF error: {0}")]
    FMFError(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        fmf::FMFError,
    ),
    #[error("UFMF error: {0}")]
    UFMFError(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        ufmf::UFMFError,
    ),
    #[error("io error: {source}")]
    IoError {
        #[from]
        source: std::io::Error,
        #[cfg(feature = "backtrace")]
        backtrace: std::backtrace::Backtrace,
    },
    #[error("try send error")]
    TrySendError,
    #[error("BUI backend error: {0}")]
    BuiBackendError(#[from] bui_backend::Error),
    #[error("ci2 error: {0}")]
    Ci2Error(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        ci2::Error,
    ),
    #[error("plugin disconnected")]
    PluginDisconnected,
    #[error("video streaming error")]
    VideoStreamingError(#[from] video_streaming::Error),
    #[error(
        "The --jwt-secret argument must be passed or the JWT_SECRET environment \
                  variable must be set."
    )]
    JwtError,
    #[cfg(feature = "flydratrax")]
    #[error("MVG error: {0}")]
    MvgError(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        mvg::MvgError,
    ),
    #[error("{0}")]
    MkvWriterError(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        mkv_writer::Error,
    ),
    #[error("{0}")]
    AddrParseError(#[from] std::net::AddrParseError),
    #[error("background movie writer error: {0}")]
    BgMovieWriterError(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        bg_movie_writer::Error,
    ),
    #[error("Braid update image listener disconnected")]
    BraidUpdateImageListenerDisconnected,
    #[error("{0}")]
    NvEncError(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        nvenc::NvEncError,
    ),
    #[cfg(feature = "flydratrax")]
    #[error("flydra2 error: {0}")]
    Flydra2Error(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        flydra2::Error,
    ),
    #[error("futures mpsc send error: {0}")]
    FuturesChannelMpscSend(#[from] futures::channel::mpsc::SendError),
    #[error("SendMsgErr")]
    SendMsgErr {
        #[cfg(feature = "backtrace")]
        backtrace: std::backtrace::Backtrace,
    },
    #[cfg(feature = "fiducial")]
    #[error("{0}")]
    CsvError(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        csv::Error,
    ),
    #[error("thread done")]
    ThreadDone,

    #[error("{0}")]
    SerialportError(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        tokio_serial::Error,
    ),

    #[error("A camera name is required")]
    CameraNameRequired,
    #[error("hyper error: {0}")]
    HyperError(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        hyper::Error,
    ),
    #[error("tokio::task::JoinError: {0}")]
    TokioTaskJoinError(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        tokio::task::JoinError,
    ),
}

impl<T> From<tokio::sync::mpsc::error::SendError<T>> for StrandCamError {
    fn from(_orig: tokio::sync::mpsc::error::SendError<T>) -> StrandCamError {
        StrandCamError::SendMsgErr {
            #[cfg(feature = "backtrace")]
            backtrace: std::backtrace::Backtrace::capture(),
        }
    }
}

#[cfg(feature = "plugin-process-frame")]
struct CloseAppOnThreadExit {
    file: &'static str,
    line: u32,
    thread_handle: std::thread::Thread,
    sender: Option<tokio::sync::mpsc::Sender<CamArg>>,
}

#[cfg(feature = "plugin-process-frame")]
impl CloseAppOnThreadExit {
    pub fn new(sender: tokio::sync::mpsc::Sender<CamArg>, file: &'static str, line: u32) -> Self {
        let thread_handle = std::thread::current();
        Self {
            sender: Some(sender),
            file,
            line,
            thread_handle,
        }
    }

    fn check<T, E>(&self, result: std::result::Result<T, E>) -> T
    where
        E: std::convert::Into<anyhow::Error>,
    {
        match result {
            Ok(v) => v,
            Err(e) => self.fail(e.into()),
        }
    }

    fn fail(&self, e: anyhow::Error) -> ! {
        display_err(
            e,
            self.file,
            self.line,
            self.thread_handle.name(),
            self.thread_handle.id(),
        );
        panic!(
            "panicing thread {:?} due to error",
            self.thread_handle.name()
        );
    }

    fn success(mut self) {
        self.sender.take();
    }
}

#[cfg(feature = "plugin-process-frame")]
fn display_err(
    err: anyhow::Error,
    file: &str,
    line: u32,
    thread_name: Option<&str>,
    thread_id: std::thread::ThreadId,
) {
    eprintln!(
        "Error {}:{} ({:?} Thread name {:?}): {}",
        file, line, thread_id, thread_name, err
    );
    eprintln!("Alternate view of error:",);
    eprintln!("{:#?}", err,);
    eprintln!("Debug view of error:",);
    eprintln!("{:?}", err,);
}

#[cfg(feature = "plugin-process-frame")]
impl Drop for CloseAppOnThreadExit {
    fn drop(&mut self) {
        if let Some(sender) = self.sender.take() {
            debug!(
                "*** dropping in thread {:?} {}:{}",
                self.thread_handle.name(),
                self.file,
                self.line
            );
            match sender.blocking_send(CamArg::DoQuit) {
                Ok(()) => {}
                Err(e) => {
                    error!("failed sending quit command: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }
}

pub(crate) enum Msg {
    StartMkv((String, MkvRecordingConfig)),
    StopMkv,
    StartFMF((String, RecordingFrameRate)),
    StopFMF,
    #[cfg(feature = "flydra_feat_detect")]
    StartUFMF(String),
    #[cfg(feature = "flydra_feat_detect")]
    StopUFMF,
    #[cfg(feature = "flydra_feat_detect")]
    SetTracking(bool),
    PostTriggerStartMkv((String, MkvRecordingConfig)),
    SetPostTriggerBufferSize(usize),
    Mframe(DynamicFrame),
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
    SetClockModel(Option<rust_cam_bui_types::ClockModel>),
    QuitFrameProcessThread,
    StartAprilTagRec(String),
    StopAprilTagRec,
}

impl std::fmt::Debug for Msg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        write!(f, "strand_cam::Msg{{..}}")
    }
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub enum FrameProcessingErrorState {
    NotifyAll,
    IgnoreUntil(chrono::DateTime<chrono::Utc>),
    IgnoreAll,
}

impl Default for FrameProcessingErrorState {
    fn default() -> Self {
        FrameProcessingErrorState::NotifyAll
    }
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
    pub fn update(&mut self, fi: &ci2::FrameInfo) -> Option<f64> {
        let fno = fi.host_framenumber;
        let stamp = fi.host_timestamp;
        let mut reset_previous = true;
        let mut result = None;
        if let Some((prev_frame, ref prev_stamp)) = self.prev {
            let n_frames = fno - prev_frame;
            if n_frames < self.frames_to_average {
                reset_previous = false;
            } else {
                let dur_nsec = stamp
                    .clone()
                    .signed_duration_since(prev_stamp.clone())
                    .num_nanoseconds();
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

#[derive(Serialize, Deserialize, Debug, Clone)]
struct AprilConfig {
    created_at: chrono::DateTime<chrono::Local>,
    camera_name: String,
    camera_width_pixels: usize,
    camera_height_pixels: usize,
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

#[cfg(feature = "flydratrax")]
struct FlydraConfigState {
    region: video_streaming::Shape,
    kalman_tracking_config: KalmanTrackingConfig,
}

#[cfg(feature = "checkercal")]
type CollectedCornersArc = Arc<RwLock<Vec<Vec<(f32, f32)>>>>;

async fn register_node_and_update_image(
    api_http_address: flydra_types::MainbrainBuiLocation,
    msg: flydra_types::RegisterNewCamera,
    mut transmit_msg_rx: mpsc::Receiver<flydra_types::HttpApiCallback>,
) -> Result<()> {
    let mut mainbrain_session =
        braid_http_session::mainbrain_future_session(api_http_address).await?;
    mainbrain_session.register_flydra_camnode(&msg).await?;
    while let Some(msg) = transmit_msg_rx.next().await {
        mainbrain_session.send_message(msg).await?;
    }
    Ok(())
}

async fn convert_stream(
    ros_cam_name: RosCamName,
    mut transmit_feature_detect_settings_rx: tokio::sync::mpsc::Receiver<
        flydra_feature_detector_types::ImPtDetectCfg,
    >,
    mut transmit_msg_tx: mpsc::Sender<flydra_types::HttpApiCallback>,
) -> Result<()> {
    while let Some(val) = transmit_feature_detect_settings_rx.recv().await {
        let msg =
            flydra_types::HttpApiCallback::UpdateFeatureDetectSettings(flydra_types::PerCam {
                ros_cam_name: ros_cam_name.clone(),
                inner: flydra_types::UpdateFeatureDetectSettings {
                    current_feature_detect_settings: val,
                },
            });
        transmit_msg_tx.send(msg).await?;
    }
    Ok(())
}

struct MainbrainInfo {
    mainbrain_internal_addr: MainbrainBuiLocation,
    transmit_msg_rx: mpsc::Receiver<flydra_types::HttpApiCallback>,
    transmit_msg_tx: mpsc::Sender<flydra_types::HttpApiCallback>,
}

// We perform image analysis in its own task.
async fn frame_process_task(
    my_runtime: tokio::runtime::Handle,
    #[cfg(feature = "flydratrax")] flydratrax_model_server: (
        tokio::sync::mpsc::Sender<(flydra2::SendType, flydra2::TimeDataPassthrough)>,
        flydra2::ModelServer,
    ),
    #[cfg(feature = "flydratrax")] flydratrax_calibration_source: CalSource,
    cam_name: RawCamName,
    #[cfg(feature = "flydra_feat_detect")] camera_cfg: CameraCfgFview2_0_26,
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
    #[cfg(feature = "flydratrax")] led_box_tx_std: tokio::sync::mpsc::Sender<ToLedBoxDevice>,
    mut quit_rx: tokio::sync::oneshot::Receiver<()>,
    is_starting_tx: tokio::sync::oneshot::Sender<()>,
    #[cfg(feature = "flydratrax")] http_camserver_info: BuiServerInfo,
    process_frame_priority: Option<(i32, i32)>,
    mainbrain_info: Option<MainbrainInfo>,
    camdata_addr: Option<RealtimePointsDestAddr>,
    led_box_heartbeat_update_arc: Arc<RwLock<Option<std::time::Instant>>>,
    #[cfg(feature = "plugin-process-frame")] do_process_frame_callback: bool,
    #[cfg(feature = "checkercal")] collected_corners_arc: CollectedCornersArc,
    #[cfg(feature = "flydratrax")] save_empty_data2d: SaveEmptyData2dType,
    #[cfg(feature = "flydratrax")] valve: stream_cancel::Valve,
    #[cfg(feature = "flydra_feat_detect")] acquisition_duration_allowed_imprecision_msec: Option<
        f64,
    >,
    new_cam_data: flydra_types::RegisterNewCamera,
    frame_info_extractor: &dyn ci2::ExtractFrameInfo,
    #[cfg(feature = "flydra_feat_detect")] app_name: &'static str,
) -> anyhow::Result<()> {
    let is_braid = camdata_addr.is_some();

    let ros_cam_name: RosCamName = new_cam_data.ros_cam_name.clone();

    #[cfg(feature = "posix_sched_fifo")]
    {
        if let Some((policy, priority)) = process_frame_priority {
            posix_scheduler::sched_setscheduler(0, policy, priority)?;
            info!(
                "Frame processing thread called POSIX sched_setscheduler() \
                with policy {} priority {}",
                policy, priority
            );
        } else {
            info!(
                "Frame processing thread using \
                default posix scheduler settings."
            );
        }
    }

    #[cfg(not(feature = "posix_sched_fifo"))]
    {
        if process_frame_priority.is_some() {
            panic!(
                "Cannot set process frame priority because no support
                was compiled in."
            );
        } else {
            info!("Frame processing thread not configured to set posix scheduler.");
        }
    }

    #[cfg(feature = "flydratrax")]
    let mut maybe_flydra2_stream = None;
    #[cfg(feature = "flydratrax")]
    let mut maybe_flydra2_write_control = None;

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
    let mut my_mkv_writer: Option<bg_movie_writer::BgMovieWriter> = None;
    let mut fmf_writer: Option<FmfWriteInfo<_>> = None;
    #[cfg(feature = "flydra_feat_detect")]
    let mut ufmf_state: Option<UfmfState> = Some(UfmfState::Stopped);
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

    let (transmit_feature_detect_settings_tx, transmit_msg_tx) = if let Some(info) = mainbrain_info
    {
        let addr = info.mainbrain_internal_addr;
        let transmit_msg_tx = info.transmit_msg_tx.clone();

        let (transmit_feature_detect_settings_tx, transmit_feature_detect_settings_rx) =
            tokio::sync::mpsc::channel::<flydra_feature_detector_types::ImPtDetectCfg>(10);

        my_runtime.spawn(convert_stream(
            ros_cam_name.clone(),
            transmit_feature_detect_settings_rx,
            transmit_msg_tx.clone(),
        ));

        let transmit_msg_rx = info.transmit_msg_rx;
        my_runtime.spawn(register_node_and_update_image(
            addr,
            new_cam_data,
            // current_image_png,
            transmit_msg_rx,
        ));

        (
            Some(transmit_feature_detect_settings_tx),
            Some(info.transmit_msg_tx),
        )
    } else {
        (None, None)
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
    let mut im_tracker = FlydraFeatureDetector::new(
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
    let mut shared_store_arc: Option<Arc<RwLock<ChangeTracker<StoreType>>>> = None;
    let mut fps_calc = FpsCalc::new(100); // average 100 frames to get mean fps
    #[cfg(feature = "flydratrax")]
    let mut kalman_tracking_config = KalmanTrackingConfig::default(); // this is replaced below
    #[cfg(feature = "flydratrax")]
    let mut led_program_config;
    #[cfg(feature = "flydratrax")]
    let mut led_state = false;
    #[cfg(feature = "flydratrax")]
    let mut current_flydra_config_state: Option<FlydraConfigState> = None;
    #[cfg(feature = "flydratrax")]
    let mut dirty_flydra = false;
    #[cfg(feature = "flydratrax")]
    let mut current_led_program_config_state: Option<LedProgramConfig> = None;
    #[cfg(feature = "flydratrax")]
    let mut dirty_led_program = false;

    #[cfg(feature = "flydratrax")]
    let red_style = StrokeStyle::from_rgb(255, 100, 100);

    let expected_framerate_arc = Arc::new(RwLock::new(None));

    is_starting_tx.send(()).ok(); // signal that we are we are no longer starting

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

    let current_image_timer_arc = Arc::new(RwLock::new(std::time::Instant::now()));

    let mut im_ops_socket: Option<std::net::UdpSocket> = None;

    let mut opt_clock_model = None;
    let mut opt_frame_offset = None;

    while quit_rx.try_recv() == Err(tokio::sync::oneshot::error::TryRecvError::Empty) {
        #[cfg(feature = "flydra_feat_detect")]
        {
            if let Some(ref ssa) = shared_store_arc {
                match ssa.try_read() {
                    Some(store) => {
                        let tracker = store.as_ref();
                        is_doing_object_detection = tracker.is_doing_object_detection;
                        // make copy. TODO only copy on change.
                    }
                    None => {}
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

            if dirty_led_program {
                current_led_program_config_state = None;
                dirty_led_program = false;
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
                        let tracker = ssa.write();
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
                                let recon = match &flydratrax_calibration_source {
                                    CalSource::PseudoCal => {
                                        let cal_data = PseudoCameraCalibrationData {
                                            cam_name: cam_name.clone(),
                                            width,
                                            height,
                                            physical_diameter_meters: kalman_tracking_config
                                                .arena_diameter_meters,
                                            image_circle: circ,
                                        };
                                        cal_data.to_camera_system()?
                                    }
                                    CalSource::XmlFile(cal_fname) => {
                                        let rdr = std::fs::File::open(&cal_fname)?;
                                        flydra_mvg::FlydraMultiCameraSystem::from_flydra_xml(rdr)?
                                    }
                                    CalSource::PymvgJsonFile(cal_fname) => {
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
                                    flydratrax_handle_msg::create_message_handler(
                                        cam_cal,
                                        model_receiver,
                                        &mut led_state,
                                        ssa2,
                                        led_box_tx_std2,
                                    )
                                    .await
                                    .map_err(|e| anyhow::Error::new(Box::new(e)))
                                    .unwrap();
                                };
                                let msg_handler_jh = my_runtime.spawn(msg_handler_fut);

                                let expected_framerate_arc2 = expected_framerate_arc.clone();
                                let cam_name2 = cam_name.clone();
                                let http_camserver =
                                    CamHttpServerInfo::Server(http_camserver_info.clone());
                                let recon2 = recon.clone();
                                let flydratrax_model_server2 = flydratrax_model_server.clone();
                                let valve2 = valve.clone();

                                let cam_manager = flydra2::ConnectedCamerasManager::new_single_cam(
                                    &cam_name2,
                                    &http_camserver,
                                    &Some(recon2),
                                );
                                let tracking_params =
                                    flydra_types::default_tracking_params_flat_3d();
                                let ignore_latency = false;
                                let mut coord_processor = CoordProcessor::new(
                                    CoordProcessorConfig {
                                        tracking_params,
                                        save_empty_data2d,
                                        ignore_latency,
                                    },
                                    tokio::runtime::Handle::current(),
                                    cam_manager,
                                    Some(recon),
                                    "strand-cam",
                                    valve.clone(),
                                )
                                .expect("create CoordProcessor");

                                let braidz_write_tx = coord_processor.get_braidz_write_tx();
                                maybe_flydra2_write_control =
                                    Some(CoordProcessorControl::new(braidz_write_tx));

                                let (model_server_data_tx, _model_server) =
                                    flydratrax_model_server2;

                                coord_processor.add_listener(model_sender); // the local LED control thing
                                coord_processor.add_listener(model_server_data_tx); // the HTTP thing

                                let expected_framerate = *expected_framerate_arc2.read();
                                let flydra2_rx_valved = valve2.wrap(flydra2_rx);
                                let consume_future = coord_processor
                                    .consume_stream(flydra2_rx_valved, expected_framerate);

                                let flydra_jh = my_runtime.spawn(async {
                                    // Run until flydra is done.
                                    let jh = consume_future.await;

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
                if let Some(ref mut write_controller) = maybe_flydra2_write_control.as_mut() {
                    match write_controller.stop_saving_data().await {
                        Ok(()) => {}
                        Err(_) => {
                            log::info!("Channel to data writing task closed. Ending.");
                            break;
                        }
                    }
                }
                maybe_flydra2_write_control = None;
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
                        dirty_led_program = true;
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
                let f = std::fs::File::create(&path)?;
                fmf_writer = Some(FmfWriteInfo::new(FMFWriter::new(f)?, recording_framerate));
            }
            Msg::StartMkv((format_str_mkv, mkv_recording_config)) => {
                my_mkv_writer = Some(bg_movie_writer::BgMovieWriter::new_mkv_writer(
                    format_str_mkv,
                    mkv_recording_config,
                    100,
                ));
            }
            #[cfg(feature = "flydra_feat_detect")]
            Msg::StartUFMF(dest) => {
                ufmf_state = Some(UfmfState::Starting(dest));
            }
            Msg::PostTriggerStartMkv((format_str_mkv, mkv_recording_config)) => {
                let frames = post_trig_buffer.get_and_clear();
                let mut raw = bg_movie_writer::BgMovieWriter::new_mkv_writer(
                    format_str_mkv,
                    mkv_recording_config,
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
                my_mkv_writer = Some(raw);
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
                let opt_trigger_stamp = flydra_types::get_start_ts(
                    opt_clock_model.as_ref(),
                    opt_frame_offset.clone(),
                    extracted_frame_info.host_framenumber,
                );
                let save_mkv_fmf_stamp = if let Some(trigger_timestamp) = &opt_trigger_stamp {
                    trigger_timestamp.into()
                } else {
                    extracted_frame_info.host_timestamp
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
                                        x,
                                        y,
                                        center_x: store_cache_ref.im_ops_state.center_x,
                                        center_y: store_cache_ref.im_ops_state.center_y,
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
                                        match socket.send_to(
                                            &buf,
                                            &store_cache_ref.im_ops_state.destination,
                                        ) {
                                            Ok(_n_bytes) => {}
                                            Err(e) => {
                                                log::error!(
                                                    "Unable to send image moment data. {}",
                                                    e
                                                );
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

                    let device_timestamp = extracted_frame_info.device_timestamp;
                    let block_id = extracted_frame_info.frame_id;

                    #[cfg(not(feature = "flydra_feat_detect"))]
                    {
                        use flydra_types::{FlydraFloatTimestampLocal, ImageProcessingSteps};

                        // In case we are not doing flydra feature detection, send frame data to braid anyway.
                        let process_new_frame_start = chrono::Utc::now();
                        let acquire_stamp = FlydraFloatTimestampLocal::from_dt(
                            &extracted_frame_info.host_timestamp,
                        );

                        let preprocess_stamp =
                            datetime_conversion::datetime_to_f64(&process_new_frame_start);

                        let tracker_annotation = flydra_types::FlydraRawUdpPacket {
                            cam_name: ros_cam_name.as_str().to_string(),
                            timestamp: opt_trigger_stamp,
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
                                    opt_trigger_stamp,
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
                                    let trigger_timestamp =
                                        Some(FlydraFloatTimestampLocal::<Triggerbox>::from_f64(
                                            cam_received_timestamp,
                                        ));

                                    // This is not a lie.
                                    let cam_received_timestamp =
                                        FlydraFloatTimestampLocal::<HostClock>::from_f64(
                                            cam_received_timestamp,
                                        );

                                    let cam_num = 0.into(); // Only one camera, so this must be correct.
                                    let frame_data = flydra2::FrameData::new(
                                        ros_cam_name.clone(),
                                        cam_num,
                                        SyncFno(
                                            frame.extra().host_framenumber().try_into().unwrap(),
                                        ),
                                        trigger_timestamp,
                                        cam_received_timestamp,
                                        device_timestamp,
                                        block_id,
                                    );
                                    let fdp = flydra2::FrameDataAndPoints { frame_data, points };
                                    let si = StreamItem::Packet(fdp);

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
                                        let save_cfg = SaveCfgFview2_0_25 {
                                            name: app_name.to_string(),
                                            version: env!("CARGO_PKG_VERSION").to_string(),
                                            git_hash: env!("GIT_HASH").to_string(),
                                        };

                                        let object_detection_cfg = im_tracker.config();

                                        let full_cfg = FullCfgFview2_0_26 {
                                            app: save_cfg,
                                            camera: camera_cfg.clone(),
                                            created_at: local,
                                            csv_rate_limit: rate_limit,
                                            object_detection_cfg,
                                        };
                                        let cfg_yaml = serde_yaml::to_string(&full_cfg).unwrap();
                                        writeln!(fd, "# -- start of yaml config --")?;
                                        for line in cfg_yaml.lines() {
                                            writeln!(fd, "# {}", line)?;
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
                                            .checked_sub_signed(chrono::Duration::days(1))
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
                                    if interval >= inner.min_interval && points.len() >= 1 {
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
                                                        get_intensity(&device_state, 1)
                                                    );
                                                    led2 = format!(
                                                        "{}",
                                                        get_intensity(&device_state, 2)
                                                    );
                                                    led3 = format!(
                                                        "{}",
                                                        get_intensity(&device_state, 3)
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
                                                        format!("{:.3}", orientation_mod_pi)
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
                                        .and_then(|(slope, _ecc)| Some(f32::atan(slope as f32))),
                                    area: Some(pt.area as f32),
                                })
                                .collect();

                            all_points.extend(display_points);
                            blkajdsfads = Some(im_tracker.valid_region())
                        }
                    }
                    (all_points, blkajdsfads)
                };

                if let Some(ref mut inner) = my_mkv_writer {
                    let data = frame.clone(); // copy entire frame data
                    inner.write(data, save_mkv_fmf_stamp)?;
                }

                if let Some(ref mut inner) = fmf_writer {
                    // Based on our recording framerate, do we need to save this frame?
                    let do_save = match inner.last_saved_stamp {
                        None => true,
                        Some(stamp) => {
                            let elapsed = save_mkv_fmf_stamp - stamp;
                            elapsed
                                >= chrono::Duration::from_std(inner.recording_framerate.interval())?
                        }
                    };
                    if do_save {
                        match_all_dynamic_fmts!(&frame, x, {
                            inner.writer.write(x, save_mkv_fmf_stamp)?
                        });
                        inner.last_saved_stamp = Some(save_mkv_fmf_stamp);
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
                                        return Err(StrandCamError::PluginDisconnected.into());
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

                {
                    // send current image every 2 seconds
                    let mut timer = current_image_timer_arc.write();
                    let elapsed = timer.elapsed();
                    if elapsed > std::time::Duration::from_millis(2000) {
                        *timer = std::time::Instant::now();
                        // encode frame to png buf

                        if let Some(mut transmit_msg_tx) = transmit_msg_tx.clone() {
                            let ros_cam_name = ros_cam_name.clone();
                            let current_image_png = match_all_dynamic_fmts!(&frame, x, {
                                convert_image::frame_to_image(x, convert_image::ImageOptions::Png)
                                    .unwrap()
                            });

                            my_runtime.spawn(async move {
                                let msg = flydra_types::HttpApiCallback::UpdateCurrentImage(
                                    flydra_types::PerCam {
                                        ros_cam_name,
                                        inner: flydra_types::UpdateImage {
                                            current_image_png: current_image_png.into(),
                                        },
                                    },
                                );
                                transmit_msg_tx.send(msg).await.unwrap();
                            });
                        }
                    }
                }

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
                    vec![DrawableShape::from_shape(
                        &clpcs.led_on_shape_pixels,
                        &red_style,
                        1.0,
                    )]
                } else {
                    vec![]
                };

                #[cfg(not(feature = "flydratrax"))]
                let annotations = vec![];

                let name = None;
                if firehose_tx.capacity() == 0 {
                    debug!("cannot transmit frame for viewing: channel full");
                } else {
                    firehose_tx
                        .send(AnnotatedFrame {
                            frame,
                            found_points,
                            valid_display,
                            annotations,
                            name,
                        })
                        .await
                        .unwrap();
                }
            }
            #[cfg(feature = "flydra_feat_detect")]
            Msg::SetIsSavingObjDetectionCsv(new_value) => {
                info!(
                    "setting object detection CSV save state to: {:?}",
                    new_value
                );
                if let CsvSaveConfig::Saving(fps_limit) = new_value {
                    if !store_cache
                        .map(|s| s.is_doing_object_detection)
                        .unwrap_or(false)
                    {
                        error!("Not doing object detection, ignoring command to save data to CSV.");
                    } else {
                        csv_save_state = SavingState::Starting(fps_limit);

                        #[cfg(feature = "flydratrax")]
                        {
                            if let Some(ref mut write_controller) =
                                maybe_flydra2_write_control.as_mut()
                            {
                                let local: chrono::DateTime<chrono::Local> = chrono::Local::now();
                                let dirname = local.format("%Y%m%d_%H%M%S.braid").to_string();
                                let mut my_dir = csv_save_pathbuf.clone();
                                my_dir.push(dirname);

                                warn!("unimplemented setting of FPS and camera images");

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
                                write_controller.start_saving_data(cfg).await;
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
                        if let Some(ref mut write_controller) = maybe_flydra2_write_control.as_mut()
                        {
                            match write_controller.stop_saving_data().await {
                                Ok(()) => {}
                                Err(_) => {
                                    log::info!("Channel to data writing task closed. Ending.");
                                    break;
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
            Msg::SetClockModel(cm) => {
                opt_clock_model = cm;
            }
            Msg::StopMkv => {
                if let Some(mut inner) = my_mkv_writer.take() {
                    inner.finish()?;
                }
            }
            Msg::StopFMF => {
                fmf_writer = None;
            }
            #[cfg(feature = "flydra_feat_detect")]
            Msg::StopUFMF => {
                ufmf_state = Some(UfmfState::Stopped);
            }
            #[cfg(feature = "flydra_feat_detect")]
            Msg::SetTracking(value) => {
                is_doing_object_detection = value;
            }
            Msg::QuitFrameProcessThread => {
                break;
            }
        };
    }
    info!(
        "frame process thread done for camera '{}'",
        cam_name.as_str()
    );
    Ok(())
}

fn open_braid_destination_addr(dest_addr: &RealtimePointsDestAddr) -> Result<DatagramSocket> {
    info!("Sending detected coordinates to: {:?}", dest_addr);

    let timeout = std::time::Duration::new(0, 1);

    match dest_addr {
        #[cfg(feature = "flydra-uds")]
        &RealtimePointsDestAddr::UnixDomainSocket(ref uds) => {
            let socket = unix_socket::UnixDatagram::unbound()?;
            socket.set_write_timeout(Some(timeout))?;
            info!("UDS connecting to {:?}", uds.filename);
            socket.connect(&uds.filename)?;
            Ok(DatagramSocket::Uds(socket))
        }
        #[cfg(not(feature = "flydra-uds"))]
        &RealtimePointsDestAddr::UnixDomainSocket(ref _uds) => {
            Err(StrandCamError::UnixDomainSocketsNotSupported(
                #[cfg(feature = "backtrace")]
                Backtrace::capture(),
            ))
        }
        &RealtimePointsDestAddr::IpAddr(ref dest_ip_addr) => {
            let dest = format!("{}:{}", dest_ip_addr.ip(), dest_ip_addr.port());
            let mut dest_addrs: Vec<SocketAddr> = dest.to_socket_addrs()?.collect();

            if let Some(dest_sock_addr) = dest_addrs.pop() {
                // Let OS choose what port to use.
                let mut src_addr = dest_sock_addr;
                src_addr.set_port(0);
                if !dest_sock_addr.ip().is_loopback() {
                    // Let OS choose what IP to use, but preserve V4 or V6.
                    match src_addr {
                        SocketAddr::V4(_) => {
                            src_addr.set_ip(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)));
                        }
                        SocketAddr::V6(_) => {
                            src_addr.set_ip(IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 0)));
                        }
                    }
                }

                let sock = UdpSocket::bind(src_addr)?;
                sock.set_write_timeout(Some(timeout))?;
                debug!("UDP connecting to {}", dest);
                sock.connect(&dest)?;
                Ok(DatagramSocket::Udp(sock))
            } else {
                Err(StrandCamError::SocketAddressConversionFailed(
                    #[cfg(feature = "backtrace")]
                    Backtrace::capture(),
                ))
            }
        }
    }
}

#[cfg(feature = "flydra_feat_detect")]
fn get_intensity(device_state: &led_box_comms::DeviceState, chan_num: u8) -> u16 {
    let ch: &led_box_comms::ChannelState = match chan_num {
        1 => &device_state.ch1,
        2 => &device_state.ch2,
        3 => &device_state.ch3,
        c => panic!("unknown channel {}", c),
    };
    match ch.on_state {
        led_box_comms::OnState::Off => 0,
        led_box_comms::OnState::ConstantOn => ch.intensity,
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

impl<T: std::fmt::Debug> IgnoreSendError
    for std::result::Result<(), tokio::sync::mpsc::error::SendError<T>>
{
    fn ignore_send_error(self) {
        match self {
            Ok(()) => {}
            Err(e) => {
                log::debug!("Ignoring send error ({}:{}): {:?}", file!(), line!(), e)
            }
        }
    }
}

#[derive(Clone)]
struct MyCallbackHandler {
    firehose_callback_tx: tokio::sync::mpsc::Sender<FirehoseCallback>,
    cam_args_tx: tokio::sync::mpsc::Sender<CamArg>,
    led_box_tx_std: tokio::sync::mpsc::Sender<ToLedBoxDevice>,
    #[allow(dead_code)]
    tx_frame: tokio::sync::mpsc::Sender<Msg>,
}

impl CallbackHandler for MyCallbackHandler {
    type Data = CallbackType;

    /// HTTP request to "/callback" has been made with payload which as been
    /// deserialized into `Self::Data` and session data stored in
    /// [CallbackDataAndSession].
    fn call<'a>(
        &'a self,
        data_sess: CallbackDataAndSession<Self::Data>,
    ) -> Pin<Box<dyn Future<Output = std::result::Result<(), Box<dyn StdError + Send>>> + Send + 'a>>
    {
        let payload = data_sess.payload;
        let fut = async {
            match payload {
                CallbackType::ToCamera(cam_arg) => {
                    debug!("in cb: {:?}", cam_arg);
                    self.cam_args_tx.send(cam_arg).await.ignore_send_error();
                }
                CallbackType::FirehoseNotify(inner) => {
                    let arrival_time = chrono::Utc::now();
                    let fc = FirehoseCallback {
                        arrival_time,
                        inner,
                    };
                    self.firehose_callback_tx.send(fc).await.ignore_send_error();
                }
                CallbackType::TakeCurrentImageAsBackground => {
                    #[cfg(feature = "flydra_feat_detect")]
                    self.tx_frame
                        .send(Msg::TakeCurrentImageAsBackground)
                        .await
                        .ignore_send_error();
                }
                CallbackType::ClearBackground(value) => {
                    #[cfg(feature = "flydra_feat_detect")]
                    self.tx_frame
                        .send(Msg::ClearBackground(value))
                        .await
                        .ignore_send_error();
                    #[cfg(not(feature = "flydra_feat_detect"))]
                    let _ = value;
                }
                CallbackType::ToLedBox(led_box_arg) => futures::executor::block_on(async {
                    // todo: make this whole block async and remove the `futures::executor::block_on` aspect here.
                    info!("in led_box callback: {:?}", led_box_arg);
                    self.led_box_tx_std
                        .send(led_box_arg)
                        .await
                        .ignore_send_error();
                }),
            }
        };
        Box::pin(async {
            fut.await;
            Ok(())
        })
    }
}

pub struct StrandCamApp {
    inner: BuiAppInner<StoreType, CallbackType>,
}

impl StrandCamApp {
    async fn new(
        rt_handle: tokio::runtime::Handle,
        shared_store_arc: Arc<RwLock<ChangeTracker<StoreType>>>,
        secret: Option<Vec<u8>>,
        http_server_addr: &str,
        config: Config,
        cam_args_tx: tokio::sync::mpsc::Sender<CamArg>,
        led_box_tx_std: tokio::sync::mpsc::Sender<ToLedBoxDevice>,
        tx_frame: tokio::sync::mpsc::Sender<Msg>,
        shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    ) -> std::result::Result<
        (
            tokio::sync::mpsc::Receiver<FirehoseCallback>,
            Self,
            tokio::sync::mpsc::Receiver<bui_backend::highlevel::ConnectionEvent>,
        ),
        StrandCamError,
    > {
        let chan_size = 10;

        let addr: std::net::SocketAddr = http_server_addr.parse().unwrap();
        let auth = if let Some(ref secret) = secret {
            bui_backend::highlevel::generate_random_auth(addr, secret.clone())?
        } else if addr.ip().is_loopback() {
            AccessControl::Insecure(addr)
        } else {
            return Err(StrandCamError::JwtError);
        };

        // A channel for the data sent from the client browser.
        let (firehose_callback_tx, firehose_callback_rx) = tokio::sync::mpsc::channel(10);

        let callback_handler = Box::new(MyCallbackHandler {
            cam_args_tx,
            firehose_callback_tx,
            led_box_tx_std,
            tx_frame,
        });

        let (rx_conn, bui_server) = bui_backend::lowlevel::launcher(
            config.clone(),
            &auth,
            chan_size,
            strand_cam_storetype::STRAND_CAM_EVENTS_URL_PATH,
            None,
            callback_handler,
        );

        let (new_conn_rx, inner) = create_bui_app_inner(
            rt_handle.clone(),
            Some(shutdown_rx),
            &auth,
            shared_store_arc,
            Some(strand_cam_storetype::STRAND_CAM_EVENT_NAME.to_string()),
            rx_conn,
            bui_server,
        )
        .await?;

        // let mut new_conn_rx_valved = valve.wrap(new_conn_rx);
        // let new_conn_future = async move {
        //     while let Some(msg) = new_conn_rx_valved.next().await {
        //         connection_callback_tx.send(msg).await.unwrap();
        //     }
        //     debug!("new_conn_future closing {}:{}", file!(), line!());
        // };
        // let txers = Arc::new(RwLock::new(HashMap::new()));
        // let txers2 = txers.clone();
        // let mut new_conn_rx_valved = valve.wrap(new_conn_rx);
        // let new_conn_future = async move {
        //     while let Some(msg) = new_conn_rx_valved.next().await {
        //         let mut txers = txers2.write();
        //         match msg.typ {
        //             ConnectionEventType::Connect(chunk_sender) => {
        //                 txers.insert(
        //                     msg.connection_key,
        //                     (msg.session_key, chunk_sender, msg.path),
        //                 );
        //             }
        //             ConnectionEventType::Disconnect => {
        //                 txers.remove(&msg.connection_key);
        //             }
        //         }
        //     }
        //     debug!("new_conn_future closing {}:{}", file!(), line!());
        // };
        // let _task_join_handle = rt_handle.spawn(new_conn_future);

        let my_app = StrandCamApp { inner };

        Ok((firehose_callback_rx, my_app, new_conn_rx))
    }

    fn inner(&self) -> &BuiAppInner<StoreType, CallbackType> {
        &self.inner
    }
    // fn inner_mut(&mut self) -> &mut BuiAppInner<StoreType, CallbackType> {
    //     &mut self.inner
    // }
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
    use machine_vision_formats::{ImageData, Stride};
    match frame {
        DynamicFrame::Mono8(frame) => Some(apriltag::ImageU8Borrowed::new(
            frame.width().try_into().unwrap(),
            frame.height().try_into().unwrap(),
            frame.stride().try_into().unwrap(),
            frame.image_data(),
        )),
        _ => None,
    }
}

async fn check_version(
    client: hyper::Client<HttpsConnector<hyper::client::HttpConnector>>,
    known_version: Arc<RwLock<semver::Version>>,
    app_name: &'static str,
) -> hyper::Result<()> {
    let url = format!("https://version-check.strawlab.org/{}", app_name);
    let url = url.parse::<hyper::Uri>().unwrap();
    let agent = format!("{}/{}", app_name, *known_version.read());

    let req = hyper::Request::builder()
        .uri(&url)
        .header(hyper::header::USER_AGENT, agent.as_str())
        .body(hyper::body::Body::empty())
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

    // TODO: this is some ancient hyper and there must be easier ways to get
    // body into Vec<u8>.

    let (_parts, body) = res.into_parts();

    // convert stream of Result<Chunk> into future of Vec<Result<Chunk>>
    let data_fut = body.fold(vec![], |mut buf, result_chunk| async {
        buf.push(result_chunk);
        buf
    });

    // now in this future handle the payload
    let vec_res_chunk: Vec<hyper::Result<hyper::body::Bytes>> = data_fut.await;

    // move error to outer type
    let res_vec_chunk: hyper::Result<Vec<hyper::body::Bytes>> = vec_res_chunk.into_iter().collect();

    let chunks = res_vec_chunk?;

    let data: Vec<u8> = chunks.into_iter().fold(vec![], |mut buf, chunk| {
        // trace!("got chunk: {}", String::from_utf8_lossy(&chunk));
        buf.extend_from_slice(&*chunk);
        buf
    });
    let version: VersionResponse = match serde_json::from_slice(&data) {
        Ok(version) => version,
        Err(e) => {
            log::warn!("Could not parse version response JSON from {}: {}", url, e);
            return Ok(());
        }
    };
    let mut known_v = known_version3.write();
    if version.available > *known_v {
        info!(
            "New version of {} is available: {}. {}",
            app_name, version.available, version.message
        );
        *known_v = version.available;
    }

    Ok(())
}

fn get_mkv_writing_application(is_braid: bool) -> String {
    if is_braid {
        format!(
            "braid-{}-{}",
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION")
        )
    } else {
        format!("{}-{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
    }
}

fn display_qr_url(url: &str) {
    use qrcodegen::{QrCode, QrCodeEcc};
    use std::io::stdout;

    let qr = QrCode::encode_text(url, QrCodeEcc::Low).unwrap();

    let stdout = stdout();
    let mut stdout_handle = stdout.lock();
    writeln!(stdout_handle).expect("write failed");
    for y in 0..qr.size() {
        write!(stdout_handle, " ").expect("write failed");
        for x in 0..qr.size() {
            write!(
                stdout_handle,
                "{}",
                if qr.get_module(x, y) { "██" } else { "  " }
            )
            .expect("write failed");
        }
        writeln!(stdout_handle).expect("write failed");
    }
    writeln!(stdout_handle).expect("write failed");
}

#[derive(Debug)]
/// Defines whether runtime changes from the user are persisted to disk.
///
/// If they are persisted to disk, upon program re-start, the disk
/// is checked and preferences are loaded from there. If they cannot
/// be loaded, the defaults are used.
pub enum ImPtDetectCfgSource {
    ChangesNotSavedToDisk(ImPtDetectCfg),
    ChangedSavedToDisk((&'static AppInfo, String)),
}

#[cfg(feature = "plugin-process-frame")]
pub struct ProcessFrameCbData {
    pub func_ptr: plugin_defs::ProcessFrameFunc,
    pub data_handle: plugin_defs::DataHandle,
}

#[cfg(feature = "plugin-process-frame")]
impl std::fmt::Debug for ProcessFrameCbData {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "ProcessFrameCbData{{..}}")
    }
}

// Ideally it would just be DataHandle which we declare Send, but we cannot do
// that because it is just a type alias of "*mut c_void" which is defined
// elsewhere.
#[cfg(feature = "plugin-process-frame")]
unsafe impl Send for ProcessFrameCbData {}

#[allow(dead_code)]
#[cfg(not(feature = "plugin-process-frame"))]
struct ProcessFrameCbData {}

#[derive(Debug, Serialize, Deserialize)]
struct MomentCentroid {
    x: f32,
    y: f32,
    center_x: u32,
    center_y: u32,
}

#[derive(Debug, Serialize, Deserialize)]
enum ToDevice {
    Centroid(MomentCentroid),
}

// #[derive(Debug, Serialize, Deserialize)]
#[derive(Debug)]
pub struct StrandCamArgs {
    /// A handle to the tokio runtime.
    pub handle: Option<tokio::runtime::Handle>,
    /// Is Strand Cam running inside Braid context?
    pub is_braid: bool,
    pub secret: Option<Vec<u8>>,
    pub camera_name: Option<String>,
    pub pixel_format: Option<String>,
    pub http_server_addr: Option<String>,
    pub no_browser: bool,
    pub mkv_filename_template: String,
    pub fmf_filename_template: String,
    pub ufmf_filename_template: String,
    #[cfg(feature = "flydra_feat_detect")]
    pub tracker_cfg_src: ImPtDetectCfgSource,
    pub csv_save_dir: String,
    pub raise_grab_thread_priority: bool,
    #[cfg(feature = "posix_sched_fifo")]
    pub process_frame_priority: Option<(i32, i32)>,
    pub led_box_device_path: Option<String>,
    pub mainbrain_internal_addr: Option<MainbrainBuiLocation>,
    pub camdata_addr: Option<RealtimePointsDestAddr>,
    pub show_url: bool,
    #[cfg(feature = "plugin-process-frame")]
    pub process_frame_callback: Option<ProcessFrameCbData>,
    #[cfg(feature = "plugin-process-frame")]
    pub plugin_wait_dur: std::time::Duration,
    #[cfg(feature = "flydratrax")]
    pub save_empty_data2d: SaveEmptyData2dType,
    #[cfg(feature = "flydratrax")]
    pub model_server_addr: std::net::SocketAddr,
    #[cfg(feature = "flydratrax")]
    pub flydratrax_calibration_source: CalSource,
    #[cfg(feature = "fiducial")]
    pub apriltag_csv_filename_template: String,

    /// If set, camera acquisition will external trigger.
    pub force_camera_sync_mode: bool,

    /// If enabled, limit framerate (FPS) at startup.
    ///
    /// Despite the name ("software"), this actually sets the hardware
    /// acquisition rate via the `AcquisitionFrameRate` camera parameter.
    pub software_limit_framerate: StartSoftwareFrameRateLimit,

    /// Filename of vendor-specific camera settings file.
    pub camera_settings_filename: Option<std::path::PathBuf>,

    /// Threshold duration before logging error (msec).
    ///
    /// If the image acquisition timestamp precedes the computed trigger
    /// timestamp, clearly an error has happened. This error must lie in the
    /// computation of the trigger timestamp. This specifies the threshold error
    /// at which an error is logged. (The underlying source of such errors
    /// remains unknown.)
    pub acquisition_duration_allowed_imprecision_msec: Option<f64>,
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
            handle: None,
            is_braid: false,
            secret: None,
            camera_name: None,
            pixel_format: None,
            http_server_addr: None,
            no_browser: true,
            mkv_filename_template: "movie%Y%m%d_%H%M%S.%f_{CAMNAME}.mkv".to_string(),
            fmf_filename_template: "movie%Y%m%d_%H%M%S.%f_{CAMNAME}.fmf".to_string(),
            ufmf_filename_template: "movie%Y%m%d_%H%M%S.%f_{CAMNAME}.ufmf".to_string(),
            #[cfg(feature = "fiducial")]
            apriltag_csv_filename_template: strand_cam_storetype::APRILTAG_CSV_TEMPLATE_DEFAULT
                .to_string(),
            #[cfg(feature = "flydra_feat_detect")]
            tracker_cfg_src: ImPtDetectCfgSource::ChangesNotSavedToDisk(default_im_pt_detect()),
            csv_save_dir: "/dev/null".to_string(),
            raise_grab_thread_priority: false,
            #[cfg(feature = "posix_sched_fifo")]
            process_frame_priority: None,
            led_box_device_path: None,
            mainbrain_internal_addr: None,
            camdata_addr: None,
            show_url: true,
            #[cfg(feature = "plugin-process-frame")]
            process_frame_callback: None,
            #[cfg(feature = "plugin-process-frame")]
            plugin_wait_dur: std::time::Duration::from_millis(5),
            force_camera_sync_mode: false,
            software_limit_framerate: StartSoftwareFrameRateLimit::NoChange,
            camera_settings_filename: None,
            #[cfg(feature = "flydratrax")]
            flydratrax_calibration_source: CalSource::PseudoCal,
            #[cfg(feature = "flydratrax")]
            save_empty_data2d: true,
            #[cfg(feature = "flydratrax")]
            model_server_addr: flydra_types::DEFAULT_MODEL_SERVER_ADDR.parse().unwrap(),
            acquisition_duration_allowed_imprecision_msec:
                flydra_types::DEFAULT_ACQUISITION_DURATION_ALLOWED_IMPRECISION_MSEC,
        }
    }
}

fn test_nvenc_save(cfg: &MkvRecordingConfig, frame: DynamicFrame) -> Result<bool> {
    let mut nv_cfg_test = cfg.clone();

    let libs = match nvenc::Dynlibs::new() {
        Ok(libs) => libs,
        Err(e) => {
            debug!("nvidia NvEnc library could not be loaded: {:?}", e);
            return Ok(false);
        }
    };

    let opts = ci2_remote_control::H264Options {
        bitrate: 10000,
        ..Default::default()
    };

    nv_cfg_test.codec = ci2_remote_control::MkvCodec::H264(opts);

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

    let mut mkv_writer = mkv_writer::MkvWriter::new(&mut buf, nv_cfg_test.clone(), Some(nv_enc))?;
    mkv_writer.write_dynamic(&frame, chrono::Utc::now())?;
    mkv_writer.finish()?;

    debug!("MKV video with nvenc h264 encoding succeeded.");

    // When `buf` goes out of scope, it will be dropped.
    Ok(true)
}

pub fn run_app<M, C>(
    mymod: ci2_async::ThreadedAsyncCameraModule<M, C>,
    args: StrandCamArgs,
    app_name: &'static str,
) -> Result<()>
where
    M: ci2::CameraModule<CameraType = C>,
    C: 'static + ci2::Camera + Send,
{
    let handle = args
        .handle
        .clone()
        .ok_or_else(|| anyhow::anyhow!("no tokio runtime handle"))?;

    let my_handle = handle.clone();

    let (_bui_server_info, tx_cam_arg2, fut, _my_app) =
        handle.block_on(setup_app(mymod, my_handle, args, app_name))?;

    ctrlc::set_handler(move || {
        info!("got Ctrl-C, shutting down");

        // Send quit message.
        debug!("starting to send quit message {}:{}", file!(), line!());
        match tx_cam_arg2.blocking_send(CamArg::DoQuit) {
            Ok(()) => {}
            Err(e) => {
                error!("failed sending quit command: {}", e);
            }
        }
        debug!("done sending quit message {}:{}", file!(), line!());
    })
    .expect("Error setting Ctrl-C handler");

    handle.block_on(fut)?;

    info!("done");
    Ok(())
}

pub async fn setup_app<M, C>(
    mut mymod: ci2_async::ThreadedAsyncCameraModule<M, C>,
    rt_handle: tokio::runtime::Handle,
    args: StrandCamArgs,
    app_name: &'static str,
) -> anyhow::Result<(
    BuiServerInfo,
    tokio::sync::mpsc::Sender<CamArg>,
    impl futures::Future<Output = Result<()>>,
    StrandCamApp,
)>
where
    M: ci2::CameraModule<CameraType = C>,
    C: 'static + ci2::Camera + Send,
{
    let target_feature_string = target_features::target_features().join(", ");
    info!("Compiled with features: {}", target_feature_string);

    if !imops::COMPILED_WITH_SIMD_SUPPORT {
        warn!("Package 'imops' was not compiled with simd support. Image processing with imops will be slow.");
    }

    debug!("CLI request for camera {:?}", args.camera_name);

    // -----------------------------------------------

    info!("camera module: {}", mymod.name());

    let cam_infos = mymod.camera_infos()?;
    if cam_infos.is_empty() {
        return Err(StrandCamError::NoCamerasFound.into());
    }

    for cam_info in cam_infos.iter() {
        info!("  camera {:?} detected", cam_info.name());
    }

    let name = match args.camera_name {
        Some(ref name) => name,
        None => cam_infos[0].name(),
    };

    let frame_info_extractor = mymod.frame_info_extractor();
    let settings_file_ext = mymod.settings_file_extension().to_string();

    let mut cam = match mymod.threaded_async_camera(name) {
        Ok(cam) => cam,
        Err(e) => {
            let msg = format!("{}", e);
            error!("{}", msg);
            return Err(e.into());
        }
    };

    let raw_name = cam.name().to_string();
    info!("  got camera {}", raw_name);
    let cam_name = RawCamName::new(raw_name);
    let ros_cam_name = cam_name.to_ros();

    let camera_gamma = cam
        .feature_float("Gamma")
        .map_err(|e| log::warn!("Ignoring error getting gamma: {}", e))
        .ok();

    let (frame_rate_limit_supported, mut frame_rate_limit_enabled) =
        if let Some(camera_settings_filename) = &args.camera_settings_filename {
            let settings =
                std::fs::read_to_string(camera_settings_filename).with_context(|| {
                    format!(
                        "Failed to read camera settings from file \"{}\"",
                        camera_settings_filename.display()
                    )
                })?;

            cam.node_map_load(&settings)?;
            (false, false)
        } else {
            for pixfmt in cam.possible_pixel_formats()?.iter() {
                debug!("  possible pixel format: {}", pixfmt);
            }

            if let Some(ref pixfmt_str) = args.pixel_format {
                use std::str::FromStr;
                let pixfmt = PixFmt::from_str(pixfmt_str)
                    .map_err(|e: &str| StrandCamError::StringError(e.to_string()))?;
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

    cam.acquisition_start()?;
    // Buffer 20 frames to be processed before dropping them.
    let (tx_frame, rx_frame) = tokio::sync::mpsc::channel::<Msg>(20);
    let tx_frame2 = tx_frame.clone();
    let tx_frame3 = tx_frame.clone();

    // Get initial frame to determine width, height and pixel_format.
    debug!("  started acquisition, waiting for first frame");
    let frame = cam.next_frame()?;
    info!(
        "  acquired first frame: {}x{}",
        frame.width(),
        frame.height()
    );

    #[cfg(feature = "plugin-process-frame")]
    let (plugin_handler_thread_tx, plugin_handler_thread_rx) =
        channellib::bounded::<DynamicFrame>(500);
    #[cfg(feature = "plugin-process-frame")]
    let (plugin_result_tx, plugin_result_rx) = channellib::bounded::<_>(500);

    #[cfg(feature = "plugin-process-frame")]
    let plugin_wait_dur = args.plugin_wait_dur;

    let (firehose_tx, firehose_rx) = tokio::sync::mpsc::channel::<AnnotatedFrame>(5);

    let image_width = frame.width();
    let image_height = frame.height();

    let current_image_png = match_all_dynamic_fmts!(&frame, x, {
        convert_image::frame_to_image(x, convert_image::ImageOptions::Png)?
    });

    #[cfg(feature = "posix_sched_fifo")]
    let process_frame_priority = args.process_frame_priority;

    #[cfg(not(feature = "posix_sched_fifo"))]
    let process_frame_priority = None;

    let raise_grab_thread_priority = args.raise_grab_thread_priority;

    #[cfg(feature = "flydra_feat_detect")]
    let tracker_cfg_src = args.tracker_cfg_src;

    #[cfg(feature = "flydratrax")]
    let save_empty_data2d = args.save_empty_data2d;

    #[cfg(feature = "flydra_feat_detect")]
    let tracker_cfg = match &tracker_cfg_src {
        &ImPtDetectCfgSource::ChangedSavedToDisk(ref src) => {
            // Retrieve the saved preferences
            let (ref app_info, ref prefs_key) = src;
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
        &ImPtDetectCfgSource::ChangesNotSavedToDisk(ref cfg) => cfg.clone(),
    };

    #[cfg(feature = "flydra_feat_detect")]
    let im_pt_detect_cfg = tracker_cfg.clone();

    let mainbrain_info = args.mainbrain_internal_addr.map(|addr| {
        let (transmit_msg_tx, transmit_msg_rx) = mpsc::channel::<flydra_types::HttpApiCallback>(10);

        MainbrainInfo {
            mainbrain_internal_addr: addr,
            transmit_msg_rx,
            transmit_msg_tx,
        }
    });

    let transmit_msg_tx = mainbrain_info.as_ref().map(|i| i.transmit_msg_tx.clone());

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

    if args.force_camera_sync_mode {
        cam.start_default_external_triggering().unwrap();
        send_cam_settings_to_braid(
            &cam.node_map_save()?,
            transmit_msg_tx.as_ref(),
            &current_cam_settings_extension,
            &ros_cam_name,
        )
        .map(|fut| rt_handle.spawn(fut));
    }

    if args.camera_settings_filename.is_none() {
        if let StartSoftwareFrameRateLimit::Enable(fps_limit) = &args.software_limit_framerate {
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

    #[cfg(not(feature = "fiducial"))]
    let apriltag_state = None;

    #[cfg(feature = "fiducial")]
    let apriltag_state = Some(ApriltagState::default());

    let im_ops_state = ImOpsState::default();

    // Here we just create some default, it does not matter what, because it
    // will not be used for anything.
    #[cfg(not(feature = "flydra_feat_detect"))]
    let im_pt_detect_cfg = flydra_pt_detect_cfg::default_absdiff();

    #[cfg(feature = "flydra_feat_detect")]
    let has_image_tracker_compiled = true;

    #[cfg(not(feature = "flydra_feat_detect"))]
    let has_image_tracker_compiled = false;

    let is_braid = args.is_braid;

    // -----------------------------------------------
    // Check if we can use nv h264 and, if so, set that as default.
    let mut mkv_recording_config = MkvRecordingConfig {
        writing_application: Some(get_mkv_writing_application(is_braid)),
        title: Some(cam_name.as_str().to_string()),
        gamma: camera_gamma.clone(),
        ..Default::default()
    };

    let is_nvenc_functioning = test_nvenc_save(&mkv_recording_config, frame)?;

    if is_nvenc_functioning {
        mkv_recording_config.codec =
            ci2_remote_control::MkvCodec::H264(ci2_remote_control::H264Options {
                bitrate: 10000,
                ..Default::default()
            });
    } else {
    }

    // -----------------------------------------------

    let mkv_filename_template = args
        .mkv_filename_template
        .replace("{CAMNAME}", cam_name.as_str());
    let fmf_filename_template = args
        .fmf_filename_template
        .replace("{CAMNAME}", cam_name.as_str());
    let ufmf_filename_template = args
        .ufmf_filename_template
        .replace("{CAMNAME}", cam_name.as_str());

    #[cfg(feature = "fiducial")]
    let format_str_apriltag_csv = args
        .apriltag_csv_filename_template
        .replace("{CAMNAME}", cam_name.as_str());

    #[cfg(not(feature = "fiducial"))]
    let format_str_apriltag_csv = "".into();

    #[cfg(feature = "flydratrax")]
    let has_flydratrax_compiled = true;

    #[cfg(not(feature = "flydratrax"))]
    let has_flydratrax_compiled = false;

    let shared_store = ChangeTracker::new(StoreType {
        is_braid,
        is_nvenc_functioning,
        is_recording_mkv: None,
        is_recording_fmf: None,
        is_recording_ufmf: None,
        format_str_apriltag_csv,
        format_str_mkv: mkv_filename_template,
        format_str: fmf_filename_template,
        format_str_ufmf: ufmf_filename_template,
        camera_name: cam.name().into(),
        recording_filename: None,
        recording_framerate: RecordingFrameRate::default(),
        mkv_recording_config,
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
        im_pt_detect_cfg,
        has_flydratrax_compiled,
        kalman_tracking_config,
        led_program_config,
        led_box_device_lost: false,
        led_box_device_state: None,
        led_box_device_path: args.led_box_device_path,
        #[cfg(feature = "checkercal")]
        has_checkercal_compiled: true,
        #[cfg(not(feature = "checkercal"))]
        has_checkercal_compiled: false,
        checkerboard_data: strand_cam_storetype::CheckerboardCalState::new(),
        checkerboard_save_debug: None,
        post_trigger_buffer_size: 0,
        cuda_devices,
        apriltag_state,
        im_ops_state,
        had_frame_processing_error: false,
        camera_calibration: None,
    });

    let frame_processing_error_state = Arc::new(RwLock::new(FrameProcessingErrorState::default()));

    let camdata_addr = args.camdata_addr;

    let mut config = get_default_config();
    config.cookie_name = "strand-camclient".to_string();

    let shared_store_arc = Arc::new(RwLock::new(shared_store));

    let cam_args_tx2 = cam_args_tx.clone();
    let secret = args.secret.clone();

    // todo: integrate with quit_channel and quit_rx elsewhere.
    let (quit_trigger, valve) = stream_cancel::Valve::new();

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    #[cfg(feature = "flydratrax")]
    let (model_server_shutdown_tx, model_server_shutdown_rx) =
        tokio::sync::oneshot::channel::<()>();

    let http_server_addr = if let Some(http_server_addr) = args.http_server_addr.as_ref() {
        // In braid, this will be `127.0.0.1:0` to get a free port.
        http_server_addr.clone()
    } else {
        // This will be `127.0.0.1:3440` to get a free port.
        DEFAULT_HTTP_ADDR.to_string()
    };

    let (firehose_callback_rx, my_app, connection_callback_rx) = StrandCamApp::new(
        rt_handle.clone(),
        shared_store_arc.clone(),
        secret,
        &http_server_addr,
        config,
        cam_args_tx2.clone(),
        led_box_tx_std.clone(),
        tx_frame3,
        shutdown_rx,
    )
    .await?;

    // The value `args.http_server_addr` is transformed to
    // `local_addr` by doing things like replacing port 0
    // with the actual open port number.

    let (is_loopback, http_camserver_info) = {
        let local_addr = *my_app.inner().local_addr();
        let is_loopback = local_addr.ip().is_loopback();
        let token = my_app.inner().token();
        (is_loopback, BuiServerInfo::new(local_addr, token))
    };

    let url = http_camserver_info.guess_base_url_with_token();

    if args.show_url {
        println!(
            "Depending on things, you may be able to login with this url: {}",
            url,
        );

        if !is_loopback {
            println!("This same URL as a QR code:");
            display_qr_url(&url);
        }
    }

    #[cfg(feature = "plugin-process-frame")]
    let do_process_frame_callback = args.process_frame_callback.is_some();

    #[cfg(feature = "plugin-process-frame")]
    let process_frame_callback = args.process_frame_callback;

    #[cfg(feature = "checkercal")]
    let collected_corners_arc: CollectedCornersArc = Arc::new(RwLock::new(Vec::new()));

    let frame_process_cjh = {
        let (is_starting_tx, is_starting_rx) = tokio::sync::oneshot::channel();

        #[cfg(feature = "flydra_feat_detect")]
        let acquisition_duration_allowed_imprecision_msec =
            args.acquisition_duration_allowed_imprecision_msec;
        #[cfg(feature = "flydra_feat_detect")]
        let csv_save_dir = args.csv_save_dir.clone();
        #[cfg(feature = "flydratrax")]
        let model_server_addr = args.model_server_addr.clone();
        #[cfg(feature = "flydratrax")]
        let led_box_tx_std = led_box_tx_std.clone();
        #[cfg(feature = "flydratrax")]
        let http_camserver_info2 = http_camserver_info.clone();
        let led_box_heartbeat_update_arc2 = led_box_heartbeat_update_arc.clone();

        let handle2 = rt_handle.clone();
        #[cfg(feature = "flydratrax")]
        let (model_server_data_tx, model_server, flydratrax_calibration_source) = {
            let model_server_shutdown_rx = Some(model_server_shutdown_rx);

            info!("send_pose server at {}", model_server_addr);
            let info = flydra_types::StaticMainbrainInfo {
                name: env!("CARGO_PKG_NAME").into(),
                version: env!("CARGO_PKG_VERSION").into(),
            };

            let (model_server_data_tx, data_rx) = tokio::sync::mpsc::channel(50);

            // we need the tokio reactor already by here
            let model_server = flydra2::new_model_server(
                data_rx,
                valve.clone(),
                model_server_shutdown_rx,
                &model_server_addr,
                info,
                handle2.clone(),
            )
            .await?;
            let flydratrax_calibration_source = args.flydratrax_calibration_source;
            (
                model_server_data_tx,
                model_server,
                flydratrax_calibration_source,
            )
        };

        let new_cam_data = flydra_types::RegisterNewCamera {
            orig_cam_name: cam_name.clone(),
            ros_cam_name: ros_cam_name.clone(),
            http_camserver_info: Some(CamHttpServerInfo::Server(http_camserver_info.clone())),
            cam_settings_data: Some(flydra_types::UpdateCamSettings {
                current_cam_settings_buf: settings_on_start,
                current_cam_settings_extension: settings_file_ext,
            }),
            current_image_png: current_image_png.into(),
        };

        #[cfg(feature = "flydratrax")]
        let valve2 = valve.clone();
        let cam_name2 = cam_name.clone();
        let (quit_channel, quit_rx) = tokio::sync::oneshot::channel();
        let frame_process_task_fut = {
            {
                frame_process_task(
                    handle2,
                    #[cfg(feature = "flydratrax")]
                    (model_server_data_tx, model_server),
                    #[cfg(feature = "flydratrax")]
                    flydratrax_calibration_source,
                    cam_name2,
                    #[cfg(feature = "flydra_feat_detect")]
                    camera_cfg,
                    #[cfg(feature = "flydra_feat_detect")]
                    image_width,
                    #[cfg(feature = "flydra_feat_detect")]
                    image_height,
                    rx_frame,
                    #[cfg(feature = "flydra_feat_detect")]
                    tracker_cfg,
                    #[cfg(feature = "flydra_feat_detect")]
                    std::path::Path::new(&csv_save_dir).to_path_buf(),
                    firehose_tx,
                    #[cfg(feature = "plugin-process-frame")]
                    plugin_handler_thread_tx,
                    #[cfg(feature = "plugin-process-frame")]
                    plugin_result_rx,
                    #[cfg(feature = "plugin-process-frame")]
                    plugin_wait_dur,
                    #[cfg(feature = "flydratrax")]
                    led_box_tx_std,
                    quit_rx,
                    is_starting_tx,
                    #[cfg(feature = "flydratrax")]
                    http_camserver_info2,
                    process_frame_priority,
                    mainbrain_info,
                    camdata_addr,
                    led_box_heartbeat_update_arc2,
                    #[cfg(feature = "plugin-process-frame")]
                    do_process_frame_callback,
                    #[cfg(feature = "checkercal")]
                    collected_corners_arc.clone(),
                    #[cfg(feature = "flydratrax")]
                    save_empty_data2d,
                    #[cfg(feature = "flydratrax")]
                    valve2,
                    #[cfg(feature = "flydra_feat_detect")]
                    acquisition_duration_allowed_imprecision_msec,
                    new_cam_data,
                    frame_info_extractor,
                    #[cfg(feature = "flydra_feat_detect")]
                    app_name,
                )
            }
        };
        let join_handle = tokio::spawn(frame_process_task_fut);
        debug!("waiting for frame acquisition thread to start");
        is_starting_rx.await?;
        // TODO: how to check if task still running?
        ControlledTaskJoinHandle {
            quit_channel,
            join_handle,
        }
    };
    debug!("frame_process_task spawned");

    tx_frame
        .send(Msg::Store(shared_store_arc.clone()))
        .await
        .unwrap();

    debug!("installing frame stream handler");

    #[cfg(feature = "posix_sched_fifo")]
    fn with_priority() {
        // This function is run in the camera capture thread as it is started.
        let pid = 0; // this thread
        let priority = 99;
        match posix_scheduler::sched_setscheduler(pid, posix_scheduler::SCHED_FIFO, priority) {
            Ok(()) => info!("grabbing frames with SCHED_FIFO scheduler policy"),
            Err(e) => error!(
                "failed to start frame grabber thread with \
                            SCHED_FIFO scheduler policy: {}",
                e,
            ),
        };
    }

    #[cfg(not(feature = "posix_sched_fifo"))]
    fn with_priority() {
        // This funciton is run in the camera capture thread as it is started.
        debug!("starting async capture");
    }

    fn no_priority() {
        // This funciton is run in the camera capture thread as it is started.
        debug!("starting async capture");
    }

    let async_thread_start = if raise_grab_thread_priority {
        with_priority
    } else {
        no_priority
    };

    // install frame handling
    let n_buffered_frames = 100;
    let frame_stream = cam.frames(n_buffered_frames, async_thread_start)?;
    let mut frame_valved = valve.wrap(frame_stream);
    let cam_stream_future = {
        let shared_store_arc = shared_store_arc.clone();
        let frame_processing_error_state = frame_processing_error_state.clone();
        async move {
            while let Some(frame_msg) = frame_valved.next().await {
                match frame_msg {
                    ci2_async::FrameResult::Frame(frame) => {
                        let frame: DynamicFrame = frame;
                        trace!(
                            "  got frame {}: {}x{}",
                            frame.extra().host_framenumber(),
                            frame.width(),
                            frame.height()
                        );
                        if tx_frame.capacity() == 0 {
                            let mut tracker = shared_store_arc.write();
                            tracker.modify(|tracker| {
                                let mut state = frame_processing_error_state.write();
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
                            tx_frame.send(Msg::Mframe(frame)).await?;
                        }
                    }
                    ci2_async::FrameResult::SingleFrameError(s) => {
                        error!("SingleFrameError({})", s);
                    }
                }
            }
            debug!("cam_stream_future future done {}:{}", file!(), line!());
            Ok::<_, StrandCamError>(())
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

        let interval_stream = tokio_stream::wrappers::IntervalStream::new(interval_stream);

        let mut incoming1 = valve.wrap(interval_stream);

        let known_version2 = known_version;
        let stream_future = async move {
            while incoming1.next().await.is_some() {
                let https = HttpsConnector::new();
                let client = hyper::Client::builder().build::<_, hyper::Body>(https);

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
        rt_handle.spawn(Box::pin(stream_future)); // confirmed: valved and finishes
        debug!("version check future spawned {}:{}", file!(), line!());
    }

    rt_handle.spawn(Box::pin(cam_stream_future)); // confirmed: valved and finishes
    debug!("cam_stream_future future spawned {}:{}", file!(), line!());

    let cam_arg_future = {
        let shared_store_arc = shared_store_arc.clone();

        #[cfg(feature = "checkercal")]
        let cam_name2 = cam_name.clone();

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
                        let mut state = frame_processing_error_state.write();
                        match v {
                            None => {
                                *state = FrameProcessingErrorState::IgnoreAll;
                            }
                            Some(val) => {
                                if val <= 0 {
                                    *state = FrameProcessingErrorState::NotifyAll;
                                } else {
                                    let when = chrono::Utc::now() + chrono::Duration::seconds(val);
                                    *state = FrameProcessingErrorState::IgnoreUntil(when);
                                }
                            }
                        }

                        let mut tracker = shared_store_arc.write();
                        tracker.modify(|tracker| tracker.had_frame_processing_error = false);
                    }
                    CamArg::SetExposureTime(v) => match cam.set_exposure_time(v) {
                        Ok(()) => {
                            send_cam_settings_to_braid(
                                &cam.node_map_save().unwrap(),
                                transmit_msg_tx.as_ref(),
                                &current_cam_settings_extension,
                                &ros_cam_name,
                            )
                            .map(|fut| rt_handle.spawn(fut));
                            let mut tracker = shared_store_arc.write();
                            tracker.modify(|tracker| tracker.exposure_time.current = v);
                        }
                        Err(e) => {
                            error!("setting exposure_time: {:?}", e);
                        }
                    },
                    CamArg::SetGain(v) => match cam.set_gain(v) {
                        Ok(()) => {
                            send_cam_settings_to_braid(
                                &cam.node_map_save().unwrap(),
                                transmit_msg_tx.as_ref(),
                                &current_cam_settings_extension,
                                &ros_cam_name,
                            )
                            .map(|fut| rt_handle.spawn(fut));
                            let mut tracker = shared_store_arc.write();
                            tracker.modify(|tracker| tracker.gain.current = v);
                        }
                        Err(e) => {
                            error!("setting gain: {:?}", e);
                        }
                    },
                    CamArg::SetGainAuto(v) => match cam.set_gain_auto(v) {
                        Ok(()) => {
                            send_cam_settings_to_braid(
                                &cam.node_map_save().unwrap(),
                                transmit_msg_tx.as_ref(),
                                &current_cam_settings_extension,
                                &ros_cam_name,
                            )
                            .map(|fut| rt_handle.spawn(fut));
                            let mut tracker = shared_store_arc.write();
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
                        let mut tracker = shared_store_arc.write();
                        tracker.modify(|tracker| tracker.recording_framerate = v);
                    }
                    CamArg::SetMkvRecordingConfig(mut cfg) => {
                        if cfg.writing_application.is_none() {
                            // The writing application is not set in the web UI
                            cfg.writing_application = Some(get_mkv_writing_application(is_braid));
                        }
                        if cfg.title.is_none() {
                            // The title is not set in the web UI
                            cfg.title = Some(cam_name.as_str().to_string());
                        }
                        if cfg.gamma.is_none() {
                            // The gamma is not set in the web UI
                            cfg.gamma = camera_gamma.clone();
                        }
                        let mut tracker = shared_store_arc.write();
                        tracker.modify(|tracker| tracker.mkv_recording_config = cfg);
                    }
                    CamArg::SetMkvRecordingFps(v) => {
                        let mut tracker = shared_store_arc.write();
                        tracker.modify(|tracker| tracker.mkv_recording_config.max_framerate = v);
                    }
                    CamArg::SetExposureAuto(v) => match cam.set_exposure_auto(v) {
                        Ok(()) => {
                            send_cam_settings_to_braid(
                                &cam.node_map_save().unwrap(),
                                transmit_msg_tx.as_ref(),
                                &current_cam_settings_extension,
                                &ros_cam_name,
                            )
                            .map(|fut| rt_handle.spawn(fut));
                            let mut tracker = shared_store_arc.write();
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
                                send_cam_settings_to_braid(
                                    &cam.node_map_save().unwrap(),
                                    transmit_msg_tx.as_ref(),
                                    &current_cam_settings_extension,
                                    &ros_cam_name,
                                )
                                .map(|fut| rt_handle.spawn(fut));
                                let mut tracker = shared_store_arc.write();
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
                            send_cam_settings_to_braid(
                                &cam.node_map_save().unwrap(),
                                transmit_msg_tx.as_ref(),
                                &current_cam_settings_extension,
                                &ros_cam_name,
                            )
                            .map(|fut| rt_handle.spawn(fut));
                            let mut tracker = shared_store_arc.write();
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
                        tx_frame2.send(Msg::SetFrameOffset(fo)).await?;
                    }
                    CamArg::SetClockModel(cm) => {
                        tx_frame2.send(Msg::SetClockModel(cm)).await?;
                    }
                    CamArg::SetFormatStr(v) => {
                        let mut tracker = shared_store_arc.write();
                        tracker.modify(|tracker| tracker.format_str = v);
                    }
                    CamArg::SetIsRecordingMkv(do_recording) => {
                        // Copy values from cache and release the lock immediately.
                        let (is_recording_mkv, format_str_mkv, mkv_recording_config) = {
                            let tracker = shared_store_arc.read();
                            let shared: &StoreType = tracker.as_ref();
                            (
                                shared.is_recording_mkv.clone(),
                                shared.format_str_mkv.clone(),
                                shared.mkv_recording_config.clone(),
                            )
                        };

                        if is_recording_mkv.is_some() != do_recording {
                            info!("changed recording mkv value: do_recording={}", do_recording);

                            // Compute new values.
                            let (msg, new_val) = if do_recording {
                                // change state
                                (
                                    Msg::StartMkv((format_str_mkv.clone(), mkv_recording_config)),
                                    Some(RecordingPath::new(format_str_mkv)),
                                )
                            } else {
                                (Msg::StopMkv, None)
                            };

                            // Send the command.
                            tx_frame2.send(msg).await?;

                            // Save the new recording state.
                            let mut tracker = shared_store_arc.write();
                            tracker.modify(|shared| {
                                shared.is_recording_mkv = new_val;
                            });
                        }
                    }
                    CamArg::ToggleAprilTagFamily(family) => {
                        let mut tracker = shared_store_arc.write();
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
                        let mut tracker = shared_store_arc.write();
                        tracker.modify(|shared| {
                            if let Some(ref mut ts) = shared.apriltag_state {
                                ts.do_detection = do_detection;
                            } else {
                                error!("no apriltag support, not switching state");
                            }
                        });
                    }
                    CamArg::ToggleImOpsDetection(do_detection) => {
                        let mut tracker = shared_store_arc.write();
                        tracker.modify(|shared| {
                            shared.im_ops_state.do_detection = do_detection;
                        });
                    }
                    CamArg::SetImOpsDestination(v) => {
                        let mut tracker = shared_store_arc.write();
                        tracker.modify(|shared| {
                            shared.im_ops_state.destination = v;
                        });
                    }
                    CamArg::SetImOpsSource(v) => {
                        let mut tracker = shared_store_arc.write();
                        tracker.modify(|shared| {
                            shared.im_ops_state.source = v;
                        });
                    }
                    CamArg::SetImOpsCenterX(v) => {
                        let mut tracker = shared_store_arc.write();
                        tracker.modify(|shared| {
                            shared.im_ops_state.center_x = v;
                        });
                    }
                    CamArg::SetImOpsCenterY(v) => {
                        let mut tracker = shared_store_arc.write();
                        tracker.modify(|shared| {
                            shared.im_ops_state.center_y = v;
                        });
                    }
                    CamArg::SetImOpsThreshold(v) => {
                        let mut tracker = shared_store_arc.write();
                        tracker.modify(|shared| {
                            shared.im_ops_state.threshold = v;
                        });
                    }

                    CamArg::SetIsRecordingAprilTagCsv(do_recording) => {
                        let new_val = {
                            let tracker = shared_store_arc.read();
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
                            tx_frame2.send(msg).await?;
                        }

                        // Here we save the new recording state.
                        if let Some(new_val) = new_val {
                            let mut tracker = shared_store_arc.write();
                            tracker.modify(|shared| {
                                if let Some(ref mut ts) = shared.apriltag_state {
                                    ts.is_recording_csv = new_val;
                                };
                            });
                        }
                    }

                    CamArg::PostTrigger => {
                        let (is_recording_mkv, format_str_mkv, mkv_recording_config) = {
                            let tracker = shared_store_arc.read();
                            let shared: &StoreType = tracker.as_ref();
                            (
                                shared.is_recording_mkv.clone(),
                                shared.format_str_mkv.clone(),
                                shared.mkv_recording_config.clone(),
                            )
                        };

                        tx_frame2
                            .send(Msg::PostTriggerStartMkv((
                                format_str_mkv.clone(),
                                mkv_recording_config,
                            )))
                            .await?;
                        {
                            let mut tracker = shared_store_arc.write();
                            tracker.modify(|shared| {
                                shared.is_recording_mkv = Some(RecordingPath::new(format_str_mkv));
                            })
                        }
                    }
                    CamArg::SetPostTriggerBufferSize(size) => {
                        tx_frame2.send(Msg::SetPostTriggerBufferSize(size)).await?;
                    }
                    CamArg::SetIsRecordingFmf(do_recording) => {
                        // Copy values from cache and release the lock immediately.
                        let (is_recording_fmf, format_str, recording_framerate) = {
                            let tracker = shared_store_arc.read();
                            let shared: &StoreType = tracker.as_ref();
                            (
                                shared.is_recording_fmf.clone(),
                                shared.format_str.clone(),
                                shared.recording_framerate.clone(),
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
                            tx_frame2.send(msg).await?;

                            // Save the new recording state.
                            let mut tracker = shared_store_arc.write();
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
                                let tracker = shared_store_arc.read();
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
                                tx_frame2.send(msg).await?;

                                // Save the new recording state.
                                let mut tracker = shared_store_arc.write();
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
                                let mut tracker = shared_store_arc.write();
                                tracker.modify(|shared| {
                                    shared.is_doing_object_detection = value;
                                });
                            }
                            tx_frame2.send(Msg::SetTracking(value)).await?;
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
                            .await?;
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
                                tx_frame2.send(Msg::SetExpConfig(cfg.clone())).await?;
                                {
                                    let mut tracker = shared_store_arc.write();
                                    tracker.modify(|shared| {
                                        shared.im_pt_detect_cfg = cfg;
                                    });
                                }

                                if let ImPtDetectCfgSource::ChangedSavedToDisk(ref src) =
                                    tracker_cfg_src
                                {
                                    let (ref app_info, ref prefs_key) = src;
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
                                        let mut tracker = shared_store_arc.write();
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
                                        let mut tracker = shared_store_arc.write();
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
                            let mut tracker = shared_store_arc.write();
                            tracker.modify(|shared| {
                                shared.checkerboard_data.enabled = val;
                            });
                        }
                    }
                    CamArg::ToggleCheckerboardDebug(val) => {
                        #[cfg(feature = "checkercal")]
                        {
                            let mut tracker = shared_store_arc.write();
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
                            let mut tracker = shared_store_arc.write();
                            tracker.modify(|shared| {
                                shared.checkerboard_data.width = val;
                            });
                        }
                    }
                    CamArg::SetCheckerboardHeight(val) => {
                        #[cfg(feature = "checkercal")]
                        {
                            let mut tracker = shared_store_arc.write();
                            tracker.modify(|shared| {
                                shared.checkerboard_data.height = val;
                            });
                        }
                    }
                    CamArg::ClearCheckerboards => {
                        #[cfg(feature = "checkercal")]
                        {
                            {
                                let mut collected_corners = collected_corners_arc.write();
                                collected_corners.clear();
                            }

                            {
                                let mut tracker = shared_store_arc.write();
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
                                let tracker = shared_store_arc.read();
                                let shared = (*tracker).as_ref();
                                let n_rows = shared.checkerboard_data.height;
                                let n_cols = shared.checkerboard_data.width;
                                let checkerboard_save_debug =
                                    shared.checkerboard_save_debug.clone();
                                (n_rows, n_cols, checkerboard_save_debug)
                            };

                            let goodcorners: Vec<camcal::CheckerBoardData> = {
                                let collected_corners = collected_corners_arc.read();
                                collected_corners
                                    .iter()
                                    .map(|corners| {
                                        let dim = 1.234; // TODO make this useful
                                        let x: Vec<(f64, f64)> = corners
                                            .iter()
                                            .map(|x| (x.0 as f64, x.1 as f64))
                                            .collect();
                                        camcal::CheckerBoardData::new(
                                            dim,
                                            n_rows as usize,
                                            n_cols as usize,
                                            &x,
                                        )
                                    })
                                    .collect()
                            };

                            let ros_cam_name = cam_name2.to_ros();
                            let local: chrono::DateTime<chrono::Local> = chrono::Local::now();

                            if let Some(debug_dir) = &checkerboard_save_debug {
                                let format_str = format!(
                                    "checkerboard_input_{}.%Y%m%d_%H%M%S.yaml",
                                    ros_cam_name.as_str()
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
                            match camcal::compute_intrinsics::<f64>(size, &goodcorners) {
                                Ok(intrinsics) => {
                                    info!("got calibrated intrinsics: {:?}", intrinsics);

                                    // Convert from mvg to ROS format.
                                    let ci: opencv_ros_camera::RosCameraInfo<_> =
                                        opencv_ros_camera::NamedIntrinsicParameters {
                                            intrinsics,
                                            width: image_width as usize,
                                            height: image_height as usize,
                                            name: ros_cam_name.as_str().to_string(),
                                        }
                                        .into();

                                    let cal_dir = directories::BaseDirs::new()
                                        .as_ref()
                                        .map(|bd| {
                                            bd.config_dir().join(APP_INFO.name).join("camera_info")
                                        })
                                        .unwrap();

                                    let format_str =
                                        format!("{}.%Y%m%d_%H%M%S.yaml", ros_cam_name.as_str());
                                    let stamped = local.format(&format_str).to_string();
                                    let cam_info_file_stamped = cal_dir.join(stamped);

                                    let mut cam_info_file = cal_dir.clone();
                                    cam_info_file.push(ros_cam_name.as_str());
                                    cam_info_file.set_extension("yaml");

                                    // Save timestamped version first for backup
                                    // purposes (since below we overwrite the
                                    // non-timestamped file).
                                    {
                                        let f = File::create(&cam_info_file_stamped)
                                            .expect("create file");
                                        serde_yaml::to_writer(f, &ci)
                                            .expect("serde_yaml::to_writer");
                                    }

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
            tx_frame2.send(Msg::StopFMF).await?;
            tx_frame2.send(Msg::StopMkv).await?;
            #[cfg(feature = "flydra_feat_detect")]
            tx_frame2.send(Msg::StopUFMF).await?;
            #[cfg(feature = "flydra_feat_detect")]
            tx_frame2
                .send(Msg::SetIsSavingObjDetectionCsv(CsvSaveConfig::NotSaving))
                .await?;

            tx_frame2.send(Msg::QuitFrameProcessThread).await?; // this will quit the frame_process_task

            // Tell all streams to quit.
            debug!(
                "*** sending quit trigger to all valved streams. **** {}:{}",
                file!(),
                line!()
            );
            quit_trigger.cancel();
            debug!("*** sending shutdown to hyper **** {}:{}", file!(), line!());
            shutdown_tx.send(()).expect("sending shutdown to hyper");

            #[cfg(feature = "flydratrax")]
            model_server_shutdown_tx
                .send(())
                .expect("sending shutdown to model server");

            info!("attempting to nicely stop camera");
            if let Some((control, join_handle)) = cam.control_and_join_handle() {
                control.stop();
                while !control.is_done() {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                info!("camera thread stopped");
                join_handle.join().expect("join camera thread");
                info!("camera thread joined");
            } else {
                error!("camera thread not running!?");
            }

            info!("cam_args_rx future is resolved");
            Ok::<_, StrandCamError>(())
        }
    };

    if !args.no_browser {
        // sleep to let the webserver start before opening browser
        std::thread::sleep(std::time::Duration::from_millis(100));

        open_browser(url)?;
    } else {
        info!("listening at {}", url);
    }

    let (quit_channel, quit_rx) = tokio::sync::oneshot::channel();

    let join_handle = tokio::spawn(video_streaming::firehose_task(
        connection_callback_rx,
        firehose_rx,
        firehose_callback_rx,
        false,
        strand_cam_storetype::STRAND_CAM_EVENTS_URL_PATH,
        quit_rx,
    ));

    let video_streaming_cjh = ControlledTaskJoinHandle {
        quit_channel,
        join_handle,
    };

    #[cfg(feature = "plugin-process-frame")]
    let plugin_streaming_cjh = {
        let (flag, control) = thread_control::make_pair();
        let join_handle = std::thread::Builder::new()
            .name("plugin_streaming".to_string())
            .spawn(move || {
                // ignore plugin
                let thread_closer = CloseAppOnThreadExit::new(cam_args_tx2, file!(), line!());
                while flag.is_alive() {
                    let frame = thread_closer.check(plugin_handler_thread_rx.recv());
                    if let Some(ref pfc) = process_frame_callback {
                        let c_data = view_as_c_frame(&frame);
                        let c_timestamp = get_c_timestamp(&frame);
                        let ffi_result = (pfc.func_ptr)(&c_data, pfc.data_handle, c_timestamp);
                        let points = ffi_to_points(&ffi_result);
                        thread_closer.check(plugin_result_tx.send(points));
                    }
                }
                thread_closer.success();
            })?
            .into();
        ControlledThreadJoinHandle {
            control,
            join_handle,
        }
    };

    debug!("  running forever");

    {
        // run LED Box stuff here

        use tokio_serial::SerialPortBuilderExt;
        use tokio_util::codec::Decoder;

        use led_box::LedBoxCodec;
        use led_box_comms::{ChannelState, DeviceState, OnState};

        let start_led_box_instant = std::time::Instant::now();

        // enqueue initial message
        {
            fn make_chan(num: u8, on_state: OnState) -> ChannelState {
                let intensity = led_box_comms::MAX_INTENSITY;
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
            let tracker = shared_store_arc.read();
            let shared = tracker.as_ref();
            if let Some(serial_device) = shared.led_box_device_path.as_ref() {
                info!("opening LED box \"{}\"", serial_device);
                // open with default settings 9600 8N1
                #[allow(unused_mut)]
                let mut port = tokio_serial::new(serial_device, 9600)
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
            let (mut writer, mut reader) = LedBoxCodec::new().framed(port).split();

            // Clear potential initially present bytes from stream...
            let _ = tokio::time::timeout(std::time::Duration::from_millis(50), reader.next()).await;

            writer.send(led_box_comms::ToDevice::VersionRequest).await?;

            match tokio::time::timeout(std::time::Duration::from_millis(50), reader.next()).await {
                Ok(Some(Ok(msg))) => match msg {
                    led_box_comms::FromDevice::VersionResponse(led_box_comms::COMM_VERSION) => {
                        info!(
                            "Connected to firmware version {}",
                            led_box_comms::COMM_VERSION
                        );
                    }
                    msg => {
                        anyhow::bail!("Unexpected response from LED Box {:?}. Is your firmware version correct? (Needed version: {})",
                            msg, led_box_comms::COMM_VERSION);
                    }
                },
                _ => {
                    anyhow::bail!("Failed connecting to LED Box. Is your firmware version correct? (Needed version: {})",
                        led_box_comms::COMM_VERSION);
                }
            }

            // handle messages from the device
            let from_device_task = async move {
                debug!("awaiting message from LED box");
                while let Some(msg) = tokio_stream::StreamExt::next(&mut reader).await {
                    match msg {
                        Ok(led_box_comms::FromDevice::EchoResponse8(d)) => {
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
                            let mut led_box_heartbeat_update = led_box_heartbeat_update_arc.write();
                            *led_box_heartbeat_update = Some(std::time::Instant::now());
                        }
                        Ok(msg) => {
                            todo!("Did not handle {:?}", msg);
                            // error!("unknown message received: {:?}", msg);
                        }
                        Err(e) => {
                            panic!("unexpected error: {}: {:?}", e, e);
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
                    match msg {
                        ToLedBoxDevice::DeviceState(new_state) => {
                            let mut tracker = shared_store_arc.write();
                            tracker.modify(|shared| {
                                shared.led_box_device_state = Some(new_state);
                            })
                        }
                        _ => {}
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

    let ajh = AllJoinHandles {
        frame_process_cjh,
        video_streaming_cjh,
        #[cfg(feature = "plugin-process-frame")]
        plugin_streaming_cjh,
    };

    let cam_arg_future2 = async move {
        cam_arg_future.await?;

        // we get here once the whole program is trying to shut down.
        info!("Now stopping spawned tasks.");
        let result: Result<()> = ajh.close_and_join_all().await;
        result
    };

    Ok((http_camserver_info, cam_args_tx, cam_arg_future2, my_app))
}

#[cfg(feature = "plugin-process-frame")]
pub struct ControlledThreadJoinHandle<T> {
    control: thread_control::Control,
    join_handle: std::thread::JoinHandle<T>,
}

#[cfg(feature = "plugin-process-frame")]
impl<T> ControlledThreadJoinHandle<T> {
    fn thead_close_and_join(self) -> std::thread::Result<T> {
        debug!(
            "sending stop {:?} {:?}",
            self.join_handle.thread().name(),
            self.join_handle.thread().id()
        );
        self.control.stop();
        while !self.control.is_done() {
            debug!(
                "waiting for stop {:?} {:?}",
                self.join_handle.thread().name(),
                self.join_handle.thread().id()
            );
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        debug!(
            "joining {:?} {:?}",
            self.join_handle.thread().name(),
            self.join_handle.thread().id()
        );
        let result = self.join_handle.join();
        debug!("joining done");
        result
    }
}

pub struct ControlledTaskJoinHandle<T> {
    quit_channel: tokio::sync::oneshot::Sender<()>,
    join_handle: tokio::task::JoinHandle<T>,
}

impl<T> ControlledTaskJoinHandle<T> {
    async fn close_and_join(self) -> std::result::Result<T, tokio::task::JoinError> {
        debug!("sending stop");

        // debug!(
        //     "sending stop {:?} {:?}",
        //     self.join_handle.thread().name(),
        //     self.join_handle.thread().id()
        // );
        self.quit_channel.send(()).ok();
        debug!("joining");

        // debug!(
        //     "joining {:?} {:?}",
        //     self.join_handle.thread().name(),
        //     self.join_handle.thread().id()
        // );
        let result = self.join_handle.await?;
        debug!("joining done");
        Ok(result)
    }
}

pub struct AllJoinHandles {
    frame_process_cjh: ControlledTaskJoinHandle<anyhow::Result<()>>,
    video_streaming_cjh: ControlledTaskJoinHandle<std::result::Result<(), video_streaming::Error>>,
    #[cfg(feature = "plugin-process-frame")]
    plugin_streaming_cjh: ControlledThreadJoinHandle<()>,
}

impl AllJoinHandles {
    async fn close_and_join_all(self) -> Result<()> {
        self.frame_process_cjh.close_and_join().await??;
        self.video_streaming_cjh.close_and_join().await??;
        #[cfg(feature = "plugin-process-frame")]
        self.plugin_streaming_cjh.thead_close_and_join().unwrap();
        Ok(())
    }
}

#[cfg(feature = "plugin-process-frame")]
fn ffi_to_points(
    pts: &plugin_defs::StrandCamFrameAnnotation,
) -> Vec<http_video_streaming_types::Point> {
    pts.as_slice()
        .iter()
        .map(|pt| http_video_streaming_types::Point {
            x: pt.x,
            y: pt.y,
            area: None,
            theta: None,
        })
        .collect()
}

#[cfg(feature = "plugin-process-frame")]
fn get_pixfmt(pixfmt: &PixFmt) -> plugin_defs::EisvogelPixelFormat {
    match pixfmt {
        PixFmt::Mono8 => plugin_defs::EisvogelPixelFormat::MONO8,
        PixFmt::BayerRG8 => plugin_defs::EisvogelPixelFormat::BayerRG8,
        other => panic!("unsupported pixel format: {}", other),
    }
}

#[cfg(feature = "plugin-process-frame")]
fn get_c_timestamp<'a>(frame: &'a DynamicFrame) -> f64 {
    let ts = frame.extra().host_timestamp();
    datetime_conversion::datetime_to_f64(&ts)
}

#[cfg(feature = "plugin-process-frame")]
fn view_as_c_frame(frame: &DynamicFrame) -> plugin_defs::FrameData {
    use formats::Stride;

    let pixel_format = get_pixfmt(&frame.pixel_format());

    let result = plugin_defs::FrameData {
        data: frame.image_data_without_format(),
        stride: frame.stride() as u64,
        rows: frame.height(),
        cols: frame.width(),
        pixel_format,
    };
    result
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

fn send_cam_settings_to_braid(
    cam_settings: &str,
    transmit_msg_tx: Option<&mpsc::Sender<flydra_types::HttpApiCallback>>,
    current_cam_settings_extension: &str,
    ros_cam_name: &RosCamName,
) -> Option<impl std::future::Future<Output = ()>> {
    if let Some(transmit_msg_tx) = transmit_msg_tx {
        let current_cam_settings_buf = cam_settings.to_string();
        let current_cam_settings_extension = current_cam_settings_extension.to_string();
        let ros_cam_name = ros_cam_name.clone();
        let mut transmit_msg_tx = transmit_msg_tx.clone();
        let fut = async move {
            let msg = flydra_types::HttpApiCallback::UpdateCamSettings(flydra_types::PerCam {
                ros_cam_name,
                inner: flydra_types::UpdateCamSettings {
                    current_cam_settings_buf,
                    current_cam_settings_extension,
                },
            });
            transmit_msg_tx.send(msg).await.unwrap();
        };
        Some(fut)
    } else {
        None
    }
}
