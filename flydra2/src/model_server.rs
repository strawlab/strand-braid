use tracing::{debug, info};

use std::sync::{Arc, RwLock};

use http_body::Frame;
use serde::{Deserialize, Serialize};

use event_stream_types::{AcceptsEventStream, EventBroadcaster};

use crate::{Result, TimeDataPassthrough};

use flydra_types::{FlydraFloatTimestampLocal, SyncFno, Triggerbox};

const EVENTS_PATH: &str = "/events";

#[cfg(feature = "bundle_files")]
static ASSETS_DIR: include_dir::Dir<'static> =
    include_dir::include_dir!("$CARGO_MANIFEST_DIR/static");

async fn events_handler(
    axum::extract::State(app_state): axum::extract::State<ModelServerAppState>,
    _: AcceptsEventStream,
) -> impl axum::response::IntoResponse {
    let key = {
        let mut next_connection_id = app_state.next_connection_id.write().unwrap();
        let key = *next_connection_id;
        *next_connection_id += 1;
        key
    };
    let (tx, body) = app_state.event_broadcaster.new_connection(key);

    // If we have a calibration, extract it.
    let cal_data = {
        // scope for read lock on app_state.current_calibration
        let current_calibration = app_state.current_calibration.read().unwrap();
        if let Some((cal_data, tdpt)) = &*current_calibration {
            let data = (
                SendType::CalibrationFlydraXml(cal_data.clone()),
                tdpt.clone(),
            );
            Some(data)
        } else {
            None
        }
    };

    // If we extracted a calibration above, send it already now.
    if let Some(cal_data) = cal_data {
        let cal_body = get_body(&cal_data);
        tx.send(Ok(Frame::data(cal_body.into()))).await.unwrap();
    }

    body
}

#[derive(Clone)]
struct ModelServerAppState {
    current_calibration: Arc<RwLock<Option<(String, TimeDataPassthrough)>>>,
    event_broadcaster: EventBroadcaster<usize>,
    next_connection_id: Arc<RwLock<usize>>,
}

impl Default for ModelServerAppState {
    fn default() -> Self {
        Self {
            current_calibration: Arc::new(RwLock::new(None)),
            event_broadcaster: Default::default(),
            next_connection_id: Arc::new(RwLock::new(0)),
        }
    }
}

#[allow(non_snake_case)]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SendKalmanEstimatesRow {
    pub obj_id: u32,
    pub frame: SyncFno,
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

impl From<flydra_types::KalmanEstimatesRow> for SendKalmanEstimatesRow {
    fn from(orig: flydra_types::KalmanEstimatesRow) -> SendKalmanEstimatesRow {
        SendKalmanEstimatesRow {
            obj_id: orig.obj_id,
            frame: orig.frame,
            x: orig.x,
            y: orig.y,
            z: orig.z,
            xvel: orig.xvel,
            yvel: orig.yvel,
            zvel: orig.zvel,
            P00: orig.P00,
            P01: orig.P01,
            P02: orig.P02,
            P11: orig.P11,
            P12: orig.P12,
            P22: orig.P22,
            P33: orig.P33,
            P44: orig.P44,
            P55: orig.P55,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum SendType {
    // IMPORTANT NOTE: if you change this type, be sure to change the version
    // value `v`. Search for the string ZP4q and `Braid pose API`.
    Birth(SendKalmanEstimatesRow),
    Update(SendKalmanEstimatesRow),
    Death(u32), // obj_id

    EndOfFrame(SyncFno),
    /// the multicamera calibration serialized into a flydra xml file
    CalibrationFlydraXml(String),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ToListener {
    // IMPORTANT NOTE: if you change this type, be sure to change the version
    // value `v`. Search for the string ZP4q and `Braid pose API`.
    /// version
    v: u16,
    msg: SendType,
    latency: f64,
    synced_frame: SyncFno,
    #[serde(with = "flydra_types::timestamp_opt_f64")]
    trigger_timestamp: Option<FlydraFloatTimestampLocal<Triggerbox>>,
}

pub async fn new_model_server(
    mut data_rx: tokio::sync::mpsc::Receiver<(SendType, TimeDataPassthrough)>,
    addr: std::net::SocketAddr,
) -> Result<()> {
    let app_state = ModelServerAppState::default();

    let listener = tokio::net::TcpListener::bind(addr).await?;

    #[cfg(feature = "bundle_files")]
    let serve_dir = tower_serve_static::ServeDir::new(&ASSETS_DIR);

    #[cfg(feature = "serve_files")]
    let serve_dir = tower_http::services::fs::ServeDir::new(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("static"),
    );

    // Create axum router.
    let router = axum::Router::new()
        .route(EVENTS_PATH, axum::routing::get(events_handler))
        .nest_service("/", serve_dir)
        .with_state(app_state.clone());

    // create future for our app
    let http_serve_future = {
        use std::future::IntoFuture;
        axum::serve(listener, router).into_future()
    };

    info!("ModelServer at http://{}:{}/", addr.ip(), addr.port());

    debug!(
        "ModelServer events at http://{}:{}{}",
        addr.ip(),
        addr.port(),
        EVENTS_PATH,
    );

    // Infinite loop to process and forward data.
    let app_state2 = app_state.clone();
    let new_data_processor_future = async move {
        let app_state = app_state2;

        const ENV_KEY: &str = "RERUN_VIEWER_ADDR";
        let rec = std::env::var_os(ENV_KEY).map(|addr_str| {
            let socket_addr = std::net::ToSocketAddrs::to_socket_addrs(addr_str.to_str().unwrap())
                .unwrap()
                .next()
                .unwrap();
            tracing::info!("Streaming data to rerun at {socket_addr}");
            re_sdk::RecordingStreamBuilder::new("braid")
                .connect_tcp_opts(socket_addr, None)
                .unwrap()
        });

        if rec.is_none() {
            tracing::info!(
                "No Rerun viewer address specified with environment variable \
            \"{ENV_KEY}\", not logging data to Rerun. (Hint: the Rerun Viewer \
                listens by default on port 9876.)"
            );
        }

        // Wait for the next update time to arrive ...
        loop {
            let opt_new_data = data_rx.recv().await;
            match &opt_new_data {
                Some(data) => {
                    if let (SendType::CalibrationFlydraXml(calib), tdpt) = &data {
                        let mut current_calibration =
                            app_state.current_calibration.write().unwrap();
                        *current_calibration = Some((calib.clone(), tdpt.clone()));
                    }
                    send_msg(data, &app_state).await?;

                    if let Some(rec) = &rec {
                        match data {
                            (SendType::CalibrationFlydraXml(_calib), _tdpt) => {}
                            (SendType::Birth(row), _tdpt) | (SendType::Update(row), _tdpt) => {
                                let obj_id = format!("{}", row.obj_id);
                                let position = re_types::datatypes::Vec3D::new(
                                    row.x as f32,
                                    row.y as f32,
                                    row.z as f32,
                                );
                                rec.log(obj_id, &re_types::archetypes::Points3D::new([position]))
                                    .unwrap();
                            }
                            (SendType::Death(_x), _tdpt) => {}
                            (SendType::EndOfFrame(_x), _tdpt) => {}
                        }
                    }
                }
                None => {
                    // All senders done. No new data will be coming, so quit.
                    break;
                }
            }
        }
        Ok::<_, crate::Error>(())
    };

    // Wait for one of our futures to finish...
    tokio::select! {
        result = new_data_processor_future => {result?}
        _ = http_serve_future => {}
    }
    // ...then exit.

    Ok(())
}

fn get_body(data: &(SendType, TimeDataPassthrough)) -> String {
    let (msg, tdpt) = data;
    let latency: f64 = if let Some(ref tt) = tdpt.trigger_timestamp() {
        let now_f64 = datetime_conversion::datetime_to_f64(&chrono::Local::now());
        now_f64 - tt.as_f64()
    } else {
        f64::NAN
    };

    // Send updates after each observation for lowest-possible latency.
    let data = ToListener {
        // Braid pose API
        v: 3, // <- Bump when ToListener or SendType definition changes ZP4q
        msg: msg.clone(),
        latency,
        synced_frame: tdpt.synced_frame(),
        trigger_timestamp: tdpt.trigger_timestamp(),
    };

    // Serialize to JSON.
    let buf = serde_json::to_string(&data).unwrap();
    // Encode as event source.
    let buf = format!("event: braid\ndata: {}\n\n", buf);
    buf
}

async fn send_msg(
    data: &(SendType, TimeDataPassthrough),
    app_state: &ModelServerAppState,
) -> Result<()> {
    let buf = get_body(data);
    app_state.event_broadcaster.broadcast_frame(buf).await;
    Ok(())
}
