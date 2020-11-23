#[macro_use]
extern crate log;
#[macro_use]
extern crate failure_derive;
#[macro_use]
extern crate lazy_static;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use futures::{channel::mpsc, sink::SinkExt, stream::StreamExt};
use parking_lot::RwLock;

use bui_backend::highlevel::{create_bui_app_inner, BuiAppInner, ConnectionEventType};
use bui_backend::AccessControl;
use bui_backend_types::CallbackDataAndSession;

use async_change_tracker::ChangeTracker;

use crossbeam_ok::CrossbeamOk;
use http_video_streaming::{AnnotatedFrame, FirehoseCallback};
use machine_vision_formats as formats;
use rt_image_viewer_storetype::{RtImageViewerCallback, StoreType};
use simple_frame::SimpleFrame;

include!(concat!(env!("OUT_DIR"), "/rt-image-viewer-frontend.rs")); // Despite slash, this does work on Windows.

lazy_static! {
    static ref EVENTS_PREFIX: String =
        format!("/{}", rt_image_viewer_storetype::RT_IMAGE_EVENTS_URL_PATH);
}

lazy_static! {
    static ref SENDER: Arc<
        RwLock<
            Option<(
                crossbeam_channel::Sender<AnnotatedFrame<SimpleFrame>>,
                mpsc::Sender<String>
            )>,
        >,
    > = Arc::new(RwLock::new(None));
}

pub type Result<M> = std::result::Result<M, Error>;

#[derive(Fail, Debug)]
pub enum Error {
    #[fail(display = "sender not initialized")]
    SenderNotInitialized,
    #[fail(display = "sender already initialized")]
    SenderAlreadyInitialized,
    #[fail(display = "{}", _0)]
    BuiBackend(#[cause] bui_backend::Error),
    #[fail(display = "{}", _0)]
    HttpVideoStreaming(#[cause] http_video_streaming::Error),
    #[fail(display = "{}", _0)]
    Hyper(#[cause] hyper::Error),
    #[fail(display = "{}", _0)]
    Io(#[cause] std::io::Error),
}

impl From<bui_backend::Error> for Error {
    fn from(orig: bui_backend::Error) -> Error {
        Error::BuiBackend(orig)
    }
}

impl From<http_video_streaming::Error> for Error {
    fn from(orig: http_video_streaming::Error) -> Error {
        Error::HttpVideoStreaming(orig)
    }
}

impl From<hyper::Error> for Error {
    fn from(orig: hyper::Error) -> Error {
        Error::Hyper(orig)
    }
}

impl From<std::io::Error> for Error {
    fn from(orig: std::io::Error) -> Error {
        Error::Io(orig)
    }
}

fn spawn_futures(
    valve: stream_cancel::Valve,
    shutdown_rx: Option<tokio::sync::oneshot::Receiver<()>>,
    secret: Vec<u8>,
    http_server_addr: std::net::SocketAddr,
) -> Result<(thread_control::Control, std::thread::JoinHandle<()>)> {
    let auth = if http_server_addr.ip().is_loopback() {
        AccessControl::Insecure(http_server_addr)
    } else {
        bui_backend::highlevel::generate_random_auth(http_server_addr, secret)?
    };

    let my_app = RtImageViewerBuiApp::new(valve, shutdown_rx, auth)?;

    let firehose_tx = my_app.get_sender();
    let image_name_tx = my_app.get_image_name_sender();

    {
        let mut data = (*SENDER).write();
        *data = Some((firehose_tx, image_name_tx));
    }

    info!("image debugger running at http://{}", http_server_addr);

    Ok((my_app.control, my_app.jh))
}

pub fn initialize_rt_image_viewer(
    valve: stream_cancel::Valve,
    shutdown_rx: Option<tokio::sync::oneshot::Receiver<()>>,
    secret: &[u8],
    http_server_addr: &std::net::SocketAddr,
) -> Result<(thread_control::Control, std::thread::JoinHandle<()>)> {
    if (*SENDER).read().is_some() {
        // already initialized
        return Err(Error::SenderAlreadyInitialized);
    }

    let secret2: Vec<u8> = secret.into();
    let http_server_addr2 = http_server_addr.clone();

    Ok(spawn_futures(
        valve,
        shutdown_rx,
        secret2,
        http_server_addr2,
    )?)
}

fn copy_into_frame<S>(frame: &S) -> SimpleFrame
where
    S: fastimage::FastImage<C = fastimage::Chan1, D = u8>,
{
    let pixel_format = formats::PixelFormat::MONO8;
    let mut image_data: Vec<u8> =
        Vec::with_capacity((frame.size().width() * frame.size().height()) as usize);
    for row in 0..frame.size().height() as usize {
        image_data.extend(frame.row_slice(row));
    }
    SimpleFrame {
        width: frame.size().width() as u32,
        height: frame.size().height() as u32,
        stride: frame.size().width() as u32,
        image_data,
        pixel_format,
    }
}

// this is Sync and Send
pub struct RtImageViewerSender {
    firehose_tx: crossbeam_channel::Sender<AnnotatedFrame<SimpleFrame>>,
    image_name_tx: mpsc::Sender<String>,
}

impl RtImageViewerSender {
    /// Create a new instance of `RtImageViewerSender`. This should be done once per thread,
    /// as it creates a copy of the `crossbeam_channel::Sender<AnnotatedFrame>`.
    pub fn new() -> Result<Self> {
        let (firehose_tx, image_name_tx) = {
            let data = (*SENDER).write();
            match *data {
                Some((ref firehose_tx, ref image_name_tx)) => {
                    (firehose_tx.clone(), image_name_tx.clone())
                }
                None => return Err(Error::SenderNotInitialized),
            }
        };

        Ok(Self {
            firehose_tx,
            image_name_tx,
        })
    }
    pub fn send<S>(&mut self, frame: &S, image_name: &str) -> Result<()>
    where
        S: fastimage::FastImage<C = fastimage::Chan1, D = u8>,
    {
        let frame = Box::new(copy_into_frame(frame));

        let aframe = AnnotatedFrame {
            found_points: Vec::new(),
            frame: *frame,
            valid_display: None,
            name: Some(image_name.to_string()),
            annotations: vec![],
        };

        if let Some(ref name) = aframe.name {
            futures::executor::block_on(self.image_name_tx.send(name.to_string())).unwrap();
        }

        self.firehose_tx.send(aframe).cb_ok();
        Ok(())
    }
}

struct RtImageViewerBuiApp {
    _inner: BuiAppInner<StoreType, RtImageViewerCallback>,
    firehose_tx: crossbeam_channel::Sender<AnnotatedFrame<SimpleFrame>>,
    image_name_tx: mpsc::Sender<String>,
    control: thread_control::Control,
    jh: std::thread::JoinHandle<()>,
}

impl RtImageViewerBuiApp {
    fn new(
        valve: stream_cancel::Valve,
        shutdown_rx: Option<tokio::sync::oneshot::Receiver<()>>,
        auth: AccessControl,
    ) -> Result<Self> {
        let mut config = get_default_config();
        config.cookie_name = "rt-image-viewer-client".to_string();
        let shared_store = ChangeTracker::new(StoreType {
            image_names: HashSet::new(),
            image_info: None,
        });
        let tracker_arc = Arc::new(RwLock::new(shared_store));
        let chan_size = 10;
        let (new_conn_rx, mut inner) = create_bui_app_inner(
            shutdown_rx,
            &auth,
            tracker_arc.clone(),
            config,
            chan_size,
            &*EVENTS_PREFIX,
            Some(rt_image_viewer_storetype::RT_IMAGE_EVENT_NAME.to_string()),
        )?;

        // A channel for the data send from the client browser. No need to convert to
        // bounded to prevent exploding when camera too fast.
        let (firehose_callback_tx, firehose_callback_rx) = crossbeam_channel::unbounded();

        let (image_name_tx, image_name_rx) = mpsc::channel(100);
        let mut image_name_rx_wrapped = valve.wrap(image_name_rx);

        let image_name_rx_future = async move {
            while let Some(name) = image_name_rx_wrapped.next().await {
                let mut shared = tracker_arc.write();
                shared.modify(|shared| {
                    shared.image_names.insert(name);
                });
            }
        };
        tokio::spawn(Box::pin(image_name_rx_future));

        // Create a Stream to handle callbacks from clients.
        inner.set_callback_listener(Box::new(
            move |msg: CallbackDataAndSession<RtImageViewerCallback>| {
                match msg.payload {
                    RtImageViewerCallback::FirehoseNotify(inner) => {
                        let arrival_time = chrono::Utc::now();
                        let fc = FirehoseCallback {
                            arrival_time,
                            inner,
                        };
                        firehose_callback_tx.send(fc).cb_ok();
                    }
                }
                futures::future::ok(())
            },
        ));

        let txers = Arc::new(RwLock::new(HashMap::new()));
        let txers2 = txers.clone();

        let new_conn_future = new_conn_rx.for_each(move |msg| {
            let mut txers = txers2.write();
            match msg.typ {
                ConnectionEventType::Connect(chunk_sender) => {
                    txers.insert(
                        msg.connection_key,
                        (msg.session_key, chunk_sender, msg.path),
                    );
                }
                ConnectionEventType::Disconnect => {
                    txers.remove(&msg.connection_key);
                }
            }
            futures::future::ready(())
        });
        tokio::spawn(Box::pin(new_conn_future));

        let (flag, control) = thread_control::make_pair();

        // TODO: convert to bounded to prevent exploding when camera too fast?
        let (firehose_tx, firehose_rx) =
            crossbeam_channel::unbounded::<AnnotatedFrame<SimpleFrame>>();
        let jh = std::thread::spawn(move || {
            run_func(move || {
                Ok(http_video_streaming::firehose_thread(
                    txers,
                    firehose_rx,
                    firehose_callback_rx,
                    true,
                    &*EVENTS_PREFIX,
                    flag,
                )?)
            });
        });

        Ok(Self {
            _inner: inner,
            firehose_tx,
            image_name_tx,
            control,
            jh,
        })
    }

    fn get_image_name_sender(&self) -> mpsc::Sender<String> {
        self.image_name_tx.clone()
    }

    fn get_sender(&self) -> crossbeam_channel::Sender<AnnotatedFrame<SimpleFrame>> {
        self.firehose_tx.clone()
    }
}

/// run a function returning Result<()> and handle errors.
// see https://github.com/withoutboats/failure/issues/76#issuecomment-347402383
fn run_func<F: FnOnce() -> Result<()>>(real_func: F) {
    // Decide which command to run, and run it, and print any errors.
    if let Err(err) = real_func() {
        use std::io::Write;

        let mut stderr = std::io::stderr();
        writeln!(stderr, "Error: {}", err).expect("unable to write error to stderr");
        for cause in failure::Fail::iter_causes(&err) {
            writeln!(stderr, "Caused by: {}", cause).expect("unable to write error to stderr");
        }
        std::process::exit(1);
    }
}
