use tracing::{debug, error, info};

use std::{future::Future, pin::Pin};

use futures::sink::SinkExt;
use serde::{Deserialize, Serialize};

use futures::stream::StreamExt;
use http_body_util::BodyExt;
use hyper::header::ACCEPT;
use hyper::{Method, Response, StatusCode};

use crate::{Result, TimeDataPassthrough};

use flydra_types::{FlydraFloatTimestampLocal, StaticMainbrainInfo, SyncFno, Triggerbox};

#[cfg(any(feature = "bundle_files", feature = "serve_files"))]
include!(concat!(env!("OUT_DIR"), "/public.rs")); // Despite slash, this does work on Windows.

pub type EventChunkSender = tokio::sync::mpsc::Sender<hyper::body::Bytes>;

#[derive(Debug)]
pub struct NewEventStreamConnection {
    /// A sink for messages send to each connection (one per client tab).
    pub chunk_sender: EventChunkSender,
}

#[derive(Clone)]
struct ModelService {
    events_path: String,
    config_serve_filepath: String,
    config_channel_size: usize,
    tx_new_connection: futures::channel::mpsc::Sender<NewEventStreamConnection>,
    info: StaticMainbrainInfo,
    valve: stream_cancel::Valve,
    rt_handle: tokio::runtime::Handle,
}

impl ModelService {
    fn new(
        valve: stream_cancel::Valve,
        tx_new_connection: futures::channel::mpsc::Sender<NewEventStreamConnection>,
        info: StaticMainbrainInfo,
        rt_handle: tokio::runtime::Handle,
    ) -> Self {
        Self {
            valve,
            events_path: "/events".to_string(),
            config_serve_filepath: "static".to_string(),
            config_channel_size: 100,
            tx_new_connection,
            info,
            rt_handle,
        }
    }

    #[allow(dead_code)]
    fn fullpath(&self, path: &str) -> String {
        assert!(path.starts_with('/')); // security check
        let path = std::path::PathBuf::from(path)
            .strip_prefix("/")
            .unwrap()
            .to_path_buf();
        assert!(!path.starts_with("..")); // security check

        let base = std::path::PathBuf::from(self.config_serve_filepath.clone());
        let result = base.join(path);
        result.into_os_string().into_string().unwrap()
    }

    #[cfg(not(any(feature = "bundle_files", feature = "serve_files")))]
    fn get_file_content(&self, _file_path: &str) -> Option<Vec<u8>> {
        None
    }

    #[cfg(feature = "bundle_files")]
    fn get_file_content(&self, file_path: &str) -> Option<Vec<u8>> {
        let fullpath = self.fullpath(file_path);
        let r = PUBLIC.get(&fullpath);
        match r {
            Ok(s) => Some(s.into_owned()),
            Err(_) => None,
        }
    }

    #[cfg(feature = "serve_files")]
    fn get_file_content(&self, file_path: &str) -> Option<Vec<u8>> {
        let fullpath = self.fullpath(file_path);
        let contents = match std::fs::read(&fullpath) {
            Ok(contents) => contents,
            Err(e) => {
                error!("requested path {:?}, but got error {:?}", file_path, e);
                return None;
            }
        };
        Some(contents)
    }
}

type MyBody = http_body_util::combinators::BoxBody<bytes::Bytes, hyper::Error>;

fn body_from_buf(body_buf: &[u8]) -> MyBody {
    let body = http_body_util::Full::new(bytes::Bytes::from(body_buf.to_vec()));
    MyBody::new(body.map_err(|_: std::convert::Infallible| unreachable!()))
}

impl hyper::service::Service<hyper::Request<hyper::body::Incoming>> for ModelService {
    type Response = hyper::Response<MyBody>;
    type Error = hyper::Error;
    type Future =
        Pin<Box<dyn Future<Output = std::result::Result<Self::Response, Self::Error>> + Send>>;

    fn call(&self, req: http::Request<hyper::body::Incoming>) -> Self::Future {
        let resp = Response::builder();
        debug!("got request {:?}", req);
        let resp_final = match (req.method(), req.uri().path()) {
            (&Method::GET, path) => {
                let path = if path == "/" { "/index.html" } else { path };

                if path == "/info" {
                    let buf = serde_json::to_string_pretty(&self.info).unwrap();
                    let len = buf.len();
                    let body = body_from_buf(buf.as_bytes());
                    resp.header(hyper::header::CONTENT_LENGTH, format!("{}", len).as_bytes())
                        .header(
                            hyper::header::CONTENT_TYPE,
                            hyper::header::HeaderValue::from_str("application/json")
                                .expect("from_str"),
                        )
                        .body(body)
                        .expect("response") // todo map err
                } else if path == self.events_path {
                    let mut accepts_event_stream = false;
                    for value in req.headers().get_all(ACCEPT).iter() {
                        if value
                            .to_str()
                            .expect("to_str()")
                            .contains("text/event-stream")
                        {
                            accepts_event_stream = true;
                        }
                    }

                    if accepts_event_stream {
                        let (tx_event_stream, rx_event_stream) =
                            tokio::sync::mpsc::channel(self.config_channel_size);
                        let tx_event_stream: EventChunkSender = tx_event_stream; // type annotation only

                        let rx_event_stream = self
                            .valve
                            .wrap(tokio_stream::wrappers::ReceiverStream::new(rx_event_stream));

                        let rx_event_stream = rx_event_stream
                            .map(|data: bytes::Bytes| Ok::<_, _>(http_body::Frame::data(data)));

                        {
                            let conn_info = NewEventStreamConnection {
                                chunk_sender: tx_event_stream,
                            };

                            let mut tx_new_connection2 = self.tx_new_connection.clone();
                            let fut = async move {
                                match tx_new_connection2.send(conn_info).await {
                                    Ok(()) => {}
                                    Err(e) => error!("sending new connection info failed: {}", e),
                                }
                            };

                            self.rt_handle.spawn(fut);
                        }

                        let body1 = http_body_util::StreamBody::new(rx_event_stream);
                        let body2 = http_body_util::BodyExt::boxed(body1);

                        resp.header(
                            hyper::header::CONTENT_TYPE,
                            hyper::header::HeaderValue::from_str("text/event-stream")
                                .expect("from_str"),
                        )
                        .body(body2)
                        .expect("response") // todo map err
                    } else {
                        let msg = r#"<!doctype html>
<html lang="en">
    <head>
        <meta charset="utf-8">
        <title>Error - bad request</title>
    </head>
    <body>
        <h1>Error - bad request</h1>
        Event request does not specify 'Accept' HTTP Header or does not accept
        the required 'text/event-stream'. (View event stream live in browser
        <a href="/">here</a>.)
    </body>
</html>"#
                            .to_string();
                        resp.status(StatusCode::BAD_REQUEST)
                            .body(body_from_buf(msg.as_bytes()))
                            .expect("response") // todo map err
                    }
                } else {
                    // TODO read file asynchronously
                    match self.get_file_content(path) {
                        Some(buf) => {
                            let len = buf.len();
                            let body = body_from_buf(&buf);
                            resp.header(
                                hyper::header::CONTENT_LENGTH,
                                format!("{}", len).as_bytes(),
                            )
                            .body(body)
                            .expect("response") // todo map err
                        }
                        None => {
                            resp.status(StatusCode::NOT_FOUND)
                                .body(body_from_buf(b""))
                                .expect("response") // todo map err
                        }
                    }
                }
            }
            _ => {
                resp.status(StatusCode::NOT_FOUND)
                    .body(body_from_buf(b""))
                    .expect("response") // todo map err
            }
        };
        Box::pin(futures::future::ok(resp_final))
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

#[derive(Clone)]
pub struct ModelServer {
    local_addr: std::net::SocketAddr,
}

pub async fn new_model_server(
    data_rx: tokio::sync::mpsc::Receiver<(SendType, TimeDataPassthrough)>,
    valve: stream_cancel::Valve,
    addr: &std::net::SocketAddr,
    info: StaticMainbrainInfo,
    rt_handle: tokio::runtime::Handle,
) -> Result<ModelServer> {
    {
        let channel_size = 2;
        let (tx_new_connection, rx_new_connection) = futures::channel::mpsc::channel(channel_size);

        let service = ModelService::new(
            valve.clone(),
            tx_new_connection,
            info.clone(),
            rt_handle.clone(),
        );

        let service2 = service.clone();

        let listener = tokio::net::TcpListener::bind(addr).await?;

        let local_addr = listener.local_addr()?;
        let handle2 = rt_handle.clone();

        rt_handle.spawn(async move {
            loop {
                let (socket, _remote_addr) = listener.accept().await.unwrap();
                let model_service = service2.clone();

                // Spawn a task to handle the connection. That way we can multiple connections
                // concurrently.
                handle2.spawn(async move {
                    // Hyper has its own `AsyncRead` and `AsyncWrite` traits and doesn't use tokio.
                    // `TokioIo` converts between them.
                    let socket = hyper_util::rt::TokioIo::new(socket);
                    let model_server = model_service.clone();

                    let hyper_service = hyper::service::service_fn(
                        move |request: hyper::Request<hyper::body::Incoming>| {
                            // Do we need to call `poll_ready`????
                            use hyper::service::Service;
                            model_server.call(request)
                        },
                    );

                    // `server::conn::auto::Builder` supports both http1 and http2.
                    //
                    // `TokioExecutor` tells hyper to use `tokio::spawn` to spawn tasks.
                    if let Err(err) = hyper_util::server::conn::auto::Builder::new(
                        hyper_util::rt::TokioExecutor::new(),
                    )
                    // `serve_connection_with_upgrades` is required for websockets. If you don't need
                    // that you can use `serve_connection` instead.
                    .serve_connection_with_upgrades(socket, hyper_service)
                    .await
                    {
                        eprintln!("failed to serve connection: {err:#}");
                    }
                });
            }
        });

        info!(
            "ModelServer at http://{}:{}/",
            local_addr.ip(),
            local_addr.port()
        );

        debug!(
            "ModelServer events at http://{}:{}{}",
            local_addr.ip(),
            local_addr.port(),
            service.events_path
        );

        let result = ModelServer { local_addr };

        let mut rx_new_connection_valved = valve.wrap(rx_new_connection);
        let mut data_rx = tokio_stream::wrappers::ReceiverStream::new(data_rx);

        let main_task = async move {
            let mut connections: Vec<NewEventStreamConnection> = vec![];
            let mut current_calibration: Option<(SendType, TimeDataPassthrough)> = None;
            loop {
                tokio::select! {
                    opt_new_connection = rx_new_connection_valved.next() => {
                        match opt_new_connection {
                            Some(new_connection) => {

                                if let Some(data) = &current_calibration {
                                    let bytes = get_body(data)?;
                                    new_connection.chunk_sender.send(bytes.clone()).await.unwrap();
                                }

                                connections.push(new_connection);
                            }
                            None => {
                                // All senders done. (So the server has quit and so should we.)
                                break;
                            }
                        }
                    }
                    opt_new_data = data_rx.next() => {
                        match &opt_new_data {
                            Some(data) => {
                                if let (SendType::CalibrationFlydraXml(_),_) = &data {
                                    current_calibration = Some(data.clone());
                                }
                                send_msg(data, &mut connections).await?;
                            }
                            None => {
                                // All senders done. No new data will be coming, so quit.
                                break;
                            }
                        }


                    }
                }
            }
            Ok::<_, crate::Error>(())
        };
        rt_handle.spawn(main_task);
        Ok(result)
    }
}

impl ModelServer {
    pub fn local_addr(&self) -> &std::net::SocketAddr {
        &self.local_addr
    }
}

fn get_body(data: &(SendType, TimeDataPassthrough)) -> Result<hyper::body::Bytes> {
    let (msg, tdpt) = data;
    let latency: f64 = if let Some(ref tt) = tdpt.trigger_timestamp() {
        let now_f64 = datetime_conversion::datetime_to_f64(&chrono::Local::now());
        now_f64 - tt.as_f64()
    } else {
        std::f64::NAN
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
    let buf = serde_json::to_string(&data)?;
    // Encode as event source.
    let buf = format!("event: braid\ndata: {}\n\n", buf);

    let bytes: hyper::body::Bytes = buf.into();
    Ok(bytes)
}

async fn send_msg(
    data: &(SendType, TimeDataPassthrough),
    connections: &mut Vec<NewEventStreamConnection>,
) -> Result<()> {
    let bytes = get_body(data)?;

    // Send to all listening connections.
    let keep: Vec<bool> = futures::future::join_all(
        connections
            .iter_mut()
            .map(|conn| async { conn.chunk_sender.send(bytes.clone()).await.is_ok() }),
    )
    .await;

    assert_eq!(keep.len(), connections.len());

    // Remove connections which resulted in error.
    let mut index = 0;
    connections.retain(|_| {
        index += 1;
        keep[index - 1]
    });

    Ok(())
}
