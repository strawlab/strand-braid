#![cfg_attr(feature = "backtrace", feature(backtrace))]

#[macro_use]
extern crate log;

use parking_lot::Mutex;
use std::{collections::HashMap, sync::Arc};

use tokio_stream::StreamExt;

use bui_backend::{
    highlevel::{ConnectionEvent, ConnectionEventType},
    lowlevel::EventChunkSender,
};
use bui_backend_types::ConnectionKey;

use basic_frame::DynamicFrame;

pub use http_video_streaming_types::{
    CircleParams, DrawableShape, FirehoseCallbackInner, Point, Shape, ToClient,
};

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unknown path")]
    UnknownPath(#[cfg(feature = "backtrace")] std::backtrace::Backtrace),
    #[error(transparent)]
    ConvertImageError(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        convert_image::Error,
    ),
}

// future: use MediaSource API? https://w3c.github.io/media-source

#[derive(Debug)]
pub struct AnnotatedFrame {
    pub frame: DynamicFrame,
    pub found_points: Vec<Point>,
    pub valid_display: Option<Shape>,
    pub name: Option<String>,
    pub annotations: Vec<DrawableShape>,
}

fn _test_annotated_frame_is_send() {
    // Compile-time test to ensure AnnotatedFrame implements Send trait.
    fn implements<T: Send>() {}
    implements::<AnnotatedFrame>();
}

#[derive(Debug)]
pub struct FirehoseCallback {
    pub arrival_time: chrono::DateTime<chrono::Utc>,
    pub inner: FirehoseCallbackInner,
}

struct PerSender {
    name_selector: NameSelector,
    out: EventChunkSender,
    frame_lifo: Option<Arc<Mutex<AnnotatedFrame>>>,
    ready_to_send: bool,
    conn_key: ConnectionKey,
    fno: u64,
}

fn _test_per_sender_is_send() {
    // Compile-time test to ensure PerSender implements Send trait.
    fn implements<T: Send>() {}
    implements::<PerSender>();
}

#[derive(Debug)]
pub enum NameSelector {
    All,
    None,
    Name(String),
}

impl PerSender {
    fn new(
        out: EventChunkSender,
        conn_key: ConnectionKey,
        name_selector: NameSelector,
        frame: Arc<Mutex<AnnotatedFrame>>,
    ) -> PerSender {
        PerSender {
            name_selector,
            out,
            frame_lifo: Some(frame),
            ready_to_send: true,
            conn_key,
            fno: 0,
        }
    }
    fn push(&mut self, frame: Arc<Mutex<AnnotatedFrame>>) {
        let use_frame = match self.name_selector {
            NameSelector::All => true,
            NameSelector::None => false,
            NameSelector::Name(ref select_name) => {
                let mut tmp = false;
                if let Some(ref this_name) = frame.lock().name {
                    if this_name == select_name {
                        tmp = true;
                    }
                }
                tmp
            }
        };
        if use_frame {
            self.fno += 1;
            self.frame_lifo = Some(frame);
        }
    }
    fn got_callback(&mut self, msg: FirehoseCallback) {
        match chrono::DateTime::parse_from_rfc3339(&msg.inner.ts_rfc3339) {
            // match chrono::DateTime<chrono::FixedOffset>::parse_from_rfc3339(&msg.inner.ts_rfc3339) {
            Ok(sent_time) => {
                let latency = msg.arrival_time.signed_duration_since(sent_time);
                trace!("latency: {:?}", latency);
            }
            Err(e) => {
                error!("failed to parse timestamp in callback: {:?}", e);
            }
        }
        self.ready_to_send = true;
    }
    async fn service(&mut self) -> Result<()> {
        // check if we should send frame(s) and send if so.

        // should we send it?
        // TODO cache the converted frame.
        // TODO allow client to throttle?
        // TODO make algorithm smarter to have more in-flight frames?
        // TODO include sent time in message to clients so we don't maintain that

        if let Some(ref most_recent_frame_data) = self.frame_lifo {
            if self.ready_to_send {
                // sent_time computed early so that latency includes duration to encode, etc.
                let sent_time: chrono::DateTime<chrono::Utc> = chrono::Utc::now();
                let tc = {
                    let most_recent_frame_data = most_recent_frame_data.lock();
                    let bytes = basic_frame::match_all_dynamic_fmts!(
                        &most_recent_frame_data.frame,
                        x,
                        convert_image::frame_to_image(x, convert_image::ImageOptions::Jpeg(80),)
                    )?;
                    let firehose_frame_base64 = base64::encode(&bytes);
                    let data_url = format!("data:image/jpeg;base64,{}", firehose_frame_base64);
                    // most_recent_frame_data.data_url = Some(data_url.clone()); // todo: cache like this
                    let found_points = most_recent_frame_data.found_points.clone();
                    ToClient {
                        firehose_frame_data_url: data_url,
                        found_points,
                        valid_display: most_recent_frame_data.valid_display.clone(),
                        annotations: most_recent_frame_data.annotations.clone(),
                        fno: self.fno,
                        ts_rfc3339: sent_time.to_rfc3339(),
                        ck: self.conn_key,
                        name: most_recent_frame_data.name.clone(),
                    }
                };
                let buf = serde_json::to_string(&tc).expect("encode");
                let buf = format!(
                    "event: {}\ndata: {}\n\n",
                    http_video_streaming_types::VIDEO_STREAM_EVENT_NAME,
                    buf
                );
                let hc = buf.into();

                match self.out.send(hc).await {
                    Ok(()) => {}
                    Err(_) => {
                        info!("failed to send data to connection. dropping.");
                        // Failed to send data to event stream key.
                        // TODO: drop this sender.
                    }
                }
                self.ready_to_send = false;
            }
        }

        self.frame_lifo = None;

        Ok(())
    }
}

struct TaskState {
    use_frame_selector: bool,
    events_prefix: String,
    /// cache of senders
    per_sender_map: HashMap<ConnectionKey, PerSender>,
    /// most recent image frame, with annotations
    frame: Arc<Mutex<AnnotatedFrame>>,
}

fn _test_task_state_is_send() {
    // Compile-time test to ensure PerSender implements Send trait.
    fn implements<T: Send>() {}
    implements::<TaskState>();
}

impl TaskState {
    async fn service(&mut self) -> Result<()> {
        // TODO: make sending concurrent on all listeners and set a timeout.
        for ps in self.per_sender_map.values_mut() {
            ps.service().await?;
        }
        Ok(())
    }
    fn handle_connection(&mut self, conn_evt: ConnectionEvent) -> Result<()> {
        let path = conn_evt.path.as_str();
        match conn_evt.typ {
            ConnectionEventType::Connect(chunk_sender) => {
                // sender was added.
                let name_selector = if path == self.events_prefix.as_str() {
                    match self.use_frame_selector {
                        true => NameSelector::None,
                        false => NameSelector::All,
                    }
                } else {
                    if !path.starts_with(self.events_prefix.as_str()) {
                        return Err(Error::UnknownPath(
                            #[cfg(feature = "backtrace")]
                            std::backtrace::Backtrace::capture(),
                        ));
                    }
                    let slash_idx = self.events_prefix.len() + 1; // get location of '/' separator
                    let use_name = path[slash_idx..].to_string();
                    NameSelector::Name(use_name)
                };
                let ps = PerSender::new(
                    chunk_sender,
                    conn_evt.connection_key.clone(),
                    name_selector,
                    self.frame.clone(),
                );
                self.per_sender_map.insert(conn_evt.connection_key, ps);
            }
            ConnectionEventType::Disconnect => {
                self.per_sender_map.remove(&conn_evt.connection_key);
            }
        }
        Ok(())
    }
    fn handle_frame(&mut self, new_frame: AnnotatedFrame) -> Result<()> {
        // Move the frame into a reference-counted pointer.
        self.frame = Arc::new(Mutex::new(new_frame));
        for ps in self.per_sender_map.values_mut() {
            // Clone the pointer and move the pointer into each sender.
            ps.push(self.frame.clone());
        }
        Ok(())
    }
    fn handle_callback(&mut self, callback: FirehoseCallback) -> Result<()> {
        if let Some(ps) = self.per_sender_map.get_mut(&callback.inner.ck) {
            ps.got_callback(callback)
        } else {
            warn!(
                "Got firehose_callback for non-existant connection key. \
                            Did connection disconnect?"
            );
        }
        Ok(())
    }
}

pub async fn firehose_task(
    connection_callback_rx: tokio::sync::mpsc::Receiver<ConnectionEvent>,
    // sender_map_arc: SenderMap,
    mut firehose_rx: tokio::sync::mpsc::Receiver<AnnotatedFrame>,
    firehose_callback_rx: tokio::sync::mpsc::Receiver<FirehoseCallback>,
    use_frame_selector: bool,
    events_prefix: &str,
    mut quit_rx: tokio::sync::oneshot::Receiver<()>,
) -> Result<()> {
    // Wait for the first frame so we don't need to deal with an Option<>.
    let frame = Arc::new(Mutex::new(firehose_rx.recv().await.unwrap()));

    let mut task_state = TaskState {
        events_prefix: events_prefix.to_string(),
        use_frame_selector,
        per_sender_map: HashMap::new(),
        frame,
    };

    let mut connection_callback_rx =
        tokio_stream::wrappers::ReceiverStream::new(connection_callback_rx);
    let mut firehose_rx = tokio_stream::wrappers::ReceiverStream::new(firehose_rx);
    let mut firehose_callback_rx =
        tokio_stream::wrappers::ReceiverStream::new(firehose_callback_rx);
    loop {
        tokio::select! {
            _quit_val = &mut quit_rx => {
                log::debug!("quitting.");
                break;
            }
            opt_new_connection = connection_callback_rx.next() => {
                match opt_new_connection {
                    Some(new_connection) => {
                        task_state.handle_connection(new_connection)?;
                    }
                    None => {
                        log::debug!("new connection senders done.");
                        // All senders done.
                        break;
                    }
                }
            }
            opt_new_frame = firehose_rx.next() => {
                match opt_new_frame {
                    Some(new_frame) => {
                        task_state.handle_frame(new_frame)?;
                    }
                    None => {
                        log::debug!("new frame senders done.");
                        // All senders done.
                        break;
                    }
                }
            },
            opt_callback = firehose_callback_rx.next() => {
                match opt_callback {
                    Some(callback) => {
                        task_state.handle_callback(callback)?;
                    }
                    None => {
                        log::debug!("new callback senders done.");
                        // All senders done.
                        break;
                    }
                }
            },
        }
        task_state.service().await?;
    }
    log::debug!("firehose task done.");
    Ok(())
}
