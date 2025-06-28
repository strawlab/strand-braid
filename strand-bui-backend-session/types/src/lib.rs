//! Types for [Strand Camera](https://strawlab.org/strand-cam) BUI (Browser User
//! Interface) backend session management.
//!
//! This crate provides the core data types used for communication between a web
//! browser frontend and a Rust backend application through session-based
//! messaging. It defines the protocol for bidirectional communication using
//! JSON-serialized messages.

// Copyright 2016-2025 Andrew D. Straw.
//
// Licensed under the Apache License, Version 2.0
// <http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

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
    /// The address from which the connection was made.
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
