use bytes::Bytes;
use futures::StreamExt;
use http::{header::ACCEPT, request::Parts, StatusCode};
use http_body::Frame;
use std::{
    collections::HashMap,
    convert::Infallible,
    pin::Pin,
    sync::{Arc, RwLock},
};
use strand_bui_backend_session_types::ConnectionKey;
use tokio::sync::mpsc::Sender;
use tokio_stream::wrappers::ReceiverStream;

pub type EventChunkSender = Sender<Result<Frame<Bytes>, Infallible>>;
type EventReceiver = ReceiverStream<Result<Frame<Bytes>, Infallible>>;

/// The type of possible connect event, either connect or disconnect.
#[derive(Debug)]
pub enum ConnectionEventType {
    /// A connection event with sink for event stream messages to the connected client.
    Connect(EventChunkSender),
    /// A disconnection event.
    Disconnect,
}

/// State associated with connection or disconnection.
#[derive(Debug)]
pub struct ConnectionEvent {
    /// The type of connection for this event.
    pub typ: ConnectionEventType,
    /// Identifier for the connection (one per tab).
    pub connection_key: ConnectionKey,
}

// header extractor for "Accept: text/event-stream" --------------------------

pub struct AcceptsEventStream;

impl<S> axum::extract::FromRequestParts<S> for AcceptsEventStream
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, &'static str);
    async fn from_request_parts(p: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        const ES: &[u8] = b"text/event-stream";
        if p.headers.get_all(ACCEPT).iter().any(|v| v.as_bytes() == ES) {
            Ok(AcceptsEventStream)
        } else {
            Err((
                StatusCode::BAD_REQUEST,
                "Bad request: It is required that you have an \
                HTTP Header \"Accept: text/event-stream\"",
            ))
        }
    }
}

// TolerantJson extractor --------------------------

/// This is much like `axum::Json` but does not fail if the request does not set
/// the 'Content-Type' header.
///
/// This is purely for backwards-compatibility and can be removed sometime.
pub struct TolerantJson<T>(pub T);

impl<T, S> axum::extract::FromRequest<S> for TolerantJson<T>
where
    T: serde::de::DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = axum::extract::rejection::JsonRejection;

    async fn from_request(
        mut req: axum::extract::Request,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        if !json_content_type(req.headers()) {
            tracing::error!("request should indicate \"Content-Type: application/json\"");
            req.headers_mut().insert(
                http::header::CONTENT_TYPE,
                http::HeaderValue::from_static("application/json"),
            );
        }
        match axum::Json::from_request(req, state).await {
            Ok(payload) => Ok(TolerantJson(payload.0)),
            Err(e) => Err(e),
        }
    }
}

// events body ---------------------------

pub struct EventsBody {
    events: EventReceiver,
}

impl EventsBody {
    fn new(events: EventReceiver) -> Self {
        Self { events }
    }
}

impl http_body::Body for EventsBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        self.events.poll_next_unpin(cx)
    }
}

impl axum::response::IntoResponse for EventsBody {
    fn into_response(self) -> axum::response::Response {
        let mut response = axum::response::Response::new(axum::body::Body::new(self));
        response.headers_mut().insert(
            "content-type",
            http::header::HeaderValue::from_static("text/event-stream"),
        );
        response
    }
}

// -----

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct ConnectionSessionKey {
    session_key: uuid::Uuid,
    connection_key: std::net::SocketAddr,
}

impl ConnectionSessionKey {
    pub fn new(session_key: uuid::Uuid, connection_key: std::net::SocketAddr) -> Self {
        Self {
            session_key,
            connection_key,
        }
    }
}

/// broadcasts events to many listeners.
///
/// This is generic over the key type.
#[derive(Debug, Clone)]
pub struct EventBroadcaster<KEY> {
    txers: Arc<RwLock<HashMap<KEY, EventChunkSender>>>,
}

impl<KEY> Default for EventBroadcaster<KEY> {
    fn default() -> Self {
        Self {
            txers: Default::default(),
        }
    }
}

impl<KEY> EventBroadcaster<KEY>
where
    KEY: std::cmp::Eq + std::hash::Hash,
{
    /// Add a new connection indexed by a key.
    ///
    /// This returns an [EventsBody].
    pub fn new_connection(&self, key: KEY) -> (EventChunkSender, EventsBody) {
        let (tx, rx) = tokio::sync::mpsc::channel(10);
        let mut txers = self.txers.write().unwrap();
        txers.insert(key, tx.clone());
        let rx = tokio_stream::wrappers::ReceiverStream::new(rx);
        let body = EventsBody::new(rx);

        (tx, body)
    }
    /// Transmit bytes as frame
    ///
    /// This will drop connections which have errored.
    pub async fn broadcast_frame(&self, frame_string: String) {
        let txers: Vec<_> = {
            // Keep lock in this scope.
            // Move all listeners out of shared map.
            self.txers.write().unwrap().drain().collect()
        };

        // now we have released the lock and can await without holding the lock.
        let mut keep_event_listeners = Vec::with_capacity(txers.len());
        for (key, tx) in txers.into_iter() {
            match tx.send(Ok(Frame::data(frame_string.clone().into()))).await {
                Ok(()) => {
                    keep_event_listeners.push((key, tx));
                }
                Err(tokio::sync::mpsc::error::SendError(_frame)) => {
                    // The receiver was dropped because the connection closed.
                    tracing::debug!("send error");
                }
            }
        }

        {
            // Keep lock in this scope.
            // Move all listeners back into shared map.
            let mut event_listeners = self.txers.write().unwrap();
            for (key, value) in keep_event_listeners.into_iter() {
                event_listeners.insert(key, value);
            }
        };
    }
}

// ----

// This does not really belong here...

fn json_content_type(headers: &http::HeaderMap) -> bool {
    let content_type = if let Some(content_type) = headers.get(http::header::CONTENT_TYPE) {
        content_type
    } else {
        return false;
    };

    let content_type = if let Ok(content_type) = content_type.to_str() {
        content_type
    } else {
        return false;
    };

    let mime = if let Ok(mime) = content_type.parse::<mime::Mime>() {
        mime
    } else {
        return false;
    };

    let is_json_content_type = mime.type_() == "application"
        && (mime.subtype() == "json" || mime.suffix().is_some_and(|name| name == "json"));

    is_json_content_type
}
