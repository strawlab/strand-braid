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
use std::net::SocketAddr;

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
    pub addr: SocketAddr,
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

/// Error type for URL parsing failures
#[derive(Debug)]
pub struct UrlParseError;

impl std::fmt::Display for UrlParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Failed to parse URL")
    }
}

impl std::error::Error for UrlParseError {}

/// HTTP API server access information
///
/// This contains the address and access token.
///
/// This is used for both the Strand Camera BUI and the Braid BUI and could be
/// used for other servers.
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct BuiServerAddrInfo {
    /// The listen address of the HTTP server.
    ///
    /// Note that this can be unspecified (i.e. `0.0.0.0` for IPv4).
    addr: SocketAddr,
    /// The token for initial connection to the HTTP server.
    token: AccessToken,
}

impl BuiServerAddrInfo {
    /// Create a new BuiServerAddrInfo with the given address and token.
    pub fn new(addr: SocketAddr, token: AccessToken) -> Self {
        Self { addr, token }
    }

    /// Get the address of the HTTP server.
    pub fn addr(&self) -> &SocketAddr {
        &self.addr
    }

    /// Get the token for the HTTP server.
    pub fn token(&self) -> &AccessToken {
        &self.token
    }

    /// Parse a URL string into a BuiServerAddrInfo.
    pub fn parse_url_with_token(url: &str) -> Result<Self, UrlParseError> {
        // TODO: replace this ugly implementation...
        let stripped = url.strip_prefix("http://").ok_or(UrlParseError)?;
        let first_slash = stripped.find('/');
        let (addr_str, token) = if let Some(slash_idx) = first_slash {
            let path = &stripped[slash_idx..];
            if path == "/" || path == "/?" {
                (&stripped[..slash_idx], AccessToken::NoToken)
            } else {
                let token_str = path[1..].strip_prefix("?token=").ok_or(UrlParseError {})?;
                (
                    &stripped[..slash_idx],
                    AccessToken::PreSharedToken(token_str.to_string()),
                )
            }
        } else {
            (stripped, AccessToken::NoToken)
        };
        let addr = std::net::ToSocketAddrs::to_socket_addrs(addr_str)
            .map_err(|_io_err| UrlParseError)?
            .next()
            .ok_or(UrlParseError)?;
        if addr.ip().is_unspecified() {
            // An unspecified IP (e.g. 0.0.0.0) is not a valid remotely visible
            // address.
            return Err(UrlParseError);
        }
        Ok(Self::new(addr, token))
    }

    /// Build a base URL for the HTTP server.
    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }
}
