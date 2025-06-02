use http_video_streaming_types::StrokeStyle;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use tokio_stream::StreamExt;

use bui_backend_session_types::ConnectionKey;
use event_stream_types::{ConnectionEvent, ConnectionEventType, EventChunkSender};
use strand_dynamic_frame::DynamicFrameOwned;

pub use http_video_streaming_types::{CircleParams, DrawableShape, Point, Shape, ToClient};

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unknown path")]
    UnknownPath(),
    #[error(transparent)]
    ConvertImageError(#[from] convert_image::Error),
}

// future: use MediaSource API? https://w3c.github.io/media-source

#[derive(Debug)]
pub struct AnnotatedFrame {
    pub frame: DynamicFrameOwned,
    pub found_points: Vec<Point>,
    pub valid_display: Option<Shape>,
    pub annotations: Vec<DrawableShape>,
}

fn _test_annotated_frame_is_send() {
    // Compile-time test to ensure AnnotatedFrame implements Send trait.
    fn implements<T: Send>() {}
    implements::<AnnotatedFrame>();
}

struct PerSender {
    out: EventChunkSender,
    frame_lifo: Option<Arc<Mutex<AnnotatedFrame>>>,
    ready_to_send: bool,
    conn_key: ConnectionKey,
    fno: u64,
    green_stroke: StrokeStyle,
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
        frame: Arc<Mutex<AnnotatedFrame>>,
    ) -> PerSender {
        PerSender {
            out,
            frame_lifo: Some(frame),
            ready_to_send: true,
            conn_key,
            fno: 0,
            green_stroke: StrokeStyle::from_rgb(0x7F, 0xFF, 0x7F),
        }
    }
    fn push(&mut self, frame: Arc<Mutex<AnnotatedFrame>>) {
        self.fno += 1;
        self.frame_lifo = Some(frame);
    }
    fn got_callback(&mut self, _msg: ConnectionKey) {
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
                let sent_time = chrono::Local::now();
                let tc = {
                    let most_recent_frame_data = most_recent_frame_data.lock().unwrap();
                    let bytes = &most_recent_frame_data
                        .frame
                        .borrow()
                        .to_encoded_buffer(convert_image::EncoderOptions::Jpeg(80))?;
                    let firehose_frame_base64 = base64::encode(&bytes);
                    let data_url = format!("data:image/jpeg;base64,{}", firehose_frame_base64);
                    // most_recent_frame_data.data_url = Some(data_url.clone()); // todo: cache like this
                    let mut annotations = most_recent_frame_data.annotations.clone();
                    // Convert found points into normal annotations. (This should perhaps be done earlier.)
                    for found_point in most_recent_frame_data.found_points.iter() {
                        let line_width = 5.0;
                        let shape = Shape::Circle(CircleParams {
                            center_x: found_point.x.round() as i16,
                            center_y: found_point.y.round() as i16,
                            radius: 10,
                        });
                        let green_shape = http_video_streaming_types::DrawableShape::from_shape(
                            &shape,
                            &self.green_stroke,
                            line_width,
                        );
                        annotations.push(green_shape);
                    }
                    ToClient {
                        firehose_frame_data_url: data_url,
                        valid_display: most_recent_frame_data.valid_display.clone(),
                        annotations,
                        fno: self.fno,
                        ts_rfc3339: sent_time.to_rfc3339(),
                        ck: self.conn_key,
                    }
                };
                let buf = serde_json::to_string(&tc).expect("encode");
                let buf = format!(
                    "event: {}\ndata: {}\n\n",
                    http_video_streaming_types::VIDEO_STREAM_EVENT_NAME,
                    buf
                );
                let hc = http_body::Frame::data(bytes::Bytes::from(buf));

                match self.out.send(Ok(hc)).await {
                    Ok(()) => {}
                    Err(_) => {
                        tracing::info!("failed to send data to connection. dropping.");
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
        match conn_evt.typ {
            ConnectionEventType::Connect(chunk_sender) => {
                // sender was added.
                let ps = PerSender::new(chunk_sender, conn_evt.connection_key, self.frame.clone());
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
    fn handle_callback(&mut self, ck: ConnectionKey) -> Result<()> {
        if let Some(ps) = self.per_sender_map.get_mut(&ck) {
            ps.got_callback(ck)
        } else {
            tracing::debug!(
                "Got firehose_callback for non-existant connection key. \
                            Did connection disconnect?"
            );
        }
        Ok(())
    }
}

pub async fn firehose_task(
    connection_callback_rx: tokio::sync::mpsc::Receiver<ConnectionEvent>,
    mut firehose_rx: tokio::sync::mpsc::Receiver<AnnotatedFrame>,
    firehose_callback_rx: tokio::sync::mpsc::Receiver<ConnectionKey>,
) -> Result<()> {
    // Wait for the first frame so we don't need to deal with an Option<>.
    let first_frame = firehose_rx.recv().await.unwrap();
    let frame = Arc::new(Mutex::new(first_frame));

    let mut task_state = TaskState {
        per_sender_map: HashMap::new(),
        frame,
    };

    let mut connection_callback_rx =
        tokio_stream::wrappers::ReceiverStream::new(connection_callback_rx);
    let mut firehose_callback_rx =
        tokio_stream::wrappers::ReceiverStream::new(firehose_callback_rx);
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
    loop {
        tokio::select! {
            opt_new_connection = connection_callback_rx.next() => {
                match opt_new_connection {
                    Some(new_connection) => {
                        task_state.handle_connection(new_connection)?;
                    }
                    None => {
                        tracing::debug!("new connection senders done.");
                        // All senders done.
                        break;
                    }
                }
            }
            opt_new_frame = firehose_rx.recv() => {
                match opt_new_frame {
                    Some(new_frame) => {
                        task_state.handle_frame(new_frame)?;
                    }
                    None => {
                        tracing::debug!("new frame senders done.");
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
                        tracing::debug!("new callback senders done.");
                        // All senders done.
                        break;
                    }
                }
            },
            _ = interval.tick() => {
                task_state.service().await?;
            }
        }
    }
    tracing::debug!("firehose task done.");
    Ok(())
}
