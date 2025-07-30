//! Backend session management for the BUI (Browser User Interface) used by [Strand
//! Camera](https://strawlab.org/strand-cam) and
//! [Braid](https://strawlab.org/braid).
//!
//! This crate provides HTTP session management functionality for web-based user
//! interfaces, including cookie handling, authentication tokens, and request/response
//! processing. It's designed to work with browser-based frontends that communicate
//! with Rust backend services.
//!
//! # Features
//!
//! - HTTP session management with automatic cookie handling
//! - Support for pre-shared authentication tokens
//! - Async/await support for all HTTP operations
//! - Integration with the Strand Camera and Braid ecosystems
//!
//! # Examples
//!
//! ```rust,no_run
//! use strand_bui_backend_session::{HttpSession, create_session};
//! use std::sync::{Arc, RwLock};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create a cookie store for session management
//! let jar = Arc::new(RwLock::new(cookie_store::CookieStore::new(None)));
//!
//! // Server information with authentication token
//! let server_info = strand_bui_backend_session_types::BuiServerAddrInfo::new(
//!     "127.0.0.1:8080".parse()?,
//!     strand_bui_backend_session_types::AccessToken::NoToken
//! );
//!
//! // Create an authenticated session
//! let mut session = create_session(&server_info, jar).await?;
//!
//! // Make requests using the session
//! let response = session.get("api/status").await?;
//! # Ok(())
//! # }
//! ```

// Copyright 2016-2025 Andrew D. Straw.
//
// Licensed under the Apache License, Version 2.0
// <http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

use http::{header::ACCEPT, HeaderValue};
use std::{
    net::SocketAddr,
    sync::{Arc, RwLock},
};
use strand_bui_backend_session_types::{AccessToken, BuiServerAddrInfo};
use thiserror::Error;

const SET_COOKIE: &str = "set-cookie";
const COOKIE: &str = "cookie";

/// Type alias for the HTTP body type used throughout the crate.
///
/// This uses Axum's body type for compatibility with the web framework.
pub type MyBody = axum::body::Body;

/// Errors that can occur during HTTP session operations.
#[derive(Error, Debug)]
pub enum Error {
    /// A wrapped error from the hyper HTTP client crate.
    #[error("hyper error `{0}`")]
    Hyper(#[from] hyper::Error),
    /// A wrapped error from the hyper-util HTTP utilities crate.
    #[error("hyper-util error `{0}`")]
    HyperUtil(#[from] hyper_util::client::legacy::Error),
    /// The HTTP request was not successful.
    ///
    /// This error occurs when the server returns a non-success status code
    /// (anything other than 2xx).
    #[error("request not successful. status code: `{0}`")]
    RequestFailed(http::StatusCode),
}

/// An HTTP session for communicating with a single server.
///
/// This struct manages cookies, authentication, and provides methods for making
/// HTTP requests to a specific server. All requests made through this session
/// will automatically include appropriate cookies and authentication tokens.
#[derive(Clone, Debug)]
pub struct HttpSession {
    /// The base URI for all requests made by this session
    base_uri: hyper::Uri,
    /// Thread-safe cookie store for managing session cookies
    jar: Arc<RwLock<cookie_store::CookieStore>>,
}

/// Creates an authenticated `HttpSession` by making an initial request with the provided token.
///
/// This function establishes a session with the server by making an initial authenticated
/// request, which typically results in the server setting session cookies that will be
/// used for subsequent requests.
///
/// # Arguments
///
/// * `server_info` - Server address and authentication information
/// * `jar` - Thread-safe cookie store for managing session cookies
///
/// # Returns
///
/// An authenticated `HttpSession` ready for making requests, or an error if the
/// initial authentication request fails.
///
/// # Examples
///
/// ```rust,no_run
/// use strand_bui_backend_session::create_session;
/// use strand_bui_backend_session_types::AccessToken;
/// use std::sync::{Arc, RwLock};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let jar = Arc::new(RwLock::new(cookie_store::CookieStore::new(None)));
/// let server_info = strand_bui_backend_session_types::BuiServerAddrInfo::new(
///     "127.0.0.1:8080".parse()?,
///     AccessToken::PreSharedToken("secret123".to_string())
/// );
///
/// let session = create_session(&server_info, jar).await?;
/// # Ok(())
/// # }
/// ```
#[tracing::instrument(level = "debug", skip(server_info, jar))]
pub async fn create_session(
    server_info: &strand_bui_backend_session_types::BuiServerAddrInfo,
    jar: Arc<RwLock<cookie_store::CookieStore>>,
) -> Result<HttpSession, Error> {
    let base_uri = format!("http://{}/", server_info.addr());
    let mut base = HttpSession::new(&base_uri, jar);
    base.get_with_token("", server_info.token()).await?;
    Ok(base)
}

impl HttpSession {
    /// Creates a new HTTP session for the specified base URI.
    ///
    /// # Arguments
    ///
    /// * `base_uri` - The base URI for all requests (must end with "/")
    /// * `jar` - Thread-safe cookie store for managing session cookies
    ///
    /// # Panics
    ///
    /// Panics if the base URI cannot be parsed or doesn't end with "/".
    fn new(base_uri: &str, jar: Arc<RwLock<cookie_store::CookieStore>>) -> Self {
        let base_uri: hyper::Uri = base_uri.parse().expect("failed to parse uri");
        if let Some(pq) = base_uri.path_and_query() {
            assert_eq!(pq.path(), "/");
            assert!(pq.query().is_none());
        }
        Self { base_uri, jar }
    }

    /// Constructs a full URI from a relative path and optional access token.
    ///
    /// # Arguments
    ///
    /// * `rel` - Relative path to append to the base URI
    /// * `token` - Optional access token to include as a query parameter
    ///
    /// # Returns
    ///
    /// A complete URI ready for making HTTP requests.
    fn get_rel_uri(&self, rel: &str, token: Option<&AccessToken>) -> hyper::Uri {
        let token = if let Some(tok1) = token {
            match tok1 {
                AccessToken::NoToken => None,
                AccessToken::PreSharedToken(t) => Some(t),
            }
        } else {
            None
        };

        let pq: String = if let Some(token) = token {
            format!("/{rel}?token={token}")
        } else {
            format!("/{rel}")
        };
        let pqs: &str = &pq;

        let pq: http::uri::PathAndQuery = std::convert::TryFrom::try_from(pqs).unwrap();

        http::uri::Builder::new()
            .scheme(self.base_uri.scheme().unwrap().clone())
            .authority(self.base_uri.authority().unwrap().clone())
            .path_and_query(pq)
            .build()
            .expect("build url")
    }

    /// Internal method for making HTTP requests with full control over parameters.
    ///
    /// # Arguments
    ///
    /// * `rel` - Relative path for the request
    /// * `token` - Optional access token
    /// * `accepts` - Array of Accept header values
    /// * `method` - HTTP method to use
    /// * `body` - Request body
    async fn inner_req(
        &mut self,
        rel: &str,
        token: Option<&AccessToken>,
        accepts: &[HeaderValue],
        method: http::Method,
        body: axum::body::Body,
    ) -> Result<hyper::Response<hyper::body::Incoming>, Error> {
        let uri = self.get_rel_uri(rel, token);

        let mut req = hyper::Request::new(body);
        *req.method_mut() = method;
        *req.uri_mut() = uri;
        for accept in accepts.iter() {
            req.headers_mut().insert(ACCEPT, (*accept).clone());
        }
        let response = self.make_request(req).await?;
        Ok(response)
    }

    /// Makes a GET request to the specified relative path.
    ///
    /// This method automatically includes session cookies and handles authentication.
    ///
    /// # Arguments
    ///
    /// * `rel` - Relative path to request (e.g., "api/status")
    ///
    /// # Returns
    ///
    /// The HTTP response from the server, or an error if the request fails.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # async fn example(mut session: strand_bui_backend_session::HttpSession) -> Result<(), Box<dyn std::error::Error>> {
    /// let response = session.get("api/status").await?;
    /// println!("Status: {}", response.status());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get(
        &mut self,
        rel: &str,
    ) -> Result<hyper::Response<hyper::body::Incoming>, Error> {
        self.inner_req(rel, None, &[], http::Method::GET, axum::body::Body::empty())
            .await
    }

    /// Makes an HTTP request with custom Accept headers and method.
    ///
    /// This method provides more control over the HTTP request, allowing you to
    /// specify Accept headers and HTTP method.
    ///
    /// # Arguments
    ///
    /// * `rel` - Relative path to request
    /// * `accepts` - Array of Accept header values to include
    /// * `method` - HTTP method to use (GET, POST, PUT, etc.)
    /// * `body` - Request body
    ///
    /// # Returns
    ///
    /// The HTTP response from the server, or an error if the request fails.
    pub async fn req_accepts(
        &mut self,
        rel: &str,
        accepts: &[HeaderValue],
        method: http::Method,
        body: axum::body::Body,
    ) -> Result<hyper::Response<hyper::body::Incoming>, Error> {
        self.inner_req(rel, None, accepts, method, body).await
    }

    /// Makes a GET request with an authentication token.
    ///
    /// This method is used internally for authenticated requests, typically
    /// during session establishment.
    ///
    /// # Arguments
    ///
    /// * `rel` - Relative path to request
    /// * `token` - Access token for authentication
    async fn get_with_token(
        &mut self,
        rel: &str,
        token: &AccessToken,
    ) -> Result<hyper::Response<hyper::body::Incoming>, Error> {
        self.inner_req(
            rel,
            Some(token),
            &[],
            http::Method::GET,
            axum::body::Body::empty(),
        )
        .await
    }

    /// Makes a POST request to the specified relative path.
    ///
    /// This method automatically includes session cookies and sets the
    /// Content-Type header to "application/json".
    ///
    /// # Arguments
    ///
    /// * `rel` - Relative path to post to (e.g., "api/submit")
    /// * `body` - Request body containing the data to post
    ///
    /// # Returns
    ///
    /// The HTTP response from the server, or an error if the request fails.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # async fn example(mut session: strand_bui_backend_session::HttpSession) -> Result<(), Box<dyn std::error::Error>> {
    /// let body = axum::body::Body::from(r#"{"key": "value"}"#);
    /// let response = session.post("api/data", body).await?;
    /// # Ok(())
    /// # }
    /// ```
    #[tracing::instrument(skip_all)]
    pub async fn post(
        &mut self,
        rel: &str,
        body: MyBody,
    ) -> Result<hyper::Response<hyper::body::Incoming>, Error> {
        let uri = self.get_rel_uri(rel, None);

        let mut req = hyper::Request::new(body);
        *req.method_mut() = hyper::Method::POST;
        *req.uri_mut() = uri;
        self.make_request(req).await
    }

    /// Internal method that actually executes HTTP requests.
    ///
    /// This method handles cookie management, sets appropriate headers,
    /// and processes the response including cookie updates.
    ///
    /// # Arguments
    ///
    /// * `req` - The complete HTTP request to execute
    ///
    /// # Returns
    ///
    /// The HTTP response, or an error if the request fails or returns
    /// a non-success status code.
    #[tracing::instrument(skip_all)]
    async fn make_request(
        &mut self,
        mut req: hyper::Request<MyBody>,
    ) -> Result<hyper::Response<hyper::body::Incoming>, Error> {
        let client =
            hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
                .build_http();

        tracing::trace!("building request");
        let url = url::Url::parse(req.uri().to_string().as_ref()).unwrap();
        {
            let jar = self.jar.read().unwrap();
            for (cookie_name, cookie_value) in jar.get_request_values(&url) {
                let cookie = cookie_store::RawCookie::new(cookie_name, cookie_value);
                tracing::trace!("adding cookie {}", cookie);
                req.headers_mut().insert(
                    COOKIE,
                    hyper::header::HeaderValue::from_str(&cookie.to_string()).unwrap(),
                );
            }
        }

        req.headers_mut().insert(
            http::header::CONTENT_TYPE,
            hyper::header::HeaderValue::from_str("application/json").unwrap(),
        );

        tracing::debug!("making request {:?}", req);
        let response = client.request(req).await.map_err(|e| {
            tracing::error!("encountered error {e}: {e:?}");
            Error::from(e)
        })?;

        tracing::debug!("handling response {:?}", response);
        let response = handle_response(&url, self.jar.clone(), response)?;
        let status_code = response.status();
        if !status_code.is_success() {
            use http_body_util::BodyExt;
            let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
            let body_str = std::string::String::from_utf8_lossy(body_bytes.as_ref());
            tracing::error!("response {status_code:?}: \"{body_str}\"");
            return Err(Error::RequestFailed(status_code));
        }
        Ok(response)
    }
}

/// Processes HTTP response headers to extract and store cookies.
///
/// This function examines the response for Set-Cookie headers and updates
/// the cookie store accordingly. It's called automatically by the session
/// to maintain cookie state across requests.
///
/// # Arguments
///
/// * `url` - The URL that generated this response (for cookie domain matching)
/// * `jar` - Thread-safe cookie store to update
/// * `response` - The HTTP response to process
///
/// # Returns
///
/// The same HTTP response, or an error if cookie processing fails.
fn handle_response(
    url: &url::Url,
    jar: Arc<RwLock<cookie_store::CookieStore>>,
    mut response: hyper::Response<hyper::body::Incoming>,
) -> Result<hyper::Response<hyper::body::Incoming>, Error> {
    tracing::trace!("starting to handle cookies in response {:?}", response);

    use hyper::header::Entry::*;
    match response.headers_mut().entry(SET_COOKIE) {
        Occupied(e) => {
            let (_key, drain) = e.remove_entry_mult();
            let mut jar = jar.write().unwrap();
            jar.store_response_cookies(
                drain.map(|cookie_raw| {
                    cookie_store::RawCookie::parse(cookie_raw.to_str().unwrap().to_string())
                        .unwrap()
                }),
                url,
            );
        }
        Vacant(_) => {}
    }

    tracing::trace!("done handling cookies in response {:?}", response);
    Ok(response)
}

#[test]
fn test_serialized_cookie_store() {
    // Test that we can upgrade `cookie_store` crate without invalidating on-disk stored cookies.

    // Load CookieStore from json like we have previously saved to disk.
    let serialized_json = r#"[{"raw_cookie":"abc=def; Expires=Thu, 20 Nov 2025 12:19:29 GMT","path":["/",false],"domain":{"HostOnly":"127.0.0.1"},"expires":{"AtUtc":"2025-11-20T12:19:29Z"}}]"#;
    let loaded: cookie_store::CookieStore = serde_json::from_str(serialized_json).unwrap();

    // What do we expect?
    let expected = {
        let mut expected = cookie_store::CookieStore::new(None);
        let cookie_str = "abc=def; Expires=Thu, 20 Nov 2025 12:19:29 GMT";
        let request_url = "http://127.0.0.1/".try_into().unwrap();

        let cookie = cookie_store::Cookie::parse(cookie_str, &request_url).unwrap();
        expected.insert(cookie, &request_url).unwrap();
        expected
    };

    // Since CookieStore does not implement PartialEq, we convert to json values
    // and compare those.
    let loaded_json: serde_json::Value = serde_json::to_value(&loaded).unwrap();
    let expected_json: serde_json::Value = serde_json::to_value(&expected).unwrap();
    assert_eq!(&loaded_json, &expected_json);
}

/// Builds a list of HTTP URIs for the server address.
pub fn build_urls(bui_server_info: &BuiServerAddrInfo) -> std::io::Result<Vec<http::Uri>> {
    let query = match &bui_server_info.token() {
        AccessToken::NoToken => "".to_string(),
        AccessToken::PreSharedToken(tok) => format!("?token={tok}"),
    };
    Ok(expand_unspecified_addr(bui_server_info.addr())?
        .into_iter()
        .map(|specified_addr| {
            let addr = specified_addr.addr();
            http::uri::Builder::new()
                .scheme("http")
                .authority(format!("{}:{}", addr.ip(), addr.port()))
                .path_and_query(format!("/{query}"))
                .build()
                .unwrap()
        })
        .collect())
}

/// A newtype wrapping a [SocketAddr] which ensures that it is specified.
#[derive(Debug, PartialEq, Clone, serde::Serialize)]
#[serde(transparent)]
pub struct SpecifiedSocketAddr(SocketAddr);

impl std::fmt::Display for SpecifiedSocketAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        std::fmt::Display::fmt(&self.0, f)
    }
}

impl SpecifiedSocketAddr {
    fn make_err() -> std::io::Error {
        std::io::ErrorKind::AddrNotAvailable.into()
    }
    /// Creates a new `SpecifiedSocketAddr` from a `SocketAddr`.
    pub fn new(addr: SocketAddr) -> std::io::Result<Self> {
        if addr.ip().is_unspecified() {
            return Err(Self::make_err());
        }
        Ok(Self(addr))
    }
    /// Get the underlying IP address of the socket.
    pub fn ip(&self) -> std::net::IpAddr {
        self.0.ip()
    }
    /// Get the underlying socket address.
    pub fn addr(&self) -> &std::net::SocketAddr {
        &self.0
    }
}

impl<'de> serde::Deserialize<'de> for SpecifiedSocketAddr {
    fn deserialize<D>(deserializer: D) -> std::result::Result<SpecifiedSocketAddr, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let addr: SocketAddr = std::net::SocketAddr::deserialize(deserializer)?;
        SpecifiedSocketAddr::new(addr).map_err(|_e| serde::de::Error::custom(Self::make_err()))
    }
}

/// Expands an unspecified address into a list of specified addresses.
fn expand_unspecified_addr(addr: &SocketAddr) -> std::io::Result<Vec<SpecifiedSocketAddr>> {
    if addr.ip().is_unspecified() {
        expand_unspecified_ip(addr.ip())?
            .into_iter()
            .map(|ip| SpecifiedSocketAddr::new(SocketAddr::new(ip, addr.port())))
            .collect()
    } else {
        Ok(vec![SpecifiedSocketAddr::new(*addr).unwrap()])
    }
}

fn expand_unspecified_ip(ip: std::net::IpAddr) -> std::io::Result<Vec<std::net::IpAddr>> {
    if ip.is_unspecified() {
        // Get all interfaces if IP is unspecified.
        Ok(if_addrs::get_if_addrs()?
            .iter()
            .filter_map(|x| {
                let this_ip = x.addr.ip();
                // Take only IP addresses from correct family.
                if ip.is_ipv4() == this_ip.is_ipv4() {
                    Some(this_ip)
                } else {
                    None
                }
            })
            .collect())
    } else {
        Ok(vec![ip])
    }
}
