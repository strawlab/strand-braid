use bui_backend_session_types::AccessToken;
use http::{header::ACCEPT, HeaderValue};
use std::sync::{Arc, RwLock};
use thiserror::Error;

const SET_COOKIE: &str = "set-cookie";
const COOKIE: &str = "cookie";

// pub type MyBody = http_body_util::combinators::BoxBody<bytes::Bytes, Error>;
pub type MyBody = axum::body::Body;

/// Possible errors
#[derive(Error, Debug)]
pub enum Error {
    /// A wrapped error from the hyper crate
    #[error("hyper error `{0}`")]
    Hyper(#[from] hyper::Error),
    /// A wrapped error from the hyper-util crate
    #[error("hyper-util error `{0}`")]
    HyperUtil(#[from] hyper_util::client::legacy::Error),
    /// The request was not successful.
    #[error("request not successful. status code: `{0}`")]
    RequestFailed(http::StatusCode),
}

/// A session for a single server.
#[derive(Clone, Debug)]
pub struct HttpSession {
    base_uri: hyper::Uri,
    jar: Arc<RwLock<cookie_store::CookieStore>>,
}

/// Create an `HttpSession` which has already made a request
#[tracing::instrument(level = "debug", skip(server_info, jar))]
pub async fn create_session(
    server_info: &flydra_types::BuiServerAddrInfo,
    jar: Arc<RwLock<cookie_store::CookieStore>>,
) -> Result<HttpSession, Error> {
    let base_uri = format!("http://{}/", server_info.addr());
    let token = server_info.token().clone();
    let mut base = HttpSession::new(&base_uri, jar);
    base.get_with_token("", token).await?;
    Ok(base)
}

impl HttpSession {
    fn new(base_uri: &str, jar: Arc<RwLock<cookie_store::CookieStore>>) -> Self {
        let base_uri: hyper::Uri = base_uri.parse().expect("failed to parse uri");
        if let Some(pq) = base_uri.path_and_query() {
            assert_eq!(pq.path(), "/");
            assert!(pq.query().is_none());
        }
        Self { base_uri, jar }
    }
    /// get a relative url to the base url
    fn get_rel_uri(&self, rel: &str, token1: Option<AccessToken>) -> hyper::Uri {
        let token = if let Some(tok1) = token1 {
            match tok1 {
                AccessToken::NoToken => None,
                AccessToken::PreSharedToken(t) => Some(t),
            }
        } else {
            None
        };

        let pq: String = if let Some(token) = token {
            format!("/{}?token={}", rel, token)
        } else {
            format!("/{}", rel)
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
    async fn inner_req(
        &mut self,
        rel: &str,
        token: Option<AccessToken>,
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
    pub async fn get(
        &mut self,
        rel: &str,
    ) -> Result<hyper::Response<hyper::body::Incoming>, Error> {
        self.inner_req(rel, None, &[], http::Method::GET, axum::body::Body::empty())
            .await
    }
    pub async fn req_accepts(
        &mut self,
        rel: &str,
        accepts: &[HeaderValue],
        method: http::Method,
        body: axum::body::Body,
    ) -> Result<hyper::Response<hyper::body::Incoming>, Error> {
        self.inner_req(rel, None, accepts, method, body).await
    }
    async fn get_with_token(
        &mut self,
        rel: &str,
        token: AccessToken,
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

fn handle_response(
    url: &url::Url,
    jar2: Arc<RwLock<cookie_store::CookieStore>>,
    mut response: hyper::Response<hyper::body::Incoming>,
) -> Result<hyper::Response<hyper::body::Incoming>, Error> {
    tracing::trace!("starting to handle cookies in response {:?}", response);

    use hyper::header::Entry::*;
    match response.headers_mut().entry(SET_COOKIE) {
        Occupied(e) => {
            let (_key, drain) = e.remove_entry_mult();
            let mut jar = jar2.write().unwrap();
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
