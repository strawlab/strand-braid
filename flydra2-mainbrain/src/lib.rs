#[macro_use]
extern crate log;

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;

use parking_lot::RwLock;

use futures::stream::StreamExt;
use tokio::net::UdpSocket;
use tokio_util::udp::UdpFramed;

use structopt::StructOpt;

use async_change_tracker::ChangeTracker;
use bui_backend::highlevel::{create_bui_app_inner, BuiAppInner};
use bui_backend::AccessControl;
use bui_backend_types::CallbackDataAndSession;

use flydra2::{CoordProcessor, FrameDataAndPoints, MyFloat, StreamItem};
use flydra_types::{
    BuiServerInfo, CamInfo, CborPacketCodec, FlydraFloatTimestampLocal, FlydraPacketCodec,
    FlydraRawUdpPacket, HttpApiCallback, HttpApiShared, RosCamName, SyncFno, TriggerType,
    Triggerbox,
};
use rust_cam_bui_types::ClockModel;
use rust_cam_bui_types::RecordingPath;

mod multicam_http_session_handler;
pub use crate::multicam_http_session_handler::HttpSessionHandler;
use crossbeam_ok::CrossbeamOk;

lazy_static::lazy_static! {
    static ref EVENTS_PREFIX: String = format!("/{}", flydra_types::BRAID_EVENTS_URL_PATH);
}

// Include the files to be served and define `fn get_default_config()`.
include!(concat!(env!("OUT_DIR"), "/mainbrain_frontend.rs")); // Despite slash, this works on Windows.

use anyhow::Result;

const SYNCHRONIZE_DURATION_SEC: u8 = 3;

#[derive(thiserror::Error, Debug)]
enum MainbrainError {
    #[error("The --jwt-secret argument must be passed or the JWT_SECRET environment variable must be set.")]
    JwtError,
}

/// The structure that holds our app data
struct HttpApiApp {
    inner: BuiAppInner<HttpApiShared, HttpApiCallback>,
    time_model_arc: Arc<RwLock<Option<rust_cam_bui_types::ClockModel>>>,
    triggerbox_cmd: Option<channellib::Sender<flydra1_triggerbox::Cmd>>,
    sync_pulse_pause_started_arc: Arc<RwLock<Option<std::time::Instant>>>,
    expected_framerate_arc: Arc<RwLock<Option<f32>>>,
    write_controller_arc: Arc<RwLock<flydra2::CoordProcessorControl>>,
}

impl HttpApiApp {
    /// Create our app
    fn new(
        shutdown_rx: tokio::sync::oneshot::Receiver<()>,
        auth: AccessControl,
        cam_manager: flydra2::ConnectedCamerasManager,
        shared: HttpApiShared,
        config: Config,
        time_model_arc: Arc<RwLock<Option<rust_cam_bui_types::ClockModel>>>,
        triggerbox_cmd: Option<channellib::Sender<flydra1_triggerbox::Cmd>>,
        sync_pulse_pause_started_arc: Arc<RwLock<Option<std::time::Instant>>>,
        expected_framerate_arc: Arc<RwLock<Option<f32>>>,
        output_base_dirname: std::path::PathBuf,
        write_controller_arc: Arc<RwLock<flydra2::CoordProcessorControl>>,
        current_images_arc: Arc<RwLock<flydra2::ImageDictType>>,
    ) -> Result<Self> {
        // Create our shared state.
        let shared_store = Arc::new(RwLock::new(ChangeTracker::new(shared)));

        // Create `inner`, which takes care of the browser communication details for us.
        let chan_size = 10;
        let (_, mut inner) = create_bui_app_inner(
            Some(shutdown_rx),
            &auth,
            shared_store,
            config,
            chan_size,
            &*EVENTS_PREFIX,
            Some(flydra_types::BRAID_EVENT_NAME.to_string()),
        )?;

        let mainbrain_server_info = {
            let local_addr = inner.local_addr().clone();
            let token = inner.token();
            BuiServerInfo::new(local_addr, token)
        };

        debug!(
            "initialized HttpApiApp listening at {}",
            mainbrain_server_info.guess_base_url_with_token()
        );

        let cam_manager2 = cam_manager.clone();
        let triggerbox_cmd2 = triggerbox_cmd.clone();
        let time_model_arc2 = time_model_arc.clone();

        let expected_framerate_arc2 = expected_framerate_arc.clone();
        let output_base_dirname2 = output_base_dirname.clone();
        let write_controller_arc2 = write_controller_arc.clone();
        let current_images_arc2 = current_images_arc.clone();
        let shared_data = inner.shared_arc().clone();

        let sync_pulse_pause_started_arc2 = sync_pulse_pause_started_arc.clone();
        // Create a Stream to handle callbacks from clients.
        inner.set_callback_listener(Box::new(
            move |msg: CallbackDataAndSession<HttpApiCallback>| {
                // This closure is the callback handler called whenever the
                // client sends us something.

                use crate::HttpApiCallback::*;
                match msg.payload {
                    NewCamera(cam_info) => {
                        debug!("got NewCamera {:?}", cam_info);
                        let mut cam_manager3 = cam_manager2.clone();
                        cam_manager3.register_new_camera(
                            &cam_info.orig_cam_name,
                            &cam_info.http_camserver_info,
                            &cam_info.ros_cam_name,
                        );
                    }
                    UpdateCurrentImage(image_info) => {
                        // new image from camera
                        // (This replaces old FromRosThread::DoSendImage)
                        debug!("got new image for camera {:?}", image_info.ros_cam_name);
                        let mut current_images = current_images_arc2.write();
                        let fname = format!("{}.png", image_info.ros_cam_name);
                        current_images.insert(fname, image_info.current_image_png);
                    }
                    DoSyncCameras => {
                        debug!("got DoSyncCameras");

                        let sync_pulse_pause_started_arc3 = sync_pulse_pause_started_arc2.clone();
                        #[allow(unused_mut)]
                        let mut cam_manager3 = cam_manager2.clone();
                        let time_model_arc3 = time_model_arc2.clone();
                        let triggerbox_cmd3 = triggerbox_cmd2.clone();

                        std::thread::spawn(move || {
                            debug!("spawned thread to wait for sync");
                            synchronize_cameras(
                                triggerbox_cmd3.clone(),
                                sync_pulse_pause_started_arc3,
                                cam_manager3.clone(),
                                time_model_arc3,
                            );
                        });
                    }
                    DoRecordCsvTables(value) => {
                        debug!("got DoRecordCsvTables({})", value);
                        toggle_saving_csv_tables(
                            value,
                            expected_framerate_arc2.clone(),
                            output_base_dirname2.clone(),
                            write_controller_arc2.clone(),
                            current_images_arc2.clone(),
                            shared_data.clone(),
                        );
                    }
                    SetExperimentUuid(value) => {
                        debug!("got SetExperimentUuid({})", value);
                        let write_controller = write_controller_arc2.write();
                        write_controller.set_experiment_uuid(value);
                    }
                }
                futures::future::ok(())
            },
        ));

        // Return our app.
        Ok(HttpApiApp {
            inner,
            time_model_arc,
            triggerbox_cmd,
            sync_pulse_pause_started_arc,
            expected_framerate_arc,
            write_controller_arc,
        })
    }
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

/// Convert the address we are listening on to a string.
///
/// We can strings over the network, but not binary representations of
/// socket addresses.
fn addr_to_buf(local_addr: &std::net::SocketAddr) -> Result<String> {
    let addr_ip = flydra_types::AddrInfoIP::from_socket_addr(local_addr);
    Ok(serde_json::to_string(&addr_ip)?)
}

struct SendConnectedCamToBuiBackend {
    shared_store: Arc<RwLock<ChangeTracker<HttpApiShared>>>,
}

impl flydra2::ConnectedCamCallback for SendConnectedCamToBuiBackend {
    fn on_cam_changed(&self, new_cam_list: Vec<CamInfo>) {
        let mut tracker = self.shared_store.write();
        tracker.modify(|shared| shared.connected_cameras = new_cam_list.clone());
    }
}

fn display_qr_url(url: &str) {
    use qrcodegen::{QrCode, QrCodeEcc};
    use std::io::{stdout, Write};

    let qr = QrCode::encode_text(&url, QrCodeEcc::Low).unwrap();

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

pub struct StartupPhase1 {
    pub camdata_socket: tokio::net::UdpSocket,
    my_app: HttpApiApp,
    pub mainbrain_server_info: BuiServerInfo,
    cam_manager: flydra2::ConnectedCamerasManager,
    http_session_handler: HttpSessionHandler,
    handle: tokio::runtime::Handle,
    valve: stream_cancel::Valve,
    trigger_cfg: TriggerType,
    triggerbox_rx: Option<channellib::Receiver<flydra1_triggerbox::Cmd>>,
    flydra1: bool,
    model_pose_server_addr: std::net::SocketAddr,
    coord_processor: CoordProcessor,
    model_server_shutdown_rx: tokio::sync::oneshot::Receiver<()>,
}

pub async fn pre_run(
    handle: &tokio::runtime::Handle,
    cal_fname: Option<std::path::PathBuf>,
    output_base_dirname: std::path::PathBuf,
    opt_tracking_params: Option<flydra2::SwitchingTrackingParams>,
    show_tracking_params: bool,
    // sched_policy_priority: Option<(libc::c_int, libc::c_int)>,
    camdata_addr: &str,
    trigger_cfg: TriggerType,
    flydra1: bool,
    http_api_server_addr: String,
    http_api_server_token: Option<String>,
    model_pose_server_addr: std::net::SocketAddr,
    save_empty_data2d: bool,
    jwt_secret: Option<Vec<u8>>,
) -> Result<StartupPhase1> {
    info!("saving to directory: {}", output_base_dirname.display());

    let (quit_trigger, valve) = stream_cancel::Valve::new();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let (model_server_shutdown_tx, model_server_shutdown_rx) =
        tokio::sync::oneshot::channel::<()>();
    let (shtdwn_q_tx, mut shtdwn_q_rx) = tokio::sync::mpsc::channel::<()>(5);

    ctrlc::set_handler(move || {
        // This closure can get called multiple times, but quit_trigger
        // and shutdown_tx cannot be copied or cloned and thus can only
        // but fired once. So in this signal handler we fire a message
        // on a queue and then on the receive side only deal with the first
        // send.
        info!("got Ctrl-C, shutting down");

        let mut shtdwn_q_tx2 = shtdwn_q_tx.clone();

        // Send quit message.
        match futures::executor::block_on(shtdwn_q_tx2.send(())) {
            Ok(()) => {}
            Err(e) => {
                error!("failed sending quit command: {}", e);
            }
        }
    })
    .expect("Error setting Ctrl-C handler");

    let recon = if let Some(ref cal_fname) = cal_fname {
        info!("using calibration: {}", cal_fname.display());

        // read the calibration
        let cal_file = anyhow::Context::with_context(std::fs::File::open(&cal_fname), || {
            format!("loading calibration {}", cal_fname.display())
        })?;
        Some(flydra_mvg::FlydraMultiCameraSystem::<MyFloat>::from_flydra_xml(cal_file)?)
    } else {
        None
    };

    let cam_manager = flydra2::ConnectedCamerasManager::new(&recon);
    let http_session_handler = HttpSessionHandler::new(cam_manager.clone());

    let (save_data_tx, save_data_rx) = channellib::unbounded();

    let tracking_params = opt_tracking_params.unwrap_or_else(|| {
        info!("no tracking parameters file given, using default tracking parameters");
        flydra2::SwitchingTrackingParams::default()
    });

    if show_tracking_params {
        let t2: flydra_types::TrackingParams = tracking_params.into();
        let buf = toml::to_string(&t2)?;
        println!("{}", buf);
        std::process::exit(0);
    }

    let ignore_latency = false;
    let coord_processor = CoordProcessor::new(
        cam_manager.clone(),
        recon.clone(),
        tracking_params,
        save_data_tx,
        save_data_rx,
        save_empty_data2d,
        ignore_latency,
    )?;
    let write_controller = coord_processor.get_write_controller();
    let write_controller_arc = Arc::new(RwLock::new(write_controller.clone())); // TODO do not use Arc<RwLock<_>>

    // Here is what we do on quit:
    // 1) Stop saving data, convert .braid dir to .braidz, close files.
    // 2) Fire a DoQuit message to all cameras and wait for them to quit.
    // 3) Only then close all our network ports and streams nicely.
    let mut quit_trigger_container = Some(quit_trigger);
    let mut http_session_handler2 = http_session_handler.clone();
    let write_controller_arc2 = write_controller_arc.clone();
    handle.spawn(async move {
        while let Some(()) = shtdwn_q_rx.next().await {
            debug!("got shutdown command {}:{}", file!(), line!());

            {
                // Stop saving Braid data.
                // Do not need to wait for completion because we are going to
                // exit nicely by manually ending all threads and letting all
                // drop handlers run œ(without aborting) and thus the program
                // will finish writing without an explicit wait. (Of course,
                // this fails during an actual abort).
                let write_controller = write_controller_arc2.write();
                write_controller.stop_saving_data();
            }

            http_session_handler2
                .send_quit_all()
                .await
                .expect("send_quit_all");

            // When we get here, we have successfully sent DoQuit to all cams.
            // We can now quit everything in the mainbrain.
            if let Some(quit_trigger) = quit_trigger_container.take() {
                quit_trigger.cancel();
                break; // no point to listen for more
            }
        }
        shutdown_tx.send(()).expect("sending quit to hyper");
        model_server_shutdown_tx
            .send(())
            .expect("sending quit to model server");
    });

    // This `get_default_config()` function is created by bui_backend_codegen
    // and is pulled in here by the `include!` macro above.
    let config = get_default_config();

    let time_model_arc = Arc::new(RwLock::new(None));

    let (triggerbox_cmd, triggerbox_rx, fake_sync) = match &trigger_cfg {
        TriggerType::TriggerboxV1(_) => {
            let (tx, rx) = channellib::unbounded();
            (Some(tx), Some(rx), false)
        }
        TriggerType::FakeSync(_) => (None, None, true),
    };

    let sync_pulse_pause_started: Option<std::time::Instant> = None;
    let sync_pulse_pause_started_arc = Arc::new(RwLock::new(sync_pulse_pause_started));

    let flydra_app_name = "Braid".to_string();

    let shared = HttpApiShared {
        fake_sync,
        csv_tables_dirname: None,
        clock_model_copy: None,
        calibration_filename: cal_fname.map(|x| x.into_os_string().into_string().unwrap()),
        connected_cameras: Vec::new(),
        model_server_addr: None,
        flydra_app_name,
    };

    let expected_framerate_arc = Arc::new(RwLock::new(None));

    let current_images_arc = Arc::new(RwLock::new(flydra2::ImageDictType::new()));

    use std::net::ToSocketAddrs;
    let http_api_server_addr = http_api_server_addr.to_socket_addrs()?.next().unwrap();

    let auth = if let Some(ref secret) = jwt_secret {
        if let Some(token) = http_api_server_token {
            bui_backend::highlevel::generate_auth_with_token(
                http_api_server_addr,
                secret.to_vec(),
                token,
            )?
        } else {
            bui_backend::highlevel::generate_random_auth(http_api_server_addr, secret.to_vec())?
        }
    } else {
        if http_api_server_addr.ip().is_loopback() {
            AccessControl::Insecure(http_api_server_addr)
        } else {
            return Err(MainbrainError::JwtError.into());
        }
    };

    let my_app = HttpApiApp::new(
        shutdown_rx,
        auth,
        cam_manager.clone(),
        shared,
        config,
        time_model_arc,
        triggerbox_cmd,
        sync_pulse_pause_started_arc,
        expected_framerate_arc,
        output_base_dirname.clone(),
        write_controller_arc.clone(),
        current_images_arc.clone(),
    )?;

    let is_loopback = my_app.inner.local_addr().ip().is_loopback();
    let mainbrain_server_info =
        flydra_types::BuiServerInfo::new(my_app.inner.local_addr().clone(), my_app.inner.token());
    let url = mainbrain_server_info.guess_base_url_with_token();
    println!(
        "Depending on things, you may be able to login with this url: {}",
        url
    );
    if !is_loopback {
        println!("This same URL as a QR code:");
        display_qr_url(&url);
    }

    let camdata_addr = camdata_addr.parse::<SocketAddr>().unwrap();

    let camdata_addr_buf = addr_to_buf(&camdata_addr)?;
    debug!("flydra mainbrain camera listener at: {}", camdata_addr_buf);

    let camdata_socket_fut = UdpSocket::bind(&camdata_addr);
    let camdata_socket = camdata_socket_fut.await?;

    Ok(StartupPhase1 {
        camdata_socket,
        my_app,
        mainbrain_server_info,
        cam_manager,
        http_session_handler,
        handle: handle.clone(),
        trigger_cfg,
        triggerbox_rx,
        flydra1,
        model_pose_server_addr,
        coord_processor,
        valve,
        model_server_shutdown_rx,
    })
}

pub async fn run(phase1: StartupPhase1) -> Result<()> {
    let camdata_socket = phase1.camdata_socket;
    let my_app = phase1.my_app;

    let mainbrain_server_info = phase1.mainbrain_server_info;
    let mut cam_manager = phase1.cam_manager;
    let http_session_handler = phase1.http_session_handler;
    let handle = phase1.handle;
    let rt_handle = handle.clone();
    let rt_handle2 = rt_handle.clone();
    let trigger_cfg = phase1.trigger_cfg;
    let triggerbox_rx = phase1.triggerbox_rx;
    let flydra1 = phase1.flydra1;
    let model_pose_server_addr = phase1.model_pose_server_addr;
    let mut coord_processor = phase1.coord_processor;
    let valve = phase1.valve;
    let model_server_shutdown_rx = phase1.model_server_shutdown_rx;

    info!(
        "http api server at {}",
        mainbrain_server_info.guess_base_url_with_token()
    );

    let time_model_arc = my_app.time_model_arc.clone();
    let expected_framerate_arc = my_app.expected_framerate_arc.clone();
    let sync_pulse_pause_started_arc = my_app.sync_pulse_pause_started_arc.clone();

    let write_controller_arc = my_app.write_controller_arc.clone();

    {
        let sender = SendConnectedCamToBuiBackend {
            shared_store: my_app.inner.shared_arc().clone(),
        };
        let old_callback = cam_manager.set_cam_changed_callback(Box::new(sender));
        assert!(old_callback.is_none());
    }

    let info = flydra_types::StaticMainbrainInfo {
        name: env!("CARGO_PKG_NAME").into(),
        version: env!("CARGO_PKG_VERSION").into(),
    };

    let (triggerbox_data_tx, triggerbox_data_rx) = channellib::unbounded();

    let write_controller_arc2 = write_controller_arc.clone();
    let triggerbox_data_thread_builder =
        std::thread::Builder::new().name("triggerbox_data_thread".to_string());
    let _triggerbox_data_thread_handle = Some(triggerbox_data_thread_builder.spawn(move || {
        loop {
            match triggerbox_data_rx.recv() {
                Ok(msg) => {
                    let write_controller = write_controller_arc2.write();
                    write_controller.append_trigger_clock_info_message(msg);
                }
                Err(e) => {
                    let _: channellib::RecvError = e;
                    break;
                }
            };
        }
        error!("done listening for trigger clock data: sender hung up.");
    })?);

    let mut _triggerbox_thread_control = None;

    let tracker = my_app.inner.shared_arc().clone();

    let on_new_clock_model = {
        let time_model_arc = time_model_arc.clone();
        let http_session_handler = http_session_handler.clone();
        let tracker = tracker.clone();
        Box::new(move |tm: Option<ClockModel>| {
            let cm = tm.clone();
            {
                let mut guard = time_model_arc.write();
                *guard = tm;
            }
            {
                let mut tracker_guard = tracker.write();
                tracker_guard.modify(|shared| shared.clock_model_copy = cm.clone());
            }
            let mut http_session_handler3 = http_session_handler.clone();
            handle.spawn(async move {
                let r = http_session_handler3.send_clock_model_to_all(cm).await;
                match r {
                    Ok(_http_response) => {}
                    Err(e) => {
                        error!("error sending clock model: {}", e);
                    }
                };
            });
        })
    };

    // if let Some(ref cfg) = trigger_cfg {
    match &trigger_cfg {
        TriggerType::TriggerboxV1(cfg) => {
            let dev = &cfg.device_fname;
            let fps = &cfg.framerate;
            let query_dt = &cfg.query_dt;

            use flydra1_triggerbox::{launch_background_thread, make_trig_fps_cmd, Cmd};

            let device = std::path::PathBuf::from(dev);
            let tx = my_app.triggerbox_cmd.clone().unwrap();
            let cmd_rx = triggerbox_rx.unwrap();

            tx.send(Cmd::StopPulsesAndReset).cb_ok();
            tx.send(make_trig_fps_cmd(*fps as f64)).cb_ok();
            tx.send(Cmd::StartPulses).cb_ok();

            let mut expected_framerate = expected_framerate_arc.write();
            *expected_framerate = Some(*fps);

            // triggerbox_cmd = Some(tx);

            let (control, _handle) = launch_background_thread(
                on_new_clock_model,
                device,
                cmd_rx,
                Some(triggerbox_data_tx),
                *query_dt,
            )?;

            _triggerbox_thread_control = Some(control);
        }
        TriggerType::FakeSync(cfg) => {
            info!("No triggerbox configuration. Using fake synchronization.");

            let mut expected_framerate = expected_framerate_arc.write();
            *expected_framerate = Some(cfg.fps as f32);

            let gain = 1.0 / cfg.fps as f64;

            let now: chrono::DateTime<chrono::Utc> = chrono::Utc::now();
            // let local = now.with_timezone(&chrono::Local);
            let offset = datetime_conversion::datetime_to_f64(&now);

            (on_new_clock_model)(Some(ClockModel {
                gain,
                n_measurements: 0,
                offset,
                residuals: 0.0,
            }));
        }
    };

    let expected_framerate_arc9 = expected_framerate_arc.clone();

    let live_stats_collector = LiveStatsCollector::new(tracker.clone());
    let tracker2 = tracker.clone();

    let raw_cam_data_stream: Box<
        dyn futures::stream::Stream<
                Item = std::result::Result<
                    (flydra_types::FlydraRawUdpPacket, std::net::SocketAddr),
                    std::io::Error,
                >,
            > + Send
            + Unpin,
    > = match flydra1 {
        true => {
            let codec = FlydraPacketCodec::default();
            // let (_sink, stream) = tokio::codec::Framed::new( camdata_socket, codec ).split();

            // let (_sink, stream) = UdpFramed::new( camdata_socket, codec ).split();
            let stream = UdpFramed::new(camdata_socket, codec);

            // let (_sink, stream) = UdpFramed::new(camdata_socket, FlydraPacketCodec::default()).split();
            // let stream = futures::compat::Compat01As03::new(stream);
            Box::new(stream)
        }
        false => {
            let codec = CborPacketCodec::default();
            // let (_sink, stream) = tokio::codec::Framed::new( camdata_socket, codec ).split();
            // let (_sink, stream) = UdpFramed::new( camdata_socket, codec ).split();
            let stream = UdpFramed::new(camdata_socket, codec);

            // let (_sink, stream) = UdpFramed::new(camdata_socket, CborPacketCodec::default()).split();
            // let stream = futures::compat::Compat01As03::new(stream);
            Box::new(stream)
        }
    };

    let http_session_handler2 = http_session_handler.clone();
    let cam_manager2 = cam_manager.clone();
    let live_stats_collector2 = live_stats_collector.clone();

    let flydra2_stream = futures::stream::StreamExt::filter_map(raw_cam_data_stream, move |r| {
        let r: std::result::Result<
            (flydra_types::FlydraRawUdpPacket, std::net::SocketAddr),
            std::io::Error,
        > = r;

        match r {
            Ok(_) => {}
            Err(e) => {
                error!("{}", e);
                return futures::future::ready(Some(StreamItem::EOF));
            }
        }

        let (packet, _addr): (FlydraRawUdpPacket, std::net::SocketAddr) = r.unwrap();

        let ros_cam_name = RosCamName::new(packet.cam_name.clone());
        live_stats_collector2.register_new_frame_data(&ros_cam_name, packet.points.len());

        let http_session_handler3 = http_session_handler2.clone();

        let sync_time_min = match &trigger_cfg {
            TriggerType::TriggerboxV1(_) => {
                // Using trigger box
                std::time::Duration::from_secs(SYNCHRONIZE_DURATION_SEC as u64)
            }
            TriggerType::FakeSync(_) => {
                // Using fake trigger
                std::time::Duration::from_secs(0)
            }
        };

        let synced_frame = match cam_manager2.got_new_frame_live(
            &packet,
            &sync_pulse_pause_started_arc,
            sync_time_min,
            std::time::Duration::from_secs(SYNCHRONIZE_DURATION_SEC as u64 + 2),
            |name, frame| {
                let name2 = name.clone();
                let mut http_session_handler4 = http_session_handler3.clone();
                let fut_no_err = async move {
                    match http_session_handler4.send_frame_offset(&name2, frame).await {
                        Ok(_http_response) => {}
                        Err(e) => {
                            error!("Error sending frame offset: {}", e);
                        }
                    };
                };
                rt_handle.spawn(fut_no_err); // TODO: spawn
            },
        ) {
            Some(v) => v,
            None => {
                return futures::future::ready(None);
            } // cannot compute synced_frame number, drop this data
        };

        let trigger_timestamp = {
            let time_model = time_model_arc.read();
            compute_trigger_timestamp(&time_model, synced_frame)
        };

        let cam_num = match cam_manager.cam_num(&ros_cam_name) {
            Some(cam_num) => cam_num,
            None => {
                error!("Unknown camera name '{}'.", ros_cam_name.as_str());
                panic!("unknown camera name");
            }
        };

        let frame_data = flydra2::FrameData::new(
            ros_cam_name,
            cam_num,
            synced_frame,
            trigger_timestamp,
            packet.cam_received_time,
        );

        assert!(packet.points.len() < u8::max_value() as usize);
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
        futures::future::ready(Some(StreamItem::Packet(fdp)))
    });

    let ms = flydra2::ModelServer::new(
        valve.clone(),
        Some(model_server_shutdown_rx),
        &model_pose_server_addr,
        info,
        rt_handle2,
    )?;

    {
        let mut tracker = tracker2.write();
        tracker.modify(|shared| shared.model_server_addr = Some(ms.local_addr().clone()))
    }

    let expected_framerate: Option<f32> = *expected_framerate_arc9.read();
    info!("expected_framerate: {:?}", expected_framerate);

    coord_processor.add_listener(Box::new(ms));
    let consume_future =
        coord_processor.consume_stream(valve.wrap(flydra2_stream), expected_framerate);

    let opt_jh = consume_future.await;

    // Allow writer thread time to finish writing.
    if let Some(jh) = opt_jh {
        jh.join().expect("join writer_thread_handle");
    }

    // TODO: reenable this to stop nicely.
    // // hmm do we need this? We could just end without idling.
    // runtime.shutdown_on_idle();

    debug!("done {}:{}", file!(), line!());

    Ok(())
}

#[derive(Clone)]
struct LiveStatsCollector {
    shared: Arc<RwLock<ChangeTracker<HttpApiShared>>>,
    collected: Arc<RwLock<BTreeMap<RosCamName, LiveStatsAccum>>>,
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
    fn new(shared: Arc<RwLock<ChangeTracker<HttpApiShared>>>) -> Self {
        let collected = Arc::new(RwLock::new(BTreeMap::new()));
        Self { shared, collected }
    }

    fn register_new_frame_data(&self, name: &RosCamName, n_points: usize) {
        let to_send = {
            // scope for lock on self.collected
            let mut collected = self.collected.write();
            let entry = collected
                .entry(name.clone())
                .or_insert_with(|| LiveStatsAccum::new());
            entry.update(n_points);

            if entry.start.elapsed() > std::time::Duration::from_secs(1) {
                Some((name.clone(), entry.get_results_and_reset()))
            } else {
                None
            }
        };
        if let Some((name, recent_stats)) = to_send {
            // scope for shared scope
            let mut tracker = self.shared.write();
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

fn toggle_saving_csv_tables(
    start_saving: bool,
    expected_framerate_arc: Arc<RwLock<Option<f32>>>,
    output_base_dirname: std::path::PathBuf,
    write_controller_arc: Arc<RwLock<flydra2::CoordProcessorControl>>,
    current_images_arc: Arc<RwLock<flydra2::ImageDictType>>,
    shared_data: Arc<RwLock<ChangeTracker<HttpApiShared>>>,
) {
    if start_saving {
        let expected_framerate = expected_framerate_arc.read();
        let local: chrono::DateTime<chrono::Local> = chrono::Local::now();
        let dirname = local.format("%Y%m%d_%H%M%S.braid").to_string();
        let mut my_dir = output_base_dirname.clone();
        my_dir.push(dirname);
        let write_controller = write_controller_arc.write();
        let current_images = current_images_arc.read();
        let images = (*current_images).clone();
        let cfg = flydra2::StartSavingCsvConfig {
            out_dir: my_dir.clone(),
            local: Some(local),
            git_rev: env!("GIT_HASH").to_string(),
            fps: *expected_framerate,
            images,
            print_stats: false,
            save_performance_histograms: true,
        };
        write_controller.start_saving_data(cfg);

        {
            let mut tracker = shared_data.write();
            tracker.modify(|store| {
                store.csv_tables_dirname = Some(RecordingPath::new(my_dir.display().to_string()));
            });
        }

        // TODO: set filename in shared data.

        // let data_file_topic = format!("/{}/data_file", rosname2);
        // // TODO: create data_file_pub only once
        // let mut data_file_pub = rosrust::publish(&data_file_topic).unwrap();
        // data_file_pub.set_latching(true);
        // let msg = msg::std_msgs::String { data: my_dir.to_string_lossy().into() };
        // data_file_pub.send(msg).unwrap();

        info!("saving data to {:?}", my_dir);
    } else {
        let write_controller = write_controller_arc.write();
        write_controller.stop_saving_data();

        {
            let mut tracker = shared_data.write();
            tracker.modify(|store| {
                store.csv_tables_dirname = None;
            });
        }

        // let data_file_topic = format!("/{}/data_file", rosname2);
        // // TODO: create data_file_pub only once
        // let mut data_file_pub = rosrust::publish(&data_file_topic).unwrap();
        // data_file_pub.set_latching(true);
        // let msg = msg::std_msgs::String { data: "".to_string() };
        // data_file_pub.send(msg).unwrap();

        info!("stopping saving");
    }
}

fn synchronize_cameras(
    triggerbox_cmd: Option<channellib::Sender<flydra1_triggerbox::Cmd>>,
    sync_pulse_pause_started_arc: Arc<RwLock<Option<std::time::Instant>>>,
    mut cam_manager: flydra2::ConnectedCamerasManager,
    time_model_arc: Arc<RwLock<Option<rust_cam_bui_types::ClockModel>>>,
) {
    info!("preparing to synchronize cameras");

    // This time must be prior to actually resetting sync data.
    {
        let mut sync_pulse_pause_started = sync_pulse_pause_started_arc.write();
        *sync_pulse_pause_started = Some(std::time::Instant::now());
    }

    // Now we can reset the sync data.
    cam_manager.reset_sync_data();

    {
        let mut guard = time_model_arc.write();
        *guard = None;
    }

    if let Some(tx) = triggerbox_cmd {
        begin_cam_sync_triggerbox_in_process(tx);
    } else {
        info!("Using fake synchronization method.");
    }
}

fn begin_cam_sync_triggerbox_in_process(tx: channellib::Sender<flydra1_triggerbox::Cmd>) {
    // This is the case when the triggerbox is within this process.
    info!("preparing for triggerbox to temporarily stop sending pulses");

    info!("requesting triggerbox to stop sending pulses");
    use flydra1_triggerbox::Cmd::*;
    tx.send(StopPulsesAndReset).cb_ok();
    // TODO FIXME: fire a tokio_timer to sleep and then to then after it returns.
    // This is probably really bad.
    std::thread::sleep(std::time::Duration::from_secs(
        SYNCHRONIZE_DURATION_SEC as u64,
    ));
    tx.send(StartPulses).cb_ok();
    info!("requesting triggerbox to start sending pulses again");
}

/// run a function returning Result<()> and handle errors.
// see https://github.com/withoutboats/failure/issues/76#issuecomment-347402383
pub fn run_func<F: FnOnce() -> Result<()>>(real_func: F) {
    // Decide which command to run, and run it, and print any errors.
    if let Err(err) = real_func() {
        use std::io::Write;

        let mut stderr = std::io::stderr();
        writeln!(stderr, "Error: {}", err).expect("unable to write error to stderr");
        for cause in err.chain() {
            writeln!(stderr, "Caused by: {}", cause).expect("unable to write error to stderr");
        }
        std::process::exit(1);
    }
}

#[derive(Debug, StructOpt)]
struct Command {
    /// The .xml file with the reconstructor.
    #[structopt(
        parse(from_os_str),
        name = "RECONSTRUCTOR",
        short = "r",
        long = "reconstructor"
    )]
    reconstructor: Option<std::path::PathBuf>,

    /// The network addess to listen for raw flydra messages
    #[structopt(
        name = "LOWLATENCY_CAMDATA_UDP_ADDR",
        short = "a",
        default_value = "127.0.0.1:0",
        long = "lowlatency-camdata-udp-addr"
    )]
    lowlatency_camdata_udp_addr: String,

    /// Trigger device (e.g. /dev/trig1) if used
    #[structopt(name = "TRIGGER_DEVICE", short = "t", long = "trigger_device")]
    trigger_device: String,

    /// trigger device framerate
    #[structopt(
        name = "TRIGGER_DEVICE_FPS",
        long = "trigger_device_fps",
        default_value = "100.0"
    )]
    trigger_device_fps: f32,

    /// How often is the trigger device queried (to synchronize clocks), in seconds
    #[structopt(
        name = "TRIGGER_DEVICE_QUERY_INTERNVAL",
        long = "trigger_device_query_interval",
        default_value = "1"
    )]
    trigger_device_query_interval: f32,

    /// Expect flydra1 network packets from the camera nodes
    #[structopt(name = "FLYDRA1", long = "flydra1")]
    flydra1: bool,

    /// Whether to save timestamp data from frames in which no features detected
    #[structopt(name = "SAVE_EMPTY", long = "save_empty_data2d")]
    save_empty_data2d: bool,

    /// The output directory to save the 3D data.
    #[structopt(parse(from_os_str), name = "OUTPUT", short = "o", long = "output")]
    output: std::path::PathBuf,

    /// Show tracking parameters in TOML format and quit
    #[structopt(name = "SHOW_TRACKING_PARAMS", long = "show-tracking-params")]
    show_tracking_params: bool,

    /// Tracking parameters TOML file.
    #[structopt(parse(from_os_str))]
    tracking_params: Option<std::path::PathBuf>,

    /// The network addess to listen for http api command and control messages
    #[structopt(
        name = "HTTP_API_SERVER_ADDR",
        long = "http-api-server-addr",
        default_value = "127.0.0.1:0"
    )]
    http_api_server_addr: String,

    /// The network addess to listen for http api command and control messages
    #[structopt(name = "HTTP_API_SERVER_TOKEN", long = "http-api-server-token")]
    http_api_server_token: Option<String>,

    /// The network addess for serving the model pose
    #[structopt(
        name = "MODEL_SERVER_ADDR",
        long = "model-server-addr",
        default_value = flydra_types::DEFAULT_MODEL_SERVER_ADDR
    )]
    model_server_addr: std::net::SocketAddr,

    /// The network addess to listen for http api command and control messages
    #[structopt(name = "JWT_SECRET", long = "jwt-secret")]
    jwt_secret: Option<String>,
}
