use std::{
    collections::BTreeMap,
    net::SocketAddr,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, RwLock,
    },
};

use async_change_tracker::ChangeTracker;
use axum::{
    extract::{Path, State},
    routing::get,
};
use futures::StreamExt;
use http::{HeaderValue, StatusCode};
use preferences_serde1::{AppInfo, Preferences};
use serde::Serialize;
use tokio::net::UdpSocket;
use tower_http::trace::TraceLayer;
use tracing::{debug, error, info};

use bui_backend_session_types::AccessToken;
use event_stream_types::{AcceptsEventStream, EventBroadcaster};
use flydra2::{CoordProcessor, CoordProcessorConfig, FrameDataAndPoints, StreamItem};
use flydra_types::{
    braid_http::{CAM_PROXY_PATH, REMOTE_CAMERA_INFO_PATH},
    BraidHttpApiSharedState, BuiServerAddrInfo, CamInfo, CborPacketCodec, FakeSyncConfig,
    FlydraFloatTimestampLocal, HostClock, PerCamSaveData, RawCamName, SyncFno, TriggerType,
    Triggerbox, BRAID_EVENTS_URL_PATH, BRAID_EVENT_NAME, TRIGGERBOX_SYNC_SECONDS,
};
use rust_cam_bui_types::{ClockModel, RecordingPath};

use eyre::{self, Result, WrapErr};

use crate::multicam_http_session_handler::{MaybeSession, StrandCamHttpSessionHandler};

#[cfg(feature = "bundle_files")]
static ASSETS_DIR: include_dir::Dir<'static> =
    include_dir::include_dir!("$CARGO_MANIFEST_DIR/braid_frontend/pkg");

lazy_static::lazy_static! {
    static ref EVENTS_PREFIX: String = format!("/{}", BRAID_EVENTS_URL_PATH);
}

pub(crate) const APP_INFO: AppInfo = AppInfo {
    name: "braid",
    author: "AndrewStraw",
};
const COOKIE_SECRET_KEY: &str = "cookie-secret-base64";
pub(crate) const STRAND_CAM_COOKIE_KEY: &str = "strand-cam-cookie";

type SharedStore = Arc<RwLock<ChangeTracker<BraidHttpApiSharedState>>>;

#[derive(thiserror::Error, Debug)]
pub(crate) enum MainbrainError {
    #[error("{source}")]
    HyperError {
        #[from]
        source: hyper::Error,
    },
    #[error("{source}")]
    BuiBackendSessionError {
        #[from]
        source: bui_backend_session::Error,
    },
    #[error("{source}")]
    PreferencesError {
        #[from]
        source: preferences_serde1::PreferencesError,
    },
    #[error("unknown camera \"{cam_name}\"")]
    UnknownCamera { cam_name: RawCamName },
}

pub(crate) type MainbrainResult<T> = std::result::Result<T, MainbrainError>;

/// The structure that holds our app data
#[derive(Clone)]
pub(crate) struct BraidAppState {
    pub(crate) shared_store: SharedStore,
    lowlatency_camdata_udp_addr: SocketAddr,
    force_camera_sync_mode: bool,
    software_limit_framerate: flydra_types::StartSoftwareFrameRateLimit,
    event_broadcaster: EventBroadcaster<usize>,
    pub(crate) per_cam_data_arc: Arc<RwLock<BTreeMap<RawCamName, PerCamSaveData>>>,
    pub(crate) expected_framerate_arc: Arc<RwLock<Option<f32>>>,
    camera_configs: BTreeMap<RawCamName, flydra_types::BraidCameraConfig>,
    next_connection_id: Arc<RwLock<usize>>,
    pub(crate) strand_cam_http_session_handler: StrandCamHttpSessionHandler,
    pub(crate) cam_manager: flydra2::ConnectedCamerasManager,
    pub(crate) output_base_dirname: PathBuf,
    pub(crate) braidz_write_tx_weak: tokio::sync::mpsc::WeakSender<flydra2::SaveToDiskMsg>,
}

async fn events_handler(
    State(app_state): State<BraidAppState>,
    session_key: axum_token_auth::SessionKey,
    _: AcceptsEventStream,
) -> impl axum::response::IntoResponse {
    session_key.is_present();
    let key = {
        let mut next_connection_id = app_state.next_connection_id.write().unwrap();
        let key = *next_connection_id;
        *next_connection_id += 1;
        key
    };
    let (tx, body) = app_state.event_broadcaster.new_connection(key);

    // Send an initial copy of our state.
    {
        let current_state = app_state.shared_store.read().unwrap().as_ref().clone();
        let frame_string = to_event_frame(&current_state);
        match tx
            .send(Ok(http_body::Frame::data(frame_string.into())))
            .await
        {
            Ok(()) => {}
            Err(_) => {
                // The receiver was dropped because the connection closed. Should probably do more here.
                tracing::debug!("initial send error");
            }
        }
    }

    body
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

/// Query the mainbrain configuration to get data required for camera settings.
///
/// Note that this does not change the state of the mainbrain to register
/// anything about the camera but only queries for its configuration.
/// Registration of a new camera is done by
/// [flydra_types::BraidHttpApiCallback::NewCamera].
async fn remote_camera_info_handler(
    State(app_state): State<BraidAppState>,
    session_key: axum_token_auth::SessionKey,
    Path(raw_cam_name): Path<String>,
) -> impl axum::response::IntoResponse {
    session_key.is_present();
    let cam_cfg = app_state
        .camera_configs
        .get(&RawCamName::new(raw_cam_name.clone()));

    if let Some(config) = cam_cfg {
        let software_limit_framerate = app_state.software_limit_framerate.clone();

        let trig_config = app_state
            .shared_store
            .read()
            .unwrap()
            .as_ref()
            .trigger_type
            .clone();

        let msg = flydra_types::RemoteCameraInfoResponse {
            camdata_udp_port: app_state.lowlatency_camdata_udp_addr.port(),
            config: config.clone(),
            force_camera_sync_mode: app_state.force_camera_sync_mode,
            software_limit_framerate,
            trig_config,
        };
        Ok(axum::Json(msg))
    } else {
        error!("HTTP camera not found: \"{raw_cam_name:?}\"");
        Err((
            StatusCode::NOT_FOUND,
            format!("Camera \"{raw_cam_name}\" not found."),
        ))
    }
}

async fn cam_proxy_handler_inner(
    app_state: BraidAppState,
    session_key: axum_token_auth::SessionKey,
    raw_cam_name: String,
    cam_path: String,
    req: axum::extract::Request,
) -> impl axum::response::IntoResponse {
    session_key.is_present();
    tracing::debug!("raw_cam_name: {raw_cam_name}, cam_path: \"{cam_path}\", req: {req:?}");
    let accepts: Vec<HeaderValue> = req
        .headers()
        .get_all(http::header::ACCEPT)
        .iter()
        .cloned()
        .collect();
    let cam_name = RawCamName::new(raw_cam_name);

    let session = app_state
        .strand_cam_http_session_handler
        .get_or_open_session(&cam_name)
        .await
        .map_err(|e| match e {
            MainbrainError::UnknownCamera { cam_name, .. } => {
                let err_msg = format!("Unknown camera \"{cam_name}\"");
                tracing::error!(err_msg);
                (StatusCode::NOT_FOUND, err_msg)
            }
            _ => {
                let err_msg = format!("Internal server error: {e} {e:?}");
                tracing::error!(err_msg);
                (StatusCode::INTERNAL_SERVER_ERROR, err_msg)
            }
        })?;

    match session {
        MaybeSession::Alive(mut session) => {
            tracing::debug!("Will request path \"{cam_path}\". Got session {session:?}.");
            session
                .req_accepts(&cam_path, &accepts, req.method().clone(), req.into_body())
                .await
                .map_err(|e| {
                    let err_msg = format!("Failed request to Strand Cam: {e} {e:?}");
                    tracing::error!(err_msg);
                    (StatusCode::INTERNAL_SERVER_ERROR, err_msg)
                })
        }
        MaybeSession::Errored => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!(
                "Braid lost connection to camera name \"{}\".",
                cam_name.as_str()
            ),
        )),
    }
}

async fn cam_proxy_handler_root(
    State(app_state): State<BraidAppState>,
    session_key: axum_token_auth::SessionKey,
    Path(raw_cam_name): Path<String>,
    req: axum::extract::Request,
) -> impl axum::response::IntoResponse {
    session_key.is_present();
    cam_proxy_handler_inner(app_state, session_key, raw_cam_name, "".into(), req).await
}

async fn cam_proxy_handler(
    State(app_state): State<BraidAppState>,
    session_key: axum_token_auth::SessionKey,
    Path((raw_cam_name, cam_path)): Path<(String, String)>,
    req: axum::extract::Request,
) -> impl axum::response::IntoResponse {
    session_key.is_present();
    cam_proxy_handler_inner(app_state, session_key, raw_cam_name, cam_path, req).await
}

async fn launch_braid_http_backend(
    secret_base64: Option<String>,
    listener: tokio::net::TcpListener,
    mainbrain_server_info: BuiServerAddrInfo,
    app_state: BraidAppState,
) -> Result<impl futures::Future<Output = Result<()>>> {
    let persistent_secret_base64 = if let Some(secret) = secret_base64 {
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
    let token_config = match mainbrain_server_info.token() {
        AccessToken::PreSharedToken(value) => Some(axum_token_auth::TokenConfig {
            name: "token".to_string(),
            value: value.clone(),
        }),
        AccessToken::NoToken => None,
    };

    let cfg = axum_token_auth::AuthConfig {
        token_config,
        persistent_secret,
        cookie_name: "braid-bui-session",
        cookie_expires: Some(std::time::Duration::from_secs(60 * 60 * 24 * 400)), // 400 days
    };

    #[cfg(feature = "bundle_files")]
    let serve_dir = tower_serve_static::ServeDir::new(&ASSETS_DIR);

    #[cfg(feature = "serve_files")]
    let serve_dir = tower_http::services::fs::ServeDir::new(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("braid_frontend")
            .join("pkg"),
    );

    let auth_layer = cfg.into_layer();

    assert_eq!(BRAID_EVENTS_URL_PATH, "braid-events");
    assert_eq!(REMOTE_CAMERA_INFO_PATH, "remote-camera-info");
    assert_eq!(CAM_PROXY_PATH, "cam-proxy");

    // Create axum router.
    let router = axum::Router::new()
        .route("/braid-events", get(events_handler))
        .route(
            "/remote-camera-info/:encoded_cam_name",
            get(remote_camera_info_handler),
        )
        // .route("/cam-proxy/:encoded_cam_name", get(slash_redirect_handler))
        .route(
            "/cam-proxy/:encoded_cam_name/",
            axum::routing::method_routing::any(cam_proxy_handler_root),
        )
        .route(
            "/cam-proxy/:encoded_cam_name/*path",
            axum::routing::method_routing::any(cam_proxy_handler),
        )
        .route(
            "/callback",
            axum::routing::post(crate::callback_handling::callback_handler)
                .layer(axum::extract::DefaultBodyLimit::max(100_000_000)),
        )
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
        use futures::TryFutureExt;
        use std::future::IntoFuture;
        axum::serve(listener, router)
            .into_future()
            .map_err(eyre::Report::from)
    };

    // Display where we are listening.
    info!(
        "Braid HTTP server listening at {}",
        mainbrain_server_info.addr()
    );

    let urls = mainbrain_server_info.build_urls()?;
    for url in urls.iter() {
        info!("Predicted URL: {url}");
        if !flydra_types::is_loopback(url) {
            println!("QR code for {url}");
            display_qr_url(&format!("{url}"));
        }
    }

    Ok(http_serve_future)
}

fn compute_trigger_timestamp(
    model: &Option<ClockModel>,
    synced_frame: SyncFno,
) -> Option<FlydraFloatTimestampLocal<Triggerbox>> {
    if let Some(model) = model {
        let v: f64 = (synced_frame.0 as f64) * model.gain + model.offset;
        Some(FlydraFloatTimestampLocal::from_f64(v))
    } else {
        None
    }
}

struct SendConnectedCamToBuiBackend {
    shared_store: SharedStore,
}

impl flydra2::ConnectedCamCallback for SendConnectedCamToBuiBackend {
    fn on_cam_changed(&self, new_cam_list: Vec<CamInfo>) {
        let mut tracker = self.shared_store.write().unwrap();
        tracker.modify(|shared| shared.connected_cameras = new_cam_list.clone());
    }
}

fn display_qr_url(url: &str) {
    use qrcodegen::{QrCode, QrCodeEcc};
    use std::io::{stdout, Write};

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

/// Format for debugging raw packet data direct from Strand Cam.
#[derive(Serialize)]
struct RawPacketLogRow {
    cam_name: String,
    #[serde(with = "flydra_types::timestamp_opt_f64")]
    timestamp: Option<FlydraFloatTimestampLocal<Triggerbox>>,
    #[serde(with = "flydra_types::timestamp_f64")]
    cam_received_time: FlydraFloatTimestampLocal<HostClock>,
    device_timestamp: Option<std::num::NonZeroU64>,
    block_id: Option<std::num::NonZeroU64>,
    framenumber: i32,
    n_frames_skipped: u32,
    done_camnode_processing: f64,
    preprocess_stamp: f64,
    cam_num: Option<flydra_types::CamNum>,
    synced_frame: Option<SyncFno>,
}

/// Logger for debugging raw packet data direct from Strand Cam.
struct RawPacketLogger {
    fd: Option<csv::Writer<std::fs::File>>,
}

impl RawPacketLogger {
    /// Create a new logger for debugging raw packet data.
    ///
    /// If `fname` argument is None, this does very little.
    fn new(fname: Option<&std::path::Path>) -> Result<Self> {
        let fd = fname
            .map(std::fs::File::create)
            .transpose()?
            .map(csv::Writer::from_writer);
        Ok(Self { fd })
    }

    /// Log debug data for raw packets.
    ///
    /// If no filename was given to `Self::new`, this does very little.
    fn log_raw_packets(
        &mut self,
        packet: &flydra_types::FlydraRawUdpPacket,
        cam_num: Option<flydra_types::CamNum>,
        synced_frame: Option<SyncFno>,
    ) -> Result<()> {
        if let Some(ref mut fd) = self.fd {
            let row = RawPacketLogRow {
                cam_name: packet.cam_name.clone(),
                timestamp: packet.timestamp.clone(),
                cam_received_time: packet.cam_received_time.clone(),
                device_timestamp: packet.device_timestamp,
                block_id: packet.block_id,
                framenumber: packet.framenumber,
                n_frames_skipped: packet.n_frames_skipped,
                done_camnode_processing: packet.done_camnode_processing,
                preprocess_stamp: packet.preprocess_stamp,
                cam_num,
                synced_frame,
            };
            fd.serialize(row)?;
        }
        Ok(())
    }
}

pub(crate) async fn do_run_forever(
    show_tracking_params: bool,
    // sched_policy_priority: Option<(libc::c_int, libc::c_int)>,
    camera_configs: BTreeMap<RawCamName, flydra_types::BraidCameraConfig>,
    trigger_cfg: TriggerType,
    mainbrain_config: braid_config_data::MainbrainConfig,
    secret_base64: Option<String>,
    all_expected_cameras: std::collections::BTreeSet<RawCamName>,
    force_camera_sync_mode: bool,
    software_limit_framerate: flydra_types::StartSoftwareFrameRateLimit,
    saving_program_name: &str,
    listener: tokio::net::TcpListener,
    mainbrain_server_info: BuiServerAddrInfo,
    mut strand_cam_set: tokio::task::JoinSet<()>,
) -> Result<()> {
    let cal_fname: Option<std::path::PathBuf> = mainbrain_config.cal_fname.clone();
    let output_base_dirname: std::path::PathBuf = mainbrain_config.output_base_dirname.clone();
    let tracking_params: flydra_types::TrackingParams = mainbrain_config.tracking_params.clone();

    let lowlatency_camdata_udp_port = &mainbrain_config.lowlatency_camdata_udp_port;
    let mut ensure_camdata_ip = None;
    if let Some(lowlatency_camdata_udp_addr) = &mainbrain_config.lowlatency_camdata_udp_addr {
        tracing::warn!("Using deprecated configuration `lowlatency_camdata_udp_addr`. Use `lowlatency_camdata_udp_port` instead.");
        let lowlatency_camdata_udp_addr = lowlatency_camdata_udp_addr.parse::<SocketAddr>()?;
        if lowlatency_camdata_udp_addr.port() != *lowlatency_camdata_udp_port {
            eyre::bail!("camdata UDP port specified two different ways");
        }
        ensure_camdata_ip = Some(lowlatency_camdata_udp_addr.ip());
    }

    let save_empty_data2d: bool = mainbrain_config.save_empty_data2d;
    let write_buffer_size_num_messages = mainbrain_config.write_buffer_size_num_messages;

    info!("saving to directory: {}", output_base_dirname.display());

    // Create `stream_cancel::Valve` for shutting everything down. Note this is
    // `Clone`, so we can (and should) shut down everything with it.
    let (quit_trigger, valve) = stream_cancel::Valve::new();
    let (_shtdwn_q_tx, mut shtdwn_q_rx) = tokio::sync::mpsc::channel::<()>(5);

    let recon = if let Some(ref cal_fname) = cal_fname {
        info!("using calibration: {}", cal_fname.display());
        Some(
            flydra_mvg::FlydraMultiCameraSystem::from_path(cal_fname).with_context(|| {
                format!("loading calibration in file \"{}\"", cal_fname.display())
            })?,
        )
    } else {
        None
    };

    let signal_all_cams_present = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let signal_all_cams_synced = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

    let periodic_signal_period_usec = if let TriggerType::PtpSync(ptpcfg) = &trigger_cfg {
        ptpcfg.periodic_signal_period_usec
    } else {
        None
    };

    let mut cam_manager = flydra2::ConnectedCamerasManager::new(
        &recon,
        all_expected_cameras,
        signal_all_cams_present.clone(),
        signal_all_cams_synced.clone(),
        periodic_signal_period_usec,
    );

    let jar: cookie_store::CookieStore = match Preferences::load(&APP_INFO, STRAND_CAM_COOKIE_KEY) {
        Ok(jar) => {
            tracing::debug!("loaded cookie store {STRAND_CAM_COOKIE_KEY}");
            jar
        }
        Err(e) => {
            tracing::debug!("cookie store {STRAND_CAM_COOKIE_KEY} not loaded: {e} {e:?}");
            cookie_store::CookieStore::new(None)
        }
    };
    let jar = Arc::new(RwLock::new(jar.clone()));
    let strand_cam_http_session_handler =
        StrandCamHttpSessionHandler::new(cam_manager.clone(), jar);

    if show_tracking_params {
        let t2: flydra_types::TrackingParams = tracking_params;
        let buf = toml::to_string(&t2)?;
        println!("{}", buf);
        std::process::exit(0);
    }

    let ignore_latency = false;
    let mut coord_processor = CoordProcessor::new(
        CoordProcessorConfig {
            tracking_params,
            save_empty_data2d,
            ignore_latency,
            mini_arena_debug_image_dir: None,
            write_buffer_size_num_messages,
        },
        cam_manager.clone(),
        recon.clone(),
        flydra2::BraidMetadataBuilder::saving_program_name(saving_program_name),
    )?;

    // Here is what we do on quit:
    // 1) Stop saving data, convert .braid dir to .braidz, close files.
    // 2) Fire a DoQuit message to all cameras and wait for them to quit.
    // 3) Only then close all our network ports and streams nicely.
    let mut quit_trigger_container = Some(quit_trigger);
    let mut strand_cam_http_session_handler2 = strand_cam_http_session_handler.clone();
    let braidz_write_tx_weak = coord_processor.braidz_write_tx.downgrade();
    tokio::spawn(async move {
        while let Some(()) = shtdwn_q_rx.recv().await {
            debug!("got shutdown command {}:{}", file!(), line!());

            if let Some(braidz_write_tx) = braidz_write_tx_weak.upgrade() {
                // `braidz_write_tx` will be dropped after this scope.

                // Stop saving Braid data.

                // Do not need to wait for completion because we are going to
                // exit nicely by manually ending all threads and letting all
                // drop handlers run (without aborting) and thus the program
                // will finish writing without an explicit wait. (Of course,
                // this fails during an actual abort).

                braidz_write_tx
                    .send(flydra2::SaveToDiskMsg::StopSavingCsv)
                    .await
                    .unwrap_or(()); // ignore error on shutdown
            }

            strand_cam_http_session_handler2.send_quit_all().await;

            // When we get here, we have successfully sent DoQuit to all cams.
            // We can now quit everything in the mainbrain.
            if let Some(quit_trigger) = quit_trigger_container.take() {
                quit_trigger.cancel();
                break; // no point to listen for more
            }
        }
        debug!("shutdown handler finished {}:{}", file!(), line!());
    });

    let (triggerbox_cmd, triggerbox_rx) = match &trigger_cfg {
        TriggerType::TriggerboxV1(_) => {
            let (tx, rx) = tokio::sync::mpsc::channel(20);
            (Some(tx), Some(rx))
        }
        TriggerType::FakeSync(_) | TriggerType::PtpSync(_) | TriggerType::DeviceTimestamp => {
            (None, None)
        }
    };

    let needs_clock_model = match &trigger_cfg {
        TriggerType::TriggerboxV1(_) | TriggerType::FakeSync(_) => true,
        TriggerType::PtpSync(_) | TriggerType::DeviceTimestamp => false,
    };

    let sync_pulse_pause_started: Option<std::time::Instant> = None;
    let sync_pulse_pause_started_arc = Arc::new(RwLock::new(sync_pulse_pause_started));

    let flydra_app_name = "Braid".to_string();

    let shared = BraidHttpApiSharedState {
        trigger_type: trigger_cfg.clone(),
        csv_tables_dirname: None,
        fake_mp4_recording_path: None,
        post_trigger_buffer_size: 0,
        clock_model: None,
        calibration_filename: cal_fname.map(|x| x.into_os_string().into_string().unwrap()),
        connected_cameras: Vec::new(),
        model_server_addr: None,
        flydra_app_name,
        all_expected_cameras_are_synced: false,
        needs_clock_model,
    };
    let shared_store = ChangeTracker::new(shared);
    let mut shared_store_changes_rx = shared_store.get_changes(1);
    let shared_store = Arc::new(RwLock::new(shared_store));

    let expected_framerate_arc = Arc::new(RwLock::new(None));

    let per_cam_data_arc = Arc::new(RwLock::new(Default::default()));

    let (lowlatency_camdata_udp_addr, camdata_socket) = {
        // The port of the low latency UDP incoming data socket may be specified
        // as 0 in which case the OS will decide which port will actually be
        // bound. So here we create the socket and get its port.
        let camdata_addr_unspecified_port = {
            // No low latency UDP port specified. Default to the same IP
            // as the mainbrain HTTP server (which may be unspecified) and
            // let the OS assign a free port by setting the port as
            // unspecified.
            let mainbrain_tcp_addr = listener.local_addr()?;
            if let Some(ensure_camdata_ip) = ensure_camdata_ip {
                if mainbrain_tcp_addr.ip() != ensure_camdata_ip {
                    eyre::bail!(
                        "requested camdata UDP IP address not equal to mainbrain TCP IP address"
                    );
                }
            }
            let mut camdata_addr_unspecified_port = mainbrain_tcp_addr;
            camdata_addr_unspecified_port.set_port(*lowlatency_camdata_udp_port);
            camdata_addr_unspecified_port
        };
        let camdata_socket = UdpSocket::bind(&camdata_addr_unspecified_port).await?;
        let camdata_addr = camdata_socket.local_addr()?;
        debug!("flydra mainbrain camera UDP listener socket: internal: {camdata_addr}");

        (camdata_addr, camdata_socket)
    };

    if !output_base_dirname.exists() {
        info!(
            "creating output data directory at \"{}\"",
            output_base_dirname.display()
        );
        std::fs::create_dir_all(&output_base_dirname)?;
    }

    debug!(
        "output .braidz data directory will be \"{}\"",
        std::fs::canonicalize(&output_base_dirname)?.display()
    );

    let braidz_write_tx_weak = coord_processor.braidz_write_tx.downgrade();

    let time_model_arc = Arc::new(RwLock::new(None));

    // Create our app state.
    let app_state = BraidAppState {
        shared_store: shared_store.clone(),
        lowlatency_camdata_udp_addr,
        force_camera_sync_mode,
        software_limit_framerate,
        event_broadcaster: Default::default(),
        per_cam_data_arc: per_cam_data_arc.clone(),
        camera_configs,
        next_connection_id: Arc::new(RwLock::new(0)),
        expected_framerate_arc: expected_framerate_arc.clone(),
        braidz_write_tx_weak,
        cam_manager: cam_manager.clone(),
        output_base_dirname,
        strand_cam_http_session_handler: strand_cam_http_session_handler.clone(),
    };

    // This future will send state updates to all connected event listeners.
    let event_broadcaster = app_state.event_broadcaster.clone();
    let event_broadcast_fut = async move {
        while let Some((_prev_state, next_state)) = shared_store_changes_rx.next().await {
            let frame_string = to_event_frame(&next_state);
            event_broadcaster.broadcast_frame(frame_string).await;
        }
    };

    let http_serve_future =
        launch_braid_http_backend(secret_base64, listener, mainbrain_server_info, app_state)
            .await?;

    let signal_triggerbox_connected = Arc::new(AtomicBool::new(false));

    {
        let sender = SendConnectedCamToBuiBackend {
            shared_store: shared_store.clone(),
        };
        let old_callback = cam_manager.set_cam_changed_callback(Box::new(sender));
        assert!(old_callback.is_none());
    }

    let (triggerbox_data_tx, mut triggerbox_data_rx) =
        tokio::sync::mpsc::channel::<braid_triggerbox::TriggerClockInfoRow>(20);

    match &trigger_cfg {
        TriggerType::TriggerboxV1(_) | TriggerType::FakeSync(_) => {
            let braidz_write_tx_weak = coord_processor.braidz_write_tx.downgrade();
            let signal_triggerbox_connected = signal_triggerbox_connected.clone();

            let mut has_triggerbox_connected = false;
            let triggerbox_future = async move {
                debug!(
                    "starting triggerbox listener future {}:{}",
                    file!(),
                    line!()
                );
                while let Some(msg) = triggerbox_data_rx.recv().await {
                    if !has_triggerbox_connected {
                        has_triggerbox_connected = true;
                        info!("triggerbox is connected.");
                        signal_triggerbox_connected.store(true, Ordering::SeqCst);
                    }
                    let msg2 = flydra_types::TriggerClockInfoRow {
                        start_timestamp: msg.start_timestamp.into(),
                        framecount: msg.framecount,
                        tcnt: msg.tcnt,
                        stop_timestamp: msg.stop_timestamp.into(),
                    };

                    if let Some(braidz_write_tx) = braidz_write_tx_weak.upgrade() {
                        // `braidz_write_tx` will be dropped after this scope.
                        braidz_write_tx
                            .send(flydra2::SaveToDiskMsg::TriggerClockInfo(msg2))
                            .await
                            .unwrap();
                    }
                }
                debug!("triggerbox listener future done {}:{}", file!(), line!());
            };
            tokio::spawn(triggerbox_future);
        }
        _ => {
            debug!("not listening to triggerbox");
        }
    }

    let tracker = shared_store.clone();

    let on_new_clock_model = {
        let time_model_arc = time_model_arc.clone();
        let strand_cam_http_session_handler = strand_cam_http_session_handler.clone();
        let tracker = tracker.clone();
        let trigger_cfg = trigger_cfg.clone();
        Box::new(move |tm1: Option<braid_triggerbox::ClockModel>| {
            match &trigger_cfg {
                TriggerType::FakeSync(_) | TriggerType::TriggerboxV1(_) => {
                    let tm = tm1.map(|x| rust_cam_bui_types::ClockModel {
                        gain: x.gain,
                        offset: x.offset,
                        n_measurements: x.n_measurements,
                        residuals: x.residuals,
                    });
                    let cm = tm.clone();
                    {
                        let mut guard = time_model_arc.write().unwrap();
                        *guard = tm;
                    }
                    {
                        let mut tracker_guard = tracker.write().unwrap();
                        tracker_guard.modify(|shared| shared.clock_model = cm.clone());
                    }
                    let strand_cam_http_session_handler2 = strand_cam_http_session_handler.clone();
                    // TODO: Do we really need to spawn here? Why not just .await?
                    tokio::spawn(async move {
                        let r = strand_cam_http_session_handler2
                            .send_clock_model_to_all(cm)
                            .await;
                        match r {
                            Ok(_http_response) => {}
                            Err(e) => {
                                error!("error sending clock model: {}", e);
                            }
                        };
                    });
                }
                TriggerType::PtpSync(_) | TriggerType::DeviceTimestamp => {
                    // no central clock model
                    panic!("No need for clock model.");
                }
            }
        })
    };

    match &trigger_cfg {
        TriggerType::TriggerboxV1(cfg) => {
            let device_fname = cfg.device_fname.clone();
            let fps = &cfg.framerate;
            let query_dt = &cfg.query_dt;

            use braid_triggerbox::{make_trig_fps_cmd, Cmd};

            let tx = triggerbox_cmd.clone().unwrap();
            let cmd_rx = triggerbox_rx.unwrap();

            let (rate_cmd, rate_actual) = make_trig_fps_cmd(*fps as f64);

            let max_triggerbox_measurement_error =
                cfg.max_triggerbox_measurement_error.unwrap_or_else(|| {
                    flydra_types::TriggerboxConfig::default()
                        .max_triggerbox_measurement_error
                        .unwrap()
                });

            // queue several commands for the triggerbox on initial start.
            tx.send(Cmd::StopPulsesAndReset).await?;
            info!(
                "Triggerbox at {} request {} fps, actual frame rate will be {} fps. Will \
                accept maximum timestamp error of {} microseconds.",
                device_fname,
                fps,
                rate_actual,
                max_triggerbox_measurement_error.as_micros(),
            );
            tx.send(rate_cmd).await?;
            tx.send(Cmd::StartPulses).await?;

            {
                let mut expected_framerate = expected_framerate_arc.write().unwrap();
                *expected_framerate = Some(rate_actual as f32);
            }

            // Emperically, an Arduino Nano requires 7 seconds to wake up.
            let sleep_dur = std::time::Duration::from_secs_f32(7.0);

            let triggerbox = braid_triggerbox::TriggerboxDevice::new(
                on_new_clock_model,
                device_fname,
                cmd_rx,
                Some(triggerbox_data_tx),
                None,
                max_triggerbox_measurement_error,
                sleep_dur,
            )
            .await
            .map_err(|e| eyre::eyre!("on TriggerboxDevice::new: {e} {e:?}"))?;
            let query_dt2 = *query_dt;
            debug!("starting triggerbox task {}:{}", file!(), line!());
            let fut = async move {
                let result = triggerbox.run_forever(query_dt2).await;
                debug!("triggerbox task done {}:{}", file!(), line!());
                if let Err(e) = result {
                    error!("triggerbox result: {:?}", e);
                }
            };
            let _join_handle = tokio::spawn(fut);
        }
        TriggerType::FakeSync(FakeSyncConfig { framerate }) => {
            info!("No triggerbox configuration. Using fake synchronization.");

            signal_triggerbox_connected.store(true, Ordering::SeqCst);

            let mut expected_framerate = expected_framerate_arc.write().unwrap();
            *expected_framerate = Some(*framerate as f32);

            let gain = 1.0 / framerate;

            let now: chrono::DateTime<chrono::Utc> = chrono::Utc::now();
            let offset = datetime_conversion::datetime_to_f64(&now);

            (on_new_clock_model)(Some(braid_triggerbox::ClockModel {
                gain,
                n_measurements: 0,
                offset,
                residuals: 0.0,
            }));
        }
        TriggerType::PtpSync(ptpcfg) => {
            signal_triggerbox_connected.store(true, Ordering::SeqCst);

            if let Some(periodic_signal_period_usec) = ptpcfg.periodic_signal_period_usec {
                let framerate = 1e6 / periodic_signal_period_usec;
                let mut expected_framerate = expected_framerate_arc.write().unwrap();
                *expected_framerate = Some(framerate as f32);
            }
        }
        TriggerType::DeviceTimestamp => {
            signal_triggerbox_connected.store(true, Ordering::SeqCst);
        }
    };

    let expected_framerate_arc9 = expected_framerate_arc.clone();

    let live_stats_collector = LiveStatsCollector::new(tracker.clone());
    let tracker2 = tracker.clone();

    // decode UDP frames
    let raw_cam_data_stream =
        tokio_util::udp::UdpFramed::new(camdata_socket, CborPacketCodec::default());

    // Initiate camera synchronization on startup
    let sync_pulse_pause_started_arc2 = sync_pulse_pause_started_arc.clone();
    let time_model_arc2 = time_model_arc.clone();
    let cam_manager2 = cam_manager.clone();
    let valve2 = valve.clone();
    let triggerbox_cmd2 = triggerbox_cmd.clone();
    let fake_sync = matches!(trigger_cfg, TriggerType::FakeSync(_));
    let _sync_start_jh = tokio::spawn(async move {
        let interval_stream = tokio_stream::wrappers::IntervalStream::new(tokio::time::interval(
            std::time::Duration::from_secs(1),
        ));
        let mut interval_stream = valve2.wrap(interval_stream);

        while let Some(_now) = interval_stream.next().await {
            let have_triggerbox = signal_triggerbox_connected.load(Ordering::SeqCst);
            let have_all_cameras = signal_all_cams_present.load(Ordering::SeqCst);

            if have_triggerbox && have_all_cameras {
                info!("have triggerbox and all cameras. Synchronizing cameras.");
                synchronize_cameras(
                    triggerbox_cmd2,
                    fake_sync,
                    sync_pulse_pause_started_arc2.clone(),
                    cam_manager2.clone(),
                    time_model_arc2.clone(),
                )
                .await
                .unwrap();
                break;
            }
        }
    });

    // Signal cameras are synchronized

    let valve2 = valve.clone();
    let _sync_done_jh = tokio::spawn(async move {
        let interval_stream = tokio_stream::wrappers::IntervalStream::new(tokio::time::interval(
            std::time::Duration::from_secs(1),
        ));
        let mut interval_stream = valve2.wrap(interval_stream);
        while let Some(_now) = interval_stream.next().await {
            let sync_done = signal_all_cams_synced.load(Ordering::SeqCst);
            if sync_done {
                info!("All cameras done synchronizing.");

                // Send message to listeners.
                let mut tracker = shared_store.write().unwrap();
                tracker.modify(|shared| shared.all_expected_cameras_are_synced = true);
                break;
            }
        }
    });

    let strand_cam_http_session_handler2 = strand_cam_http_session_handler.clone();
    let cam_manager2 = cam_manager.clone();
    let live_stats_collector2 = live_stats_collector.clone();

    let packet_filter = move |r| {
        let live_stats_collector2 = live_stats_collector2.clone();
        let trigger_cfg = trigger_cfg.clone();
        let strand_cam_http_session_handler2 = strand_cam_http_session_handler2.clone();
        let cam_manager2 = cam_manager2.clone();
        let sync_pulse_pause_started_arc = sync_pulse_pause_started_arc.clone();
        let cam_manager = cam_manager.clone();
        // This creates a debug logger when `packet_capture_dump_fname` is not
        // `None`.
        let mut raw_packet_logger =
            RawPacketLogger::new(mainbrain_config.packet_capture_dump_fname.as_deref()).unwrap();
        let time_model_arc = time_model_arc.clone();
        async move {
            // vvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvv
            // Start of closure for on each incoming packet.

            // We run this closure for each incoming packet.

            // Let's be sure about the type of our input.
            let r: std::result::Result<
                (flydra_types::FlydraRawUdpPacket, std::net::SocketAddr),
                std::io::Error,
            > = r;

            let (packet, _addr) = match r {
                Ok(r) => r,
                Err(e) => {
                    error!("{}", e);
                    return Some(StreamItem::EOF);
                }
            };

            let raw_cam_name = RawCamName::new(packet.cam_name.clone());
            live_stats_collector2.register_new_frame_data(&raw_cam_name, packet.points.len());

            // Create closure which is called only if there is a new frame offset
            // (which occurs upon synchronization).
            let send_new_frame_offset = |frame| {
                let strand_cam_http_session_handler = strand_cam_http_session_handler2.clone();
                let cam_name = raw_cam_name.clone();
                let fut_no_err = async move {
                    match strand_cam_http_session_handler
                        .send_frame_offset(&cam_name, frame)
                        .await
                    {
                        Ok(_) => {}
                        Err(e) => {
                            error!("Error sending frame offset: {}", e);
                        }
                    };
                };
                tokio::spawn(fut_no_err);
            };

            let synced_frame = cam_manager2.got_new_frame_live(
                &packet,
                &sync_pulse_pause_started_arc,
                send_new_frame_offset,
                &trigger_cfg,
            );

            let cam_num = cam_manager.cam_num(&raw_cam_name);

            raw_packet_logger
                .log_raw_packets(&packet, cam_num, synced_frame)
                .unwrap();

            let cam_num = match cam_num {
                Some(cam_num) => cam_num,
                None => {
                    let known_raw_cam_names = cam_manager.all_raw_cam_names();
                    let cam_names = known_raw_cam_names
                        .iter()
                        .map(|x| format!("\"{}\"", x.as_str()))
                        .collect::<Vec<_>>()
                        .join(", ");
                    debug!(
                        "Unknown camera name \"{}\" ({} expected cameras: [{}]).",
                        raw_cam_name.as_str(),
                        known_raw_cam_names.len(),
                        cam_names
                    );
                    // Cannot compute cam_num, drop this data.
                    return None;
                }
            };

            let (synced_frame, trigger_timestamp) = match synced_frame {
                Some(synced_frame) => {
                    let trigger_timestamp = match &trigger_cfg {
                        TriggerType::TriggerboxV1(_) | TriggerType::FakeSync(_) => {
                            let time_model = time_model_arc.read().unwrap();
                            compute_trigger_timestamp(&time_model, synced_frame)
                        }
                        TriggerType::PtpSync(_) => {
                            // In case where we trust camera sync data, use
                            // timestamp from camera. All packets from all
                            // cameras should have this same timestamp, so it
                            // shouldn't matter which camera we use.
                            packet.device_timestamp.map(|device_timestamp| {
                                let ptp_stamp = flydra_types::PtpStamp::new(device_timestamp.get());
                                let device_timestamp_chrono =
                                    chrono::DateTime::<chrono::Utc>::try_from(ptp_stamp.clone())
                                        .unwrap();
                                device_timestamp_chrono.into()
                            })
                        }
                        TriggerType::DeviceTimestamp => {
                            todo!();
                        }
                    };
                    (synced_frame, trigger_timestamp)
                }
                None => {
                    // cannot compute synced_frame number, drop this data
                    return None;
                }
            };

            let frame_data = flydra2::FrameData::new(
                raw_cam_name,
                cam_num,
                synced_frame,
                trigger_timestamp,
                packet.cam_received_time,
                packet.device_timestamp,
                packet.block_id,
            );

            assert!(packet.points.len() < u8::MAX as usize);
            let points = packet
                .points
                .into_iter()
                .enumerate()
                .map(|(idx, pt)| {
                    assert!(idx <= 255);
                    flydra2::NumberedRawUdpPoint { idx: idx as u8, pt }
                })
                .collect();

            let fdp = FrameDataAndPoints { frame_data, points };
            Some(StreamItem::Packet(fdp))
            // This is the end of closure for each incoming packet.
            // ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        }
    };

    let flydra2_stream = raw_cam_data_stream.filter_map(packet_filter);

    let (data_tx, data_rx) = tokio::sync::mpsc::channel(50);

    let model_pose_server_addr = mainbrain_config.model_server_addr;
    tokio::spawn(flydra2::new_model_server(data_rx, model_pose_server_addr));

    {
        let mut tracker = tracker2.write().unwrap();
        tracker.modify(|shared| shared.model_server_addr = Some(model_pose_server_addr))
    }

    let expected_framerate: Option<f32> = *expected_framerate_arc9.read().unwrap();
    info!("expected_framerate: {:?}", expected_framerate);

    coord_processor.add_listener(data_tx);
    let coord_proc_fut = coord_processor.consume_stream(flydra2_stream, expected_framerate);

    // We "block" (in an async way) here for the entire runtime of the program.
    // The first one of these to exit will end all of them. This should be
    // `coord_proc_fut`.
    tokio::select! {
        _ = event_broadcast_fut => {
            info!("Event broadcaster finished.");
        },
        _ = http_serve_future => {
            info!("HTTP Server finished.");
        },
        _ = strand_cam_set.join_next() => {
            info!("Strand Camera future set finished.");
        },
        res_writer_jh = coord_proc_fut => {
            info!("Coordinate processor finished.");
            // Allow writer task time to finish writing.
            debug!("Runtime ending. Joining coord_processor.consume_stream future.");
            res_writer_jh?.await??;
        },
    };

    debug!("braid-run finishing.");

    Ok(())
}

#[derive(Clone)]
struct LiveStatsCollector {
    shared: SharedStore,
    collected: Arc<RwLock<BTreeMap<RawCamName, LiveStatsAccum>>>,
}

#[derive(Debug)]
struct LiveStatsAccum {
    start: std::time::Instant,
    n_frames: usize,
    n_points: usize,
}

impl LiveStatsAccum {
    fn new() -> Self {
        Self {
            start: std::time::Instant::now(),
            n_frames: 0,
            n_points: 0,
        }
    }
    fn update(&mut self, n_points: usize) {
        self.n_frames += 1;
        self.n_points += n_points;
    }
    fn get_results_and_reset(&mut self) -> flydra_types::RecentStats {
        let recent = flydra_types::RecentStats {
            total_frames_collected: 0,
            frames_collected: self.n_frames,
            points_detected: self.n_points,
        };
        self.start = std::time::Instant::now();
        self.n_frames = 0;
        self.n_points = 0;
        recent
    }
}

impl LiveStatsCollector {
    fn new(shared: SharedStore) -> Self {
        let collected = Arc::new(RwLock::new(BTreeMap::new()));
        Self { shared, collected }
    }

    fn register_new_frame_data(&self, name: &RawCamName, n_points: usize) {
        let to_send = {
            // scope for lock on self.collected
            let mut collected = self.collected.write().unwrap();
            let entry = collected
                .entry(name.clone())
                .or_insert_with(LiveStatsAccum::new);
            entry.update(n_points);

            if entry.start.elapsed() > std::time::Duration::from_secs(1) {
                Some((name.clone(), entry.get_results_and_reset()))
            } else {
                None
            }
        };
        if let Some((name, recent_stats)) = to_send {
            // scope for shared scope
            let mut tracker = self.shared.write().unwrap();
            tracker.modify(|shared| {
                for cc in shared.connected_cameras.iter_mut() {
                    if cc.name == name {
                        let old_total = cc.recent_stats.total_frames_collected;
                        cc.recent_stats = recent_stats.clone();
                        cc.recent_stats.total_frames_collected =
                            old_total + recent_stats.frames_collected;
                        break;
                    }
                }
            });
        }
    }
}

pub(crate) async fn toggle_saving_csv_tables(
    start_saving: bool,
    expected_framerate_arc: Arc<RwLock<Option<f32>>>,
    output_base_dirname: std::path::PathBuf,
    braidz_write_tx_weak: tokio::sync::mpsc::WeakSender<flydra2::SaveToDiskMsg>,
    per_cam_data_arc: Arc<RwLock<BTreeMap<RawCamName, PerCamSaveData>>>,
    shared_data: SharedStore,
) {
    if start_saving {
        let expected_framerate: Option<f32> = *expected_framerate_arc.read().unwrap();
        let local: chrono::DateTime<chrono::Local> = chrono::Local::now();
        let dirname = local.format("%Y%m%d_%H%M%S.braid").to_string();
        let mut my_dir = output_base_dirname.clone();
        my_dir.push(dirname);
        let per_cam_data = {
            // small scope for read lock
            let per_cam_data_ref = per_cam_data_arc.read().unwrap();
            (*per_cam_data_ref).clone()
        };
        let cfg = flydra2::StartSavingCsvConfig {
            out_dir: my_dir.clone(),
            local: Some(local),
            git_rev: env!("GIT_HASH").to_string(),
            fps: expected_framerate,
            per_cam_data,
            print_stats: false,
            save_performance_histograms: true,
        };

        if let Some(braidz_write_tx) = braidz_write_tx_weak.upgrade() {
            // `braidz_write_tx` will be dropped after this scope.
            braidz_write_tx
                .send(flydra2::SaveToDiskMsg::StartSavingCsv(cfg))
                .await
                .unwrap();
            info!("saving data to \"{}\"", my_dir.display());
        } else {
            error!("data writing thread lost. Not saving data as requested");
        }

        {
            let mut tracker = shared_data.write().unwrap();
            tracker.modify(|store| {
                store.csv_tables_dirname = Some(RecordingPath::new(my_dir.display().to_string()));
            });
        }
    } else {
        if let Some(braidz_write_tx) = braidz_write_tx_weak.upgrade() {
            // `braidz_write_tx` will be dropped after this scope.
            braidz_write_tx
                .send(flydra2::SaveToDiskMsg::StopSavingCsv)
                .await
                .unwrap_or(()); // ignore error on shutdown
            info!("stopping saving");
        } else {
            error!("data writing thread lost. Could not stop saving data as requested");
        }

        {
            let mut tracker = shared_data.write().unwrap();
            tracker.modify(|store| {
                store.csv_tables_dirname = None;
            });
        }
    }
}

async fn synchronize_cameras(
    triggerbox_cmd: Option<tokio::sync::mpsc::Sender<braid_triggerbox::Cmd>>,
    fake_sync: bool,
    sync_pulse_pause_started_arc: Arc<RwLock<Option<std::time::Instant>>>,
    mut cam_manager: flydra2::ConnectedCamerasManager,
    time_model_arc: Arc<RwLock<Option<rust_cam_bui_types::ClockModel>>>,
) -> Result<()> {
    info!("preparing to synchronize cameras");

    // This time must be prior to actually resetting sync data.
    {
        let mut sync_pulse_pause_started = sync_pulse_pause_started_arc.write().unwrap();
        *sync_pulse_pause_started = Some(std::time::Instant::now());
    }

    // Now we can reset the sync data.
    cam_manager.reset_sync_data();

    {
        let mut guard = time_model_arc.write().unwrap();
        *guard = None;
    }

    if let Some(tx) = triggerbox_cmd {
        begin_cam_sync_triggerbox_in_process(tx).await?;
    }

    if fake_sync {
        info!("Using fake synchronization method.");
    }
    Ok(())
}

async fn begin_cam_sync_triggerbox_in_process(
    tx: tokio::sync::mpsc::Sender<braid_triggerbox::Cmd>,
) -> Result<()> {
    // This is the case when the triggerbox is within this process.
    info!("preparing for triggerbox to temporarily stop sending pulses");

    info!("requesting triggerbox to stop sending pulses");
    use braid_triggerbox::Cmd::*;
    tx.send(StopPulsesAndReset).await?;
    tokio::time::sleep(std::time::Duration::from_secs(TRIGGERBOX_SYNC_SECONDS)).await;
    tx.send(StartPulses).await?;
    info!("requesting triggerbox to start sending pulses again");
    Ok(())
}

fn to_event_frame(state: &BraidHttpApiSharedState) -> String {
    let buf = serde_json::to_string(&state).unwrap();
    let frame_string = format!("event: {BRAID_EVENT_NAME}\ndata: {buf}\n\n");
    frame_string
}
