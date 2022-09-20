use log::{debug, error, info};

use std::pin::Pin;

use futures::sink::SinkExt;
use serde::{Deserialize, Serialize};

use futures::stream::StreamExt;
use hyper::header::ACCEPT;
use hyper::{Method, Response, StatusCode};

use crate::{Result, TimeDataPassthrough};

use flydra_types::{FlydraFloatTimestampLocal, StaticMainbrainInfo, SyncFno, Triggerbox};

type MyError = std::io::Error; // anything that implements std::error::Error and Send

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

type ServiceResult = Pin<
    Box<
        dyn futures::future::Future<
                Output = std::result::Result<http::Response<hyper::Body>, hyper::Error>,
            > + Send,
    >,
>;

impl tower_service::Service<http::Request<hyper::Body>> for ModelService {
    type Response = http::Response<hyper::Body>;
    type Error = hyper::Error;

    // should Self::Future also implement Unpin??
    type Future = ServiceResult;

    fn poll_ready(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::result::Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: http::Request<hyper::Body>) -> Self::Future {
        let resp = Response::builder();
        debug!("got request {:?}", req);
        let resp_final = match (req.method(), req.uri().path()) {
            (&Method::GET, path) => {
                let path = if path == "/" { "/index.html" } else { path };

                if path == "/info" {
                    let buf = serde_json::to_string_pretty(&self.info).unwrap();
                    let len = buf.len();
                    let body = hyper::Body::from(buf);
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

                        let rx_event_stream = rx_event_stream.map(Ok::<_, MyError>);

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
                        resp.header(
                            hyper::header::CONTENT_TYPE,
                            hyper::header::HeaderValue::from_str("text/event-stream")
                                .expect("from_str"),
                        )
                        .body(hyper::Body::wrap_stream(rx_event_stream))
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
                            .body(msg.into())
                            .expect("response") // todo map err
                    }
                } else {
                    // TODO read file asynchronously
                    match self.get_file_content(path) {
                        Some(buf) => {
                            let len = buf.len();
                            let body = hyper::Body::from(buf);
                            resp.header(
                                hyper::header::CONTENT_LENGTH,
                                format!("{}", len).as_bytes(),
                            )
                            .body(body)
                            .expect("response") // todo map err
                        }
                        None => {
                            resp.status(StatusCode::NOT_FOUND)
                                .body(hyper::Body::empty())
                                .expect("response") // todo map err
                        }
                    }
                }
            }
            _ => {
                resp.status(StatusCode::NOT_FOUND)
                    .body(hyper::Body::empty())
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

#[derive(Serialize, Deserialize, Debug)]
pub enum SendType {
    // IMPORTANT NOTE: if you change this type, be sure to change the version
    // value `v`. Search for the string ZP4q and `Braid pose API`.
    Birth(SendKalmanEstimatesRow),
    Update(SendKalmanEstimatesRow),
    Death(u32), // obj_id

    EndOfFrame(SyncFno),
    /// the multicamera calibration serialized into a flydra xml file
    CalibrationFlydraXml(Vec<u8>),
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
    shutdown_rx: Option<tokio::sync::oneshot::Receiver<()>>,
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
        let new_service = hyper::service::make_service_fn(move |_socket| {
            futures::future::ok::<_, MyError>(service2.clone())
        });

        let server = {
            // this will fail unless there is a reactor already
            let bound = async { hyper::Server::try_bind(addr) }.await?;
            bound.serve(new_service)
        };

        let local_addr = server.local_addr();

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
            loop {
                tokio::select! {
                    opt_new_connection = rx_new_connection_valved.next() => {
                        match opt_new_connection {
                            Some(new_connection) => {
                                connections.push(new_connection);
                            }
                            None => {
                                // All senders done. (So the server has quit and so should we.)
                                break;
                            }
                        }
                    }
                    opt_new_data = data_rx.next() => {
                        match opt_new_data {
                            Some(data) => {
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

        use futures::future::FutureExt;
        let log_and_swallow_err = |r| match r {
            Ok(_) => {}
            Err(e) => {
                error!("{} ({}:{})", e, file!(), line!());
            }
        };

        if let Some(shutdown_rx) = shutdown_rx {
            let graceful = server.with_graceful_shutdown(async move {
                shutdown_rx.await.ok();
            });
            rt_handle.spawn(Box::pin(graceful.map(log_and_swallow_err)));
        } else {
            rt_handle.spawn(Box::pin(server.map(log_and_swallow_err)));
        };
        Ok(result)
    }
}

impl ModelServer {
    pub fn local_addr(&self) -> &std::net::SocketAddr {
        &self.local_addr
    }
}

async fn send_msg(
    data: (SendType, TimeDataPassthrough),
    connections: &mut Vec<NewEventStreamConnection>,
) -> Result<()> {
    let (msg, tdpt) = data;
    let latency: f64 = if let Some(ref tt) = tdpt.trigger_timestamp() {
        let now_f64 = datetime_conversion::datetime_to_f64(&chrono::Local::now());
        now_f64 - tt.as_f64()
    } else {
        std::f64::NAN
    };

    // Send updates after each observation for lowest-possible latency.
    let data = ToListener {
        /// Braid pose API
        v: 2, // <- Bump when ToListener or SendType definition changes ZP4q
        msg,
        latency,
        synced_frame: tdpt.synced_frame(),
        trigger_timestamp: tdpt.trigger_timestamp(),
    };

    // Serialize to JSON.
    let buf = serde_json::to_string(&data)?;
    // Encode as event source.
    let buf = format!("event: braid\ndata: {}\n\n", buf);

    let bytes: hyper::body::Bytes = buf.into();

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
