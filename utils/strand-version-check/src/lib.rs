// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Client for the strand-braid version-check service.
//!
//! Both Strand Camera and Braid periodically ask
//! `https://version-check.strawlab.org/<product>` whether a newer release is
//! available. This crate centralizes that network call so each application only
//! has to decide how to surface the result. The wire format is:
//!
//! ```json
//! { "available": "1.0.0-rc.3", "message": "...", "url": "https://..." }
//! ```
//!
//! All three fields are required; a response missing any of them fails to parse
//! and is treated as "no update available".

use std::time::Duration;

use bytes::Bytes;
use http_body_util::{combinators::BoxBody, BodyExt, Full};
use hyper_rustls::HttpsConnector;
use hyper_util::{
    client::legacy::{connect::HttpConnector, Client},
    rt::TokioExecutor,
};
use serde::Deserialize;
use tracing::warn;

type Body = BoxBody<Bytes, std::convert::Infallible>;

fn empty_body() -> Body {
    Full::new(Bytes::new())
        .map_err(|never| match never {})
        .boxed()
}

/// A newer available version, as reported by the version-check server.
#[derive(Debug, Clone, PartialEq)]
pub struct AvailableVersion {
    /// The newest available version.
    pub version: semver::Version,
    /// Human-readable message from the server.
    pub message: String,
    /// URL with release notes / downloads.
    pub url: String,
}

/// Reusable client for the version-check service.
///
/// Construct once and reuse for every check: it holds a connection pool, so
/// rebuilding it each time would be wasteful.
#[derive(Clone)]
pub struct VersionChecker {
    client: Client<HttpsConnector<HttpConnector>, Body>,
}

impl Default for VersionChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl VersionChecker {
    /// Build a version checker with its own HTTPS client.
    pub fn new() -> Self {
        let https = hyper_rustls::HttpsConnectorBuilder::new()
            .with_webpki_roots()
            .https_only()
            .enable_http1()
            .build();
        let client = Client::builder(TokioExecutor::new()).build::<_, Body>(https);
        Self { client }
    }

    /// Ask the server which version is available for `product` (e.g. `"braid"`
    /// or `"strand-cam"`), identifying ourselves with `user_agent`.
    ///
    /// Returns `Some` only on a successful, parseable response. Network errors,
    /// timeouts, non-200 responses, and parse errors are logged and yield
    /// `None`, so a failing check never disrupts the caller.
    pub async fn fetch(&self, product: &str, user_agent: &str) -> Option<AvailableVersion> {
        #[derive(Debug, Deserialize)]
        struct VersionResponse {
            available: semver::Version,
            message: String,
            url: String,
        }

        let url = format!("https://version-check.strawlab.org/{product}");
        let uri: hyper::Uri = match url.parse() {
            Ok(uri) => uri,
            Err(e) => {
                warn!("invalid version-check URL {url}: {e}");
                return None;
            }
        };

        let req = hyper::Request::builder()
            .uri(&uri)
            .header(hyper::header::USER_AGENT, user_agent)
            .body(empty_body())
            .unwrap();

        // Bound the request so a hung connection cannot wedge the checker.
        let res = match tokio::time::timeout(Duration::from_secs(30), self.client.request(req)).await
        {
            Ok(Ok(res)) => res,
            Ok(Err(e)) => {
                warn!("version check request to {url} failed: {e}");
                return None;
            }
            Err(_elapsed) => {
                warn!("version check request to {url} timed out");
                return None;
            }
        };

        if res.status() != hyper::StatusCode::OK {
            return None;
        }

        let data = match res.into_body().collect().await {
            Ok(collected) => collected.to_bytes(),
            Err(e) => {
                warn!("could not read version response from {url}: {e}");
                return None;
            }
        };

        match serde_json::from_slice::<VersionResponse>(&data) {
            Ok(v) => Some(AvailableVersion {
                version: v.available,
                message: v.message,
                url: v.url,
            }),
            Err(e) => {
                warn!("could not parse version response JSON from {url}: {e}");
                None
            }
        }
    }
}
