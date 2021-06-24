#![cfg_attr(feature = "backtrace", feature(backtrace))]

#[macro_use]
extern crate log;

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use futures::sink::SinkExt;
use parking_lot::RwLock;

use bui_backend::lowlevel::EventChunkSender;
use bui_backend_types::{ConnectionKey, SessionKey};

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
    #[error("receive error: {source}")]
    RecvError {
        #[from]
        source: crossbeam_channel::RecvError,
        #[cfg(feature = "backtrace")]
        backtrace: std::backtrace::Backtrace,
    },
    #[error("callback sender disconnected")]
    CallbackSenderDisconnected(#[cfg(feature = "backtrace")] std::backtrace::Backtrace),
}

// future: use MediaSource API? https://w3c.github.io/media-source

pub struct AnnotatedFrame {
    pub frame: DynamicFrame,
    pub found_points: Vec<Point>,
    pub valid_display: Option<Shape>,
    pub name: Option<String>,
    pub annotations: Vec<DrawableShape>,
}

pub struct FirehoseCallback {
    pub arrival_time: chrono::DateTime<chrono::Utc>,
    pub inner: FirehoseCallbackInner,
}

struct PerSender {
    name_selector: NameSelector,
    out: EventChunkSender,
    frame_lifo: Option<Rc<AnnotatedFrame>>,
    ready_to_send: bool,
    conn_key: ConnectionKey,
    fno: u64,
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
    ) -> PerSender {
        PerSender {
            name_selector,
            out,
            frame_lifo: None,
            ready_to_send: true,
            conn_key: conn_key,
            fno: 0,
        }
    }
    fn push(&mut self, frame: Rc<AnnotatedFrame>) {
        let use_frame = match self.name_selector {
            NameSelector::All => true,
            NameSelector::None => false,
            NameSelector::Name(ref select_name) => {
                let mut tmp = false;
                if let Some(ref this_name) = frame.name {
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
    fn service(&mut self) -> Result<()> {
        // check if we should send frame(s) and send if so.

        // should we send it?
        // TODO allow client to throttle?
        // TODO make algorithm smarter to have more in-flight frames?
        // TODO include sent time in message to clients so we don't maintain that

        match self.frame_lifo {
            Some(ref most_recent_frame_data) => {
                if self.ready_to_send {
                    // sent_time computed early so that latency includes duration to encode, etc.
                    let sent_time: chrono::DateTime<chrono::Utc> = chrono::Utc::now();
                    let bytes = basic_frame::match_all_dynamic_fmts!(
                        &most_recent_frame_data.frame,
                        x,
                        convert_image::frame_to_image(x, convert_image::ImageOptions::Jpeg(80),)
                    )?;
                    let firehose_frame_base64 = base64::encode(&bytes);
                    let data_url = format!("data:image/jpeg;base64,{}", firehose_frame_base64);
                    let found_points = most_recent_frame_data.found_points.clone();
                    let tc = ToClient {
                        firehose_frame_data_url: data_url,
                        found_points,
                        valid_display: most_recent_frame_data.valid_display.clone(),
                        annotations: most_recent_frame_data.annotations.clone(),
                        fno: self.fno,
                        ts_rfc3339: sent_time.to_rfc3339(),
                        ck: self.conn_key,
                        name: most_recent_frame_data.name.clone(),
                    };

                    let buf = serde_json::to_string(&tc).expect("encode");
                    let buf = format!(
                        "event: {}\ndata: {}\n\n",
                        http_video_streaming_types::VIDEO_STREAM_EVENT_NAME,
                        buf
                    );
                    let hc = buf.clone().into();

                    match futures::executor::block_on(self.out.send(hc)) {
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
            None => {} // nothing to do, no frame in queue
        }

        self.frame_lifo = None;

        Ok(())
    }
}

pub fn firehose_thread(
    sender_map_arc: Arc<RwLock<HashMap<ConnectionKey, (SessionKey, EventChunkSender, String)>>>,
    firehose_rx: crossbeam_channel::Receiver<AnnotatedFrame>,
    firehose_callback_rx: crossbeam_channel::Receiver<FirehoseCallback>,
    use_frame_selector: bool,
    events_prefix: &str,
    flag: thread_control::Flag,
) -> Result<()> {
    // TODO switch this to a tokio core reactor based event loop and async processing.
    let mut per_sender_map: HashMap<ConnectionKey, PerSender> = HashMap::new();
    let zero_dur = std::time::Duration::from_millis(0);
    while flag.is_alive() {
        // We have a timeout here in order to poll the `flag` variable above.
        let mut msg = match firehose_rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(msg) => msg,
            Err(e) => {
                if e.is_timeout() {
                    continue;
                } else {
                    debug!("firehose_thread: sender disconnected");
                    break;
                }
            }
        };

        // Now pump the queue for any remaining messages, but do not wait for them.
        while let Ok(msg_last) = firehose_rx.recv_timeout(zero_dur) {
            msg = msg_last;
        }
        let frame = Rc::new(msg);

        // iterate through all senders
        let previous_senders: HashSet<ConnectionKey> = per_sender_map.keys().cloned().collect();
        {
            // in this scope we hold the Arc lock
            let sender_map = sender_map_arc.read();
            let current_senders: HashSet<ConnectionKey> = sender_map.keys().cloned().collect();

            // Note: the size of this Arc lock scope could be reduced
            // by doing the bare minimum within the scope.
            for conn_key in previous_senders.symmetric_difference(&current_senders) {
                match sender_map.get(conn_key) {
                    Some(item) => {
                        // sender was added.
                        let ref path = item.2;
                        let name_selector = if path == events_prefix {
                            match use_frame_selector {
                                true => NameSelector::None,
                                false => NameSelector::All,
                            }
                        } else {
                            if !path.starts_with(events_prefix) {
                                return Err(Error::UnknownPath(
                                    #[cfg(feature = "backtrace")]
                                    std::backtrace::Backtrace::capture(),
                                ));
                            }
                            let slash_idx = events_prefix.len() + 1; // get location of '/' separator
                            let use_name = path[slash_idx..].to_string();
                            NameSelector::Name(use_name)
                        };
                        let ps = PerSender::new(item.1.clone(), *conn_key, name_selector);
                        per_sender_map.insert(*conn_key, ps);
                    }
                    None => {
                        // sender was removed.
                        per_sender_map.remove(conn_key);
                    }
                };
            }
        }

        for ps in per_sender_map.values_mut() {
            ps.push(frame.clone());
        }

        loop {
            match firehose_callback_rx.try_recv() {
                Ok(msg) => {
                    match per_sender_map.get_mut(&msg.inner.ck) {
                        Some(ps) => ps.got_callback(msg),
                        None => {
                            warn!(
                                "Got firehose_callback for non-existant connection key. \
                            Did connection disconnect?"
                            );
                        }
                    };
                }
                Err(crossbeam_channel::TryRecvError::Empty) => break,
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    return Err(Error::CallbackSenderDisconnected(
                        #[cfg(feature = "backtrace")]
                        std::backtrace::Backtrace::capture(),
                    ));
                }
            };
        }

        for ps in per_sender_map.values_mut() {
            ps.service()?;
        }
    }
    Ok(())
}
