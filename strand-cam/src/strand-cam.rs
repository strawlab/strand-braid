// TODO: if camera not available, launch alternate UI indicating such and
// waiting for it to become available?

// TODO: add quit app button to UI.

// TODO: UI automatically reconnect to app after app restart.

#![cfg_attr(feature = "backtrace", feature(error_generic_member_access))]

#[cfg(feature = "backtrace")]
use std::backtrace::Backtrace;

use anyhow::Context;

#[cfg(feature = "fiducial")]
use ads_apriltag as apriltag;

use async_change_tracker::ChangeTracker;
use event_stream_types::{
    AcceptsEventStream, ConnectionEvent, ConnectionEventType, ConnectionSessionKey,
    EventBroadcaster, TolerantJson,
};
use futures::{sink::SinkExt, stream::StreamExt};
use http::StatusCode;
use http_video_streaming as video_streaming;
use hyper_tls::HttpsConnector;
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
#[cfg(feature = "fiducial")]
use libflate::finish::AutoFinishUnchecked;
#[cfg(feature = "fiducial")]
use libflate::gzip::Encoder;
use machine_vision_formats as formats;
#[cfg(feature = "flydratrax")]
use nalgebra as na;
#[allow(unused_imports)]
use preferences_serde1::{AppInfo, Preferences};
use serde::{Deserialize, Serialize};
use tower_http::trace::TraceLayer;
use tracing::{debug, error, info, trace, warn};

use basic_frame::{match_all_dynamic_fmts, DynamicFrame};
use bui_backend_session_types::{AccessToken, ConnectionKey, SessionKey};
use ci2::{Camera, CameraInfo, CameraModule};
use ci2_async::AsyncCamera;
use fmf::FMFWriter;
use formats::PixFmt;
use timestamped_frame::ExtraTimeData;

#[cfg(feature = "flydratrax")]
use http_video_streaming_types::{DrawableShape, StrokeStyle};

use video_streaming::{AnnotatedFrame, FirehoseCallback};

use std::{path::Path, result::Result as StdResult};

#[cfg(feature = "flydra_feat_detect")]
use ci2_remote_control::CsvSaveConfig;
use ci2_remote_control::{
    CamArg, CodecSelection, Mp4Codec, Mp4RecordingConfig, NvidiaH264Options, RecordingFrameRate,
};
#[cfg(feature = "flydratrax")]
use flydra_types::BuiServerAddrInfo;
use flydra_types::{
    BuiServerInfo, RawCamName, RealtimePointsDestAddr, StartSoftwareFrameRateLimit,
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
use strand_cam_storetype::{
    CallbackType, ImOpsState, RangedValue, StoreType, ToLedBoxDevice, STRAND_CAM_EVENT_NAME,
};

use strand_cam_storetype::{KalmanTrackingConfig, LedProgramConfig};

#[cfg(feature = "flydratrax")]
use flydra_types::{FlydraFloatTimestampLocal, HostClock, SyncFno, Triggerbox};

#[cfg(feature = "flydratrax")]
use strand_cam_pseudo_cal::PseudoCameraCalibrationData;

use rust_cam_bui_types::RecordingPath;

use std::fs::File;
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::Arc;

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
use flydra2::{CoordProcessor, CoordProcessorConfig, MyFloat, SaveToDiskMsg, StreamItem};

#[cfg(feature = "imtrack-absdiff")]
pub use flydra_pt_detect_cfg::default_absdiff as default_im_pt_detect;
#[cfg(feature = "imtrack-dark-circle")]
pub use flydra_pt_detect_cfg::default_dark_circle as default_im_pt_detect;

#[cfg(feature = "bundle_files")]
static ASSETS_DIR: include_dir::Dir<'static> =
    include_dir::include_dir!("$CARGO_MANIFEST_DIR/yew_frontend/pkg");

#[cfg(feature = "flydratrax")]
const KALMAN_TRACKING_PREFS_KEY: &'static str = "kalman-tracking";

#[cfg(feature = "flydratrax")]
const LED_PROGRAM_PREFS_KEY: &'static str = "led-config";

const COOKIE_SECRET_KEY: &str = "cookie-secret-base64";
const BRAID_COOKIE_KEY: &str = "braid-cookie";

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
    // #[error("BUI backend error: {0}")]
    // BuiBackendError(#[from] bui_backend::Error),
    #[error("BUI backend session error: {0}")]
    BuiBackendSessionError(#[from] bui_backend_session::Error),
    #[error("Braid HTTP session error: {0}")]
    BraidHttpSessionError(#[from] braid_http_session::Error),
    #[error("hyper_util client legacy error: {0}")]
    HyperUtilClientLegacyError(#[from] hyper_util::client::legacy::Error),
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
    // #[error(
    //     "The --jwt-secret argument must be passed or the JWT_SECRET environment \
    //               variable must be set."
    // )]
    // JwtError,
    #[cfg(feature = "flydratrax")]
    #[error("MVG error: {0}")]
    MvgError(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        mvg::MvgError,
    ),
    #[error("{0}")]
    Mp4WriterError(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        mp4_writer::Error,
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

    fn check<T, E>(&self, result: StdResult<T, E>) -> T
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
    Mframe(DynamicFrame),
    #[cfg(feature = "flydra_feat_detect")]
    SetIsSavingObjDetectionCsv(CsvSaveConfig),
    #[cfg(feature = "flydra_feat_detect")]
    SetExpConfig(ImPtDetectCfg),
    Store(Arc<parking_lot::RwLock<ChangeTracker<StoreType>>>),
    #[cfg(feature = "flydra_feat_detect")]
    TakeCurrentImageAsBackground,
    #[cfg(feature = "flydra_feat_detect")]
    ClearBackground(f32),
    SetFrameOffset(u64),
    SetClockModel(Option<rust_cam_bui_types::ClockModel>),
    StartAprilTagRec(String),
    StopAprilTagRec,
}

impl std::fmt::Debug for Msg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> StdResult<(), std::fmt::Error> {
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
type CollectedCornersArc = Arc<parking_lot::RwLock<Vec<Vec<(f32, f32)>>>>;

async fn convert_stream(
    raw_cam_name: RawCamName,
    mut transmit_feature_detect_settings_rx: tokio::sync::mpsc::Receiver<
        flydra_feature_detector_types::ImPtDetectCfg,
    >,
    transmit_msg_tx: tokio::sync::mpsc::Sender<flydra_types::BraidHttpApiCallback>,
) -> Result<()> {
    while let Some(val) = transmit_feature_detect_settings_rx.recv().await {
        let msg =
            flydra_types::BraidHttpApiCallback::UpdateFeatureDetectSettings(flydra_types::PerCam {
                raw_cam_name: raw_cam_name.clone(),
                inner: flydra_types::UpdateFeatureDetectSettings {
                    current_feature_detect_settings: val,
                },
            });
        transmit_msg_tx.send(msg).await?;
    }
    Ok(())
}

// We perform image analysis in its own task.
async fn frame_process_task(
    #[cfg(feature = "flydratrax")] model_server_data_tx: tokio::sync::mpsc::Sender<(
        flydra2::SendType,
        flydra2::TimeDataPassthrough,
    )>,
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
    #[cfg(feature = "flydratrax")] http_camserver_info: BuiServerAddrInfo,
    process_frame_priority: Option<(i32, i32)>,
    transmit_msg_tx: Option<tokio::sync::mpsc::Sender<flydra_types::BraidHttpApiCallback>>,
    camdata_addr: Option<RealtimePointsDestAddr>,
    led_box_heartbeat_update_arc: Arc<parking_lot::RwLock<Option<std::time::Instant>>>,
    #[cfg(feature = "plugin-process-frame")] do_process_frame_callback: bool,
    #[cfg(feature = "checkercal")] collected_corners_arc: CollectedCornersArc,
    #[cfg(feature = "flydratrax")] save_empty_data2d: SaveEmptyData2dType,
    #[cfg(feature = "flydra_feat_detect")] acquisition_duration_allowed_imprecision_msec: Option<
        f64,
    >,
    frame_info_extractor: &dyn ci2::ExtractFrameInfo,
    #[cfg(feature = "flydra_feat_detect")] app_name: &'static str,
) -> anyhow::Result<()> {
    let my_runtime: tokio::runtime::Handle = tokio::runtime::Handle::current();

    let is_braid = camdata_addr.is_some();

    let raw_cam_name: RawCamName = cam_name.clone();

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

    let (transmit_feature_detect_settings_tx, mut transmit_msg_tx) = if is_braid {
        let (transmit_feature_detect_settings_tx, transmit_feature_detect_settings_rx) =
            tokio::sync::mpsc::channel::<flydra_feature_detector_types::ImPtDetectCfg>(10);

        let transmit_msg_tx = transmit_msg_tx.unwrap();

        my_runtime.spawn(convert_stream(
            raw_cam_name.clone(),
            transmit_feature_detect_settings_rx,
            transmit_msg_tx.clone(),
        ));

        (
            Some(transmit_feature_detect_settings_tx),
            Some(transmit_msg_tx),
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
    let mut shared_store_arc: Option<Arc<parking_lot::RwLock<ChangeTracker<StoreType>>>> = None;
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

    let current_image_timer_arc = Arc::new(parking_lot::RwLock::new(std::time::Instant::now()));

    let mut im_ops_socket: Option<std::net::UdpSocket> = None;

    let mut opt_clock_model = None;
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
                                    BuiServerInfo::Server(http_camserver_info.clone());
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
                                let mut coord_processor = CoordProcessor::new(
                                    CoordProcessorConfig {
                                        tracking_params,
                                        save_empty_data2d,
                                        ignore_latency,
                                        mini_arena_debug_image_dir: None,
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
                                    let jh = consume_future.await;

                                    debug!(
                                        "waiting on flydratrax coord processor {}:{}",
                                        file!(),
                                        line!()
                                    );
                                    jh.join().unwrap().unwrap();
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
                        match braidz_write_tx.send(SaveToDiskMsg::StopSavingCsv).await {
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
                let f = std::fs::File::create(path)?;
                fmf_writer = Some(FmfWriteInfo::new(FMFWriter::new(f)?, recording_framerate));
            }
            #[cfg(feature = "flydra_feat_detect")]
            Msg::StartUFMF(dest) => {
                ufmf_state = Some(UfmfState::Starting(dest));
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
                let opt_trigger_stamp = flydra_types::get_start_ts(
                    opt_clock_model.as_ref(),
                    opt_frame_offset,
                    extracted_frame_info.host_framenumber,
                );
                let (timestamp_source, save_mp4_fmf_stamp) =
                    if let Some(trigger_timestamp) = &opt_trigger_stamp {
                        (TimestampSource::BraidTrigger, trigger_timestamp.into())
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
                            cam_name: raw_cam_name.as_str().to_string(),
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
                                        raw_cam_name.clone(),
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
                                                        get_intensity(device_state, 1)
                                                    );
                                                    led2 = format!(
                                                        "{}",
                                                        get_intensity(device_state, 2)
                                                    );
                                                    led3 = format!(
                                                        "{}",
                                                        get_intensity(device_state, 3)
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
                    let send_msg = {
                        let mut timer = current_image_timer_arc.write();
                        let elapsed = timer.elapsed();
                        let mut send_msg = false;
                        if elapsed > std::time::Duration::from_millis(2000) {
                            *timer = std::time::Instant::now();
                            send_msg = true;
                        }
                        send_msg
                    };

                    if send_msg {
                        // encode frame to png buf

                        if let Some(cb_sender) = transmit_msg_tx.as_ref() {
                            let current_image_png = match_all_dynamic_fmts!(&frame, x, {
                                convert_image::frame_to_image(x, convert_image::ImageOptions::Png)
                                    .unwrap()
                            });

                            let raw_cam_name = raw_cam_name.clone();

                            let msg = flydra_types::BraidHttpApiCallback::UpdateCurrentImage(
                                flydra_types::PerCam {
                                    raw_cam_name,
                                    inner: flydra_types::UpdateImage {
                                        current_image_png: current_image_png.into(),
                                    },
                                },
                            );
                            match cb_sender.send(msg).await {
                                Ok(()) => {}
                                Err(e) => {
                                    tracing::error!("While sending current image: {e}");
                                    transmit_msg_tx = None;
                                }
                            };
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
                            if let Some(ref mut braidz_write_tx_weak) =
                                opt_braidz_write_tx_weak.as_mut()
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
                                if let Some(braidz_write_tx) = braidz_write_tx_weak.upgrade() {
                                    // `braidz_write_tx` will be dropped after this scope.
                                    braidz_write_tx
                                        .send(SaveToDiskMsg::StartSavingCsv(cfg))
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
                                match braidz_write_tx.send(SaveToDiskMsg::StopSavingCsv).await {
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
            Msg::SetClockModel(cm) => {
                opt_clock_model = cm;
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
                ufmf_state = Some(UfmfState::Stopped);
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

fn open_braid_destination_addr(dest_addr: &RealtimePointsDestAddr) -> Result<DatagramSocket> {
    info!(
        "Sending detected coordinates to: {}",
        dest_addr.into_string()
    );

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
        RealtimePointsDestAddr::UnixDomainSocket(_uds) => {
            Err(StrandCamError::UnixDomainSocketsNotSupported(
                #[cfg(feature = "backtrace")]
                Backtrace::capture(),
            ))
        }
        RealtimePointsDestAddr::IpAddr(dest_ip_addr) => {
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
        c => panic!("unknown channel {c}"),
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
    firehose_callback_tx: tokio::sync::mpsc::Sender<FirehoseCallback>,
    cam_args_tx: tokio::sync::mpsc::Sender<CamArg>,
    led_box_tx_std: tokio::sync::mpsc::Sender<ToLedBoxDevice>,
    #[allow(dead_code)]
    tx_frame: tokio::sync::mpsc::Sender<Msg>,
}

#[derive(Clone)]
struct StrandCamAppState {
    event_broadcaster: EventBroadcaster<ConnectionSessionKey>,
    callback_senders: StrandCamCallbackSenders,
    tx_new_connection: tokio::sync::mpsc::Sender<event_stream_types::ConnectionEvent>,
    shared_store_arc: Arc<parking_lot::RwLock<ChangeTracker<StoreType>>>,
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

type MyBody = http_body_util::combinators::BoxBody<bytes::Bytes, bui_backend_session::Error>;

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
    known_version: Arc<parking_lot::RwLock<semver::Version>>,
    app_name: &'static str,
) -> Result<()> {
    let url = format!("https://version-check.strawlab.org/{app_name}");
    let url = url.parse::<hyper::Uri>().unwrap();
    let agent = format!("{}/{}", app_name, *known_version.read());

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

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, Clone)]
pub enum TimestampSource {
    BraidTrigger,
    HostAcquiredTimestamp,
}

const MOMENT_CENTROID_SCHEMA_VERSION: u8 = 1;

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
pub struct MomentCentroid {
    pub schema_version: u8,
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
enum ToDevice {
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
    pub csv_save_dir: String,
    pub raise_grab_thread_priority: bool,
    #[cfg(feature = "posix_sched_fifo")]
    pub process_frame_priority: Option<(i32, i32)>,
    pub led_box_device_path: Option<String>,
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
            #[cfg(feature = "fiducial")]
            apriltag_csv_filename_template: strand_cam_storetype::APRILTAG_CSV_TEMPLATE_DEFAULT
                .to_string(),
            csv_save_dir: "/dev/null".to_string(),
            raise_grab_thread_priority: false,
            #[cfg(feature = "posix_sched_fifo")]
            process_frame_priority: None,
            led_box_device_path: None,
            #[cfg(feature = "plugin-process-frame")]
            process_frame_callback: None,
            #[cfg(feature = "plugin-process-frame")]
            plugin_wait_dur: std::time::Duration::from_millis(5),
            #[cfg(feature = "flydratrax")]
            flydratrax_calibration_source: CalSource::PseudoCal,
            #[cfg(feature = "flydratrax")]
            save_empty_data2d: true,
            #[cfg(feature = "flydratrax")]
            model_server_addr: flydra_types::DEFAULT_MODEL_SERVER_ADDR.parse().unwrap(),
        }
    }
}

fn test_nvenc_save(frame: DynamicFrame) -> Result<bool> {
    let cfg = Mp4RecordingConfig {
        codec: Mp4Codec::H264NvEnc(NvidiaH264Options {
            bitrate: 1000,
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

    let opts = ci2_remote_control::NvidiaH264Options {
        bitrate: 10000,
        ..Default::default()
    };

    nv_cfg_test.codec = ci2_remote_control::Mp4Codec::H264NvEnc(opts);

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
    mp4_writer.write_dynamic(&frame, chrono::Utc::now())?;
    mp4_writer.finish()?;

    debug!("MP4 video with nvenc h264 encoding succeeded.");

    // When `buf` goes out of scope, it will be dropped.
    Ok(true)
}

fn to_event_frame(state: &StoreType) -> String {
    let buf = serde_json::to_string(&state).unwrap();
    let frame_string = format!("event: {STRAND_CAM_EVENT_NAME}\ndata: {buf}\n\n");
    frame_string
}

async fn events_handler(
    axum::extract::State(app_state): axum::extract::State<StrandCamAppState>,
    session_key: axum_token_auth::SessionKey,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<SocketAddr>,
    _: AcceptsEventStream,
    req: axum::extract::Request,
) -> impl axum::response::IntoResponse {
    tracing::trace!("events");
    // Connection wants to subscribe to event stream.

    let key = ConnectionSessionKey::new(session_key.0, addr);
    let (tx, body) = app_state.event_broadcaster.new_connection(key);

    // Send an initial copy of our state.
    let shared_store = app_state.shared_store_arc.read().as_ref().clone();
    let frame_string = to_event_frame(&shared_store);
    match tx
        .send(Ok(http_body::Frame::data(frame_string.into())))
        .await
    {
        Ok(()) => {}
        Err(tokio::sync::mpsc::error::SendError(_)) => {
            // The receiver was dropped because the connection closed. Should probably do more here.
            tracing::debug!("initial send error");
        }
    }

    // Create a new channel in which the receiver is used to send responses to
    // the new connection. The sender receives changes from a global change
    // receiver.
    let typ = ConnectionEventType::Connect(tx);
    let path = req.uri().path().to_string();
    let connection_key = ConnectionKey { addr };
    let session_key = SessionKey(session_key.0);

    match app_state
        .tx_new_connection
        .send(ConnectionEvent {
            typ,
            session_key,
            connection_key,
            path,
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

async fn callback_handler(
    axum::extract::State(app_state): axum::extract::State<StrandCamAppState>,
    _session_key: axum_token_auth::SessionKey,
    TolerantJson(payload): TolerantJson<CallbackType>,
) -> impl axum::response::IntoResponse {
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
        CallbackType::FirehoseNotify(inner) => {
            let arrival_time = chrono::Utc::now();
            let fc = FirehoseCallback {
                arrival_time,
                inner,
            };
            app_state
                .callback_senders
                .firehose_callback_tx
                .send(fc)
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

#[derive(Debug)]
struct BraidInfo {
    mainbrain_session: braid_http_session::MainbrainSession,
    camdata_addr: flydra_types::RealtimePointsDestAddr,
    tracker_cfg_src: ImPtDetectCfgSource,
    config_from_braid: flydra_types::RemoteCameraInfoResponse,
}

/// Wrapper to enforce that first message is fixed to be
/// [flydra_types::RegisterNewCamera].
struct FirstMsgForced {
    tx: tokio::sync::mpsc::Sender<flydra_types::BraidHttpApiCallback>,
}

impl FirstMsgForced {
    /// Wrap a sender.
    fn new(tx: tokio::sync::mpsc::Sender<flydra_types::BraidHttpApiCallback>) -> Self {
        Self { tx }
    }

    /// Send the first message and return the Sender.
    async fn send_first_msg(
        self,
        new_cam_data: flydra_types::RegisterNewCamera,
    ) -> std::result::Result<
        tokio::sync::mpsc::Sender<flydra_types::BraidHttpApiCallback>,
        tokio::sync::mpsc::error::SendError<flydra_types::BraidHttpApiCallback>,
    > {
        self.tx
            .send(flydra_types::BraidHttpApiCallback::NewCamera(new_cam_data))
            .await?;
        Ok(self.tx)
    }
}

// -----------

/// top-level function once args are parsed from CLI.
pub fn run_app<M, C, G>(
    mymod: ci2_async::ThreadedAsyncCameraModule<M, C, G>,
    args: StrandCamArgs,
    app_name: &'static str,
) -> anyhow::Result<ci2_async::ThreadedAsyncCameraModule<M, C, G>>
where
    M: ci2::CameraModule<CameraType = C, Guard = G> + 'static,
    C: 'static + ci2::Camera + Send,
{
    // Start tokio runtime here.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(4)
        .thread_name("strand-cam-runtime")
        .thread_stack_size(3 * 1024 * 1024)
        .build()?;

    let mymod = runtime.block_on(run_after_maybe_connecting_to_braid(mymod, args, app_name))?;

    info!("done");
    Ok(mymod)
}

/// First, connect to Braid if requested, then run.
async fn run_after_maybe_connecting_to_braid<M, C, G>(
    mymod: ci2_async::ThreadedAsyncCameraModule<M, C, G>,
    args: StrandCamArgs,
    app_name: &'static str,
) -> anyhow::Result<ci2_async::ThreadedAsyncCameraModule<M, C, G>>
where
    M: ci2::CameraModule<CameraType = C, Guard = G> + 'static,
    C: 'static + ci2::Camera + Send,
{
    // If connecting to braid, do it here.
    let res_braid: std::result::Result<BraidInfo, StandaloneArgs> = {
        match &args.standalone_or_braid {
            StandaloneOrBraid::Braid(braid_args) => {
                info!("Will connect to braid at \"{}\"", braid_args.braid_url);
                let mainbrain_bui_loc = flydra_types::MainbrainBuiLocation(
                    flydra_types::BuiServerAddrInfo::parse_url_with_token(&braid_args.braid_url)?,
                );

                let jar: cookie_store::CookieStore =
                    match Preferences::load(&APP_INFO, BRAID_COOKIE_KEY) {
                        Ok(jar) => {
                            tracing::debug!("loaded cookie store {BRAID_COOKIE_KEY}");
                            jar
                        }
                        Err(e) => {
                            tracing::debug!(
                                "cookie store {BRAID_COOKIE_KEY} not loaded: {e} {e:?}"
                            );
                            cookie_store::CookieStore::new(None)
                        }
                    };
                let jar = Arc::new(parking_lot::RwLock::new(jar));
                let mut mainbrain_session =
                    braid_http_session::mainbrain_future_session(mainbrain_bui_loc, jar.clone())
                        .await?;
                tracing::debug!("Opened HTTP session with Braid.");
                {
                    // We have the cookie from braid now, so store it to disk.
                    let jar = jar.read();
                    Preferences::save(&*jar, &APP_INFO, BRAID_COOKIE_KEY)?;
                    tracing::debug!("saved cookie store {BRAID_COOKIE_KEY}");
                }

                let camera_name = flydra_types::RawCamName::new(braid_args.camera_name.clone());

                let config_from_braid: flydra_types::RemoteCameraInfoResponse =
                    mainbrain_session.get_remote_info(&camera_name).await?;

                let camdata_addr = {
                    let camdata_addr = config_from_braid
                        .camdata_addr
                        .parse::<std::net::SocketAddr>()?;
                    let addr_info_ip = flydra_types::AddrInfoIP::from_socket_addr(&camdata_addr);

                    flydra_types::RealtimePointsDestAddr::IpAddr(addr_info_ip)
                };

                let tracker_cfg_src = crate::ImPtDetectCfgSource::ChangesNotSavedToDisk(
                    config_from_braid.config.point_detection_config.clone(),
                );

                Ok(BraidInfo {
                    mainbrain_session,
                    config_from_braid,
                    camdata_addr,
                    tracker_cfg_src,
                })
            }
            StandaloneOrBraid::Standalone(standalone_args) => Err(standalone_args.clone()),
        }
    };

    select_cam_and_run(mymod, args, app_name, res_braid).await
}

/// Determine the camera name to be used and call `run()`.
async fn select_cam_and_run<M, C, G>(
    mymod: ci2_async::ThreadedAsyncCameraModule<M, C, G>,
    args: StrandCamArgs,
    app_name: &'static str,
    res_braid: std::result::Result<BraidInfo, StandaloneArgs>,
) -> anyhow::Result<ci2_async::ThreadedAsyncCameraModule<M, C, G>>
where
    M: ci2::CameraModule<CameraType = C, Guard = G>,
    C: 'static + ci2::Camera + Send,
{
    let strand_cam_bui_http_address_string = match &args.standalone_or_braid {
        StandaloneOrBraid::Braid(braid_args) => {
            let braid_info = match &res_braid {
                Ok(braid_info) => braid_info,
                Err(_) => {
                    anyhow::bail!("requested braid, but no braid config");
                }
            };
            let http_server_addr = braid_info.config_from_braid.config.http_server_addr.clone();
            let braid_info =
                flydra_types::BuiServerAddrInfo::parse_url_with_token(&braid_args.braid_url)?;

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
        return Err(StrandCamError::NoCamerasFound.into());
    }

    for cam_info in cam_infos.iter() {
        info!("  camera {:?} detected", cam_info.name());
    }

    let use_camera_name = match requested_camera_name {
        Some(ref name) => name,
        None => cam_infos[0].name(),
    };

    run(
        mymod,
        args,
        app_name,
        res_braid,
        strand_cam_bui_http_address_string,
        use_camera_name,
    )
    .await
}

// -----------

/// This is the main function where we spend all time after parsing startup args
/// and, in case of connecting to braid, getting the inital connection
/// information.
///
/// This function is way too huge and should be refactored.
#[tracing::instrument(skip(mymod, args, app_name, res_braid, strand_cam_bui_http_address_string))]
async fn run<M, C, G>(
    mut mymod: ci2_async::ThreadedAsyncCameraModule<M, C, G>,
    args: StrandCamArgs,
    app_name: &'static str,
    res_braid: std::result::Result<BraidInfo, StandaloneArgs>,
    strand_cam_bui_http_address_string: String,
    cam: &str,
) -> anyhow::Result<ci2_async::ThreadedAsyncCameraModule<M, C, G>>
where
    M: ci2::CameraModule<CameraType = C, Guard = G>,
    C: 'static + ci2::Camera + Send,
{
    let use_camera_name = cam; // simple arg name important for tracing::instrument
    let frame_info_extractor = mymod.frame_info_extractor();
    let settings_file_ext = mymod.settings_file_extension().to_string();

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

    let camera_settings_filename = match &res_braid {
        Ok(bi) => bi.config_from_braid.config.camera_settings_filename.clone(),
        Err(a) => a.camera_settings_filename.clone(),
    };

    let pixel_format = match &res_braid {
        Ok(bi) => bi.config_from_braid.config.pixel_format.clone(),
        Err(a) => a.pixel_format.clone(),
    };

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

    let (frame_rate_limit_supported, mut frame_rate_limit_enabled) =
        if let Some(fname) = &camera_settings_filename {
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

    // Put first frame in channel.
    firehose_tx
        .send(AnnotatedFrame {
            frame: frame.clone(),
            found_points: vec![],
            valid_display: None,
            annotations: vec![],
        })
        .await
        .unwrap();
    // .map_err(|e| anhow::anyhow!("failed to send frame"))?;

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

    #[cfg(feature = "flydratrax")]
    let save_empty_data2d = args.save_empty_data2d;

    #[cfg(feature = "flydra_feat_detect")]
    let tracker_cfg_src = match &res_braid {
        Ok(bi) => bi.tracker_cfg_src.clone(),
        Err(a) => a.tracker_cfg_src.clone(),
    };

    #[cfg(not(feature = "flydra_feat_detect"))]
    match &res_braid {
        Ok(bi) => {
            let _ = bi.tracker_cfg_src.clone(); // silence unused field warning.
        }
        Err(_) => {}
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

    let force_camera_sync_mode = match &res_braid {
        Ok(bi) => bi.config_from_braid.force_camera_sync_mode,
        Err(a) => a.force_camera_sync_mode,
    };

    let camdata_addr = match &res_braid {
        Ok(bi) => Some(bi.camdata_addr.clone()),
        Err(_a) => None,
    };

    let software_limit_framerate = match &res_braid {
        Ok(bi) => bi.config_from_braid.software_limit_framerate.clone(),
        Err(a) => a.software_limit_framerate.clone(),
    };

    let (mut mainbrain_session, ptpcfg) = match res_braid {
        Ok(bi) => (
            Some(bi.mainbrain_session),
            bi.config_from_braid.ptp_sync_config,
        ),
        Err(_a) => (None, None),
    };

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
    if let Some(val) = ptpcfg.as_ref().and_then(|c| c.periodic_signal_period_usec) {
        cam.feature_float_set(PERIOD_NAME, val)?;
    }

    let camera_periodic_signal_period_usec = {
        match cam.feature_float(PERIOD_NAME) {
            Ok(value) => Some(value),
            Err(e) => {
                tracing::debug!("Could not read feature {PERIOD_NAME}: {e}");
                None
            }
        }
    };

    if ptpcfg.is_some() {
        cam.feature_bool_set("PtpEnable", true)?;
        loop {
            cam.command_execute("PtpDataSetLatch", true)?;
            let ptp_offset_from_master = cam.feature_int("PtpOffsetFromMaster")?;
            tracing::debug!("PTP clock offset {ptp_offset_from_master} microseconds.");
            if ptp_offset_from_master.abs() < 1_000_000 {
                // if within 1 millisecond from master, call it good enough.
                break;
            }
            tracing::warn!(
                "PTP clock offset {ptp_offset_from_master} microseconds, waiting \
                for convergence."
            );
            tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
        }
        tracing::info!("PTP clock within threshold from master.");
    }

    let (cam_args_tx, cam_args_rx) = tokio::sync::mpsc::channel(100);
    let (led_box_tx_std, mut led_box_rx) = tokio::sync::mpsc::channel(20);

    let led_box_heartbeat_update_arc = Arc::new(parking_lot::RwLock::new(None));

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
        flydra_types::start_listener(&strand_cam_bui_http_address_string).await?;

    let mut transmit_msg_tx = None;
    if let Some(first_msg_tx) = first_msg_tx {
        let new_cam_data = flydra_types::RegisterNewCamera {
            raw_cam_name: raw_cam_name.clone(),
            http_camserver_info: Some(BuiServerInfo::Server(http_camserver_info.clone())),
            cam_settings_data: Some(flydra_types::UpdateCamSettings {
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
    let is_nvenc_functioning = test_nvenc_save(frame)?;

    let mp4_codec = match is_nvenc_functioning {
        true => CodecSelection::H264Nvenc,
        false => CodecSelection::H264OpenH264,
    };

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
        is_nvenc_functioning,
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
        led_box_device_path: args.led_box_device_path,
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

    let frame_processing_error_state = Arc::new(parking_lot::RwLock::new(
        FrameProcessingErrorState::default(),
    ));

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

    let shared_state = Arc::new(parking_lot::RwLock::new(shared_store));
    let shared_store_arc = shared_state.clone();

    // Create our app state.
    let app_state = StrandCamAppState {
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
            let frame_string = to_event_frame(&next_state);
            event_broadcaster.broadcast_frame(frame_string).await;
        }
    };

    #[cfg(feature = "bundle_files")]
    let serve_dir = tower_serve_static::ServeDir::new(&ASSETS_DIR);

    #[cfg(feature = "serve_files")]
    let serve_dir = tower_http::services::fs::ServeDir::new(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("yew_frontend")
            .join("pkg"),
    );

    let persistent_secret_base64 = if let Some(secret) = args.secret {
        secret
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
        .route("/callback", axum::routing::post(callback_handler))
        .nest_service("/", serve_dir)
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

    let url = http_camserver_info.build_url();

    // Display where we are listening.
    if is_braid {
        debug!("Strand Cam predicted URL: {url}");
    } else {
        info!("Strand Cam predicted URL: {url}");
        if !flydra_types::is_loopback(&url) {
            println!("QR code for {url}");
            display_qr_url(&format!("{url}"));
        }
    }

    #[cfg(feature = "plugin-process-frame")]
    let do_process_frame_callback = args.process_frame_callback.is_some();

    #[cfg(feature = "plugin-process-frame")]
    let process_frame_callback = args.process_frame_callback;

    #[cfg(feature = "checkercal")]
    let collected_corners_arc: CollectedCornersArc = Arc::new(parking_lot::RwLock::new(Vec::new()));

    let frame_process_task_fut = {
        #[cfg(feature = "flydra_feat_detect")]
        let csv_save_dir = args.csv_save_dir.clone();

        #[cfg(feature = "flydratrax")]
        let model_server_addr = flydra_types::get_best_remote_addr(&args.model_server_addr)?;

        #[cfg(feature = "flydratrax")]
        let led_box_tx_std = led_box_tx_std.clone();
        #[cfg(feature = "flydratrax")]
        let http_camserver_info2 = http_camserver_info.clone();
        let led_box_heartbeat_update_arc2 = led_box_heartbeat_update_arc.clone();
        #[cfg(feature = "flydratrax")]
        let (model_server_data_tx, flydratrax_calibration_source) = {
            info!("send_pose server at {}", model_server_addr.as_ref());
            let (model_server_data_tx, data_rx) = tokio::sync::mpsc::channel(50);
            let model_server_future =
                flydra2::new_model_server(data_rx, *model_server_addr.as_ref());
            tokio::spawn(async { model_server_future.await });
            let flydratrax_calibration_source = args.flydratrax_calibration_source;
            (model_server_data_tx, flydratrax_calibration_source)
        };

        let cam_name2 = raw_cam_name.clone();
        frame_process_task(
            #[cfg(feature = "flydratrax")]
            model_server_data_tx,
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
            im_pt_detect_cfg,
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
            #[cfg(feature = "flydratrax")]
            http_camserver_info2,
            process_frame_priority,
            transmit_msg_tx.clone(),
            camdata_addr,
            led_box_heartbeat_update_arc2,
            #[cfg(feature = "plugin-process-frame")]
            do_process_frame_callback,
            #[cfg(feature = "checkercal")]
            collected_corners_arc.clone(),
            #[cfg(feature = "flydratrax")]
            save_empty_data2d,
            #[cfg(feature = "flydra_feat_detect")]
            acquisition_duration_allowed_imprecision_msec,
            frame_info_extractor,
            #[cfg(feature = "flydra_feat_detect")]
            app_name,
        )
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
    let mut frame_stream = cam.frames(n_buffered_frames, async_thread_start)?;
    let cam_stream_future = {
        let shared_store_arc = shared_store_arc.clone();
        let frame_processing_error_state = frame_processing_error_state.clone();
        async move {
            while let Some(frame_msg) = frame_stream.next().await {
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
        let known_version = Arc::new(parking_lot::RwLock::new(app_version));

        // Create a stream to call our closure now and every 30 minutes.
        let interval_stream = tokio::time::interval(std::time::Duration::from_secs(1800));

        let mut interval_stream = tokio_stream::wrappers::IntervalStream::new(interval_stream);

        let known_version2 = known_version;
        let stream_future = async move {
            while interval_stream.next().await.is_some() {
                let https = HttpsConnector::new();
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
                            let mut tracker = shared_store_arc.write();
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
                            let mut tracker = shared_store_arc.write();
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
                        tracker.modify(|tracker| tracker.mp4_max_framerate = v);
                    }
                    CamArg::SetMp4CudaDevice(v) => {
                        let mut tracker = shared_store_arc.write();
                        tracker.modify(|tracker| tracker.mp4_cuda_device = v);
                    }
                    CamArg::SetMp4MaxFramerate(v) => {
                        let mut tracker = shared_store_arc.write();
                        tracker.modify(|tracker| tracker.mp4_max_framerate = v);
                    }
                    CamArg::SetMp4Bitrate(v) => {
                        let mut tracker = shared_store_arc.write();
                        tracker.modify(|tracker| tracker.mp4_bitrate = v);
                    }
                    CamArg::SetMp4Codec(v) => {
                        let mut tracker = shared_store_arc.write();
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
                    CamArg::SetIsRecordingMp4(do_recording) => {
                        // Copy values from cache and release the lock immediately.
                        let is_recording_mp4 = {
                            let tracker = shared_store_arc.read();
                            let shared: &StoreType = tracker.as_ref();
                            shared.is_recording_mp4.is_some()
                        };

                        if is_recording_mp4 != do_recording {
                            // Compute new values.
                            let msg = if do_recording {
                                info!("Start MP4 recording");

                                // change state
                                Msg::StartMp4
                            } else {
                                info!("Stopping MP4 recording");
                                Msg::StopMp4
                            };

                            // Send the command.
                            tx_frame2.send(msg).await?;
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
                        info!("Start MP4 recording via post trigger.");
                        tx_frame2.send(Msg::PostTriggerStartMp4).await?;
                    }
                    CamArg::SetPostTriggerBufferSize(size) => {
                        info!("Set post trigger buffer size to {size}.");
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
                            match camcal::compute_intrinsics::<f64>(size, &goodcorners) {
                                Ok(intrinsics) => {
                                    info!("got calibrated intrinsics: {:?}", intrinsics);

                                    // Convert from mvg to ROS format.
                                    let ci: opencv_ros_camera::RosCameraInfo<_> =
                                        opencv_ros_camera::NamedIntrinsicParameters {
                                            intrinsics,
                                            width: image_width as usize,
                                            height: image_height as usize,
                                            name: raw_cam_name.as_str().to_string(),
                                        }
                                        .into();

                                    let cal_dir = directories::BaseDirs::new()
                                        .as_ref()
                                        .map(|bd| {
                                            bd.config_dir().join(APP_INFO.name).join("camera_info")
                                        })
                                        .unwrap();

                                    let format_str =
                                        format!("{}.%Y%m%d_%H%M%S.yaml", raw_cam_name.as_str());
                                    let stamped = local.format(&format_str).to_string();
                                    let cam_info_file_stamped = cal_dir.join(stamped);

                                    let mut cam_info_file = cal_dir.clone();
                                    cam_info_file.push(raw_cam_name.as_str());
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
            tx_frame2.send(Msg::StopMp4).await?;
            #[cfg(feature = "flydra_feat_detect")]
            tx_frame2.send(Msg::StopUFMF).await?;
            #[cfg(feature = "flydra_feat_detect")]
            tx_frame2
                .send(Msg::SetIsSavingObjDetectionCsv(CsvSaveConfig::NotSaving))
                .await?;

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

        open_browser(format!("{url}"))?;
    }

    let connection_callback_rx = rx_new_connection;
    let firehose_task_join_handle = tokio::spawn(async {
        // The first thing this task does is pop a frame from firehose_rx, so we
        // should ensure there is one present.
        video_streaming::firehose_task(connection_callback_rx, firehose_rx, firehose_callback_rx)
            .await
            .unwrap();
    });

    #[cfg(feature = "plugin-process-frame")]
    let (plugin_streaming_control, plugin_streaming_jh) = {
        let cam_args_tx2 = cam_args_tx.clone();
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
            })?;
        (control, join_handle)
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
            let (mut writer, mut reader) = LedBoxCodec::default().framed(port).split();

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
                        let mut tracker = shared_store_arc.write();
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

    // Now run until first future returns, then exit.
    info!("Strand Cam launched.");
    tokio::select! {
        res = http_serve_future => {res?},
        res = cam_arg_future => {res?},
        _ = mainbrain_transmitter_fut => {},
        _ = send_updates_future => {},
        res = frame_process_task_fut => {res?},
        res = firehose_task_join_handle=> {res?},
    }
    info!("Strand Cam ending nicely. :)");

    #[cfg(feature = "plugin-process-frame")]
    {
        plugin_streaming_control.stop();
        while !plugin_streaming_control.is_done() {
            debug!(
                "waiting for stop {:?} {:?}",
                plugin_streaming_jh.thread().name(),
                plugin_streaming_jh.thread().id()
            );
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    Ok(mymod)
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

async fn send_cam_settings_to_braid(
    cam_settings: &str,
    transmit_msg_tx: &tokio::sync::mpsc::Sender<flydra_types::BraidHttpApiCallback>,
    current_cam_settings_extension: &str,
    raw_cam_name: &RawCamName,
) -> StdResult<(), tokio::sync::mpsc::error::SendError<flydra_types::BraidHttpApiCallback>> {
    let current_cam_settings_buf = cam_settings.to_string();
    let current_cam_settings_extension = current_cam_settings_extension.to_string();
    let raw_cam_name = raw_cam_name.clone();
    let transmit_msg_tx = transmit_msg_tx.clone();

    let msg = flydra_types::BraidHttpApiCallback::UpdateCamSettings(flydra_types::PerCam {
        raw_cam_name,
        inner: flydra_types::UpdateCamSettings {
            current_cam_settings_buf,
            current_cam_settings_extension,
        },
    });
    transmit_msg_tx.send(msg).await
}

fn bitrate_to_u32(br: &ci2_remote_control::BitrateSelection) -> u32 {
    use ci2_remote_control::BitrateSelection::*;
    match br {
        Bitrate500 => 500,
        Bitrate1000 => 1000,
        Bitrate2000 => 2000,
        Bitrate3000 => 3000,
        Bitrate4000 => 4000,
        Bitrate5000 => 5000,
        Bitrate10000 => 10000,
        BitrateUnlimited => std::u32::MAX,
    }
}

struct FinalMp4RecordingConfig {
    final_cfg: Mp4RecordingConfig,
}

impl FinalMp4RecordingConfig {
    fn new(shared: &StoreType, creation_time: chrono::DateTime<chrono::Local>) -> Self {
        let cuda_device = shared
            .cuda_devices
            .iter()
            .position(|x| x == &shared.mp4_cuda_device)
            .unwrap_or(0);
        let cuda_device = cuda_device.try_into().unwrap();
        let codec = match shared.mp4_codec {
            CodecSelection::H264Nvenc => Mp4Codec::H264NvEnc(NvidiaH264Options {
                bitrate: bitrate_to_u32(&shared.mp4_bitrate),
                cuda_device,
            }),
            CodecSelection::H264OpenH264 => {
                let preset = ci2_remote_control::OpenH264Preset::AllFrames;
                if shared.mp4_bitrate != ci2_remote_control::BitrateSelection::BitrateUnlimited {
                    warn!("ignoring mp4 bitrate with OpenH264 codec");
                }
                Mp4Codec::H264OpenH264(ci2_remote_control::OpenH264Options {
                    debug: false,
                    preset,
                })
            }
        };
        // See https://github.com/chronotope/chrono/issues/576
        let fixed = chrono::DateTime::<chrono::FixedOffset>::from_naive_utc_and_offset(
            creation_time.naive_utc(),
            *creation_time.offset(),
        );

        let mut h264_metadata = ci2_remote_control::H264Metadata::new("strand-cam", fixed);
        h264_metadata.camera_name = Some(shared.camera_name.clone());
        h264_metadata.gamma = shared.camera_gamma;
        let final_cfg = Mp4RecordingConfig {
            codec,
            max_framerate: shared.mp4_max_framerate.clone(),
            h264_metadata: Some(h264_metadata),
        };
        FinalMp4RecordingConfig { final_cfg }
    }
}
