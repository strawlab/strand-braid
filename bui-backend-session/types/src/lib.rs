use serde::{Deserialize, Serialize};

/// Identifier for each session (one per client browser).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct SessionKey(pub uuid::Uuid);

#[cfg(feature = "uuid-v4")]
impl SessionKey {
    /// Create a new SessionKey
    #[cfg_attr(docsrs, doc(cfg(feature = "uuid-v4")))]
    pub fn new() -> Self {
        SessionKey(uuid::Uuid::new_v4())
    }
}

/// Identifier for each connected event stream listener (one per client tab).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct ConnectionKey {
    pub addr: std::net::SocketAddr,
}

/// A token which can be required to gain access to HTTP API
///
/// If the server receives a valid token, it will respond with a cookie carrying
/// a session key.
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub enum AccessToken {
    /// No token needed (access must be controlled via other means).
    NoToken,
    /// A pre-shared token to gain access.
    PreSharedToken(String),
}
