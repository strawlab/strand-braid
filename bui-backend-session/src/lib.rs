#[macro_use]
extern crate log;

use bui_backend_types::AccessToken;
use parking_lot::RwLock;
use std::sync::Arc;
use thiserror::Error;

const SET_COOKIE: &str = "set-cookie";
const COOKIE: &str = "cookie";

pub type MyBody = http_body_util::combinators::BoxBody<bytes::Bytes, Error>;

fn body_from_buf(body_buf: &[u8]) -> MyBody {
    let body = http_body_util::Full::new(bytes::Bytes::from(body_buf.to_vec()));
    use http_body_util::BodyExt;
    MyBody::new(body.map_err(|_: std::convert::Infallible| unreachable!()))
}

/// Possible errors
#[derive(Error, Debug)]
pub enum Error {
    /// A wrapped error from the hyper crate
    #[error("hyper error `{0}`")]
    Hyper(#[from] hyper::Error),
    /// A wrapped error from the hyper-util crate
    #[error("hyper-util error `{0}`")]
    HyperUtil(#[from] hyper_util::client::legacy::Error),
}

/// A session for a single server.
///
/// Warning: this does not attempt to store cookies associated with a single
/// server and thus could subject you to cross-signing attacks. Therefore, the
/// name `InsecureSession`.
#[derive(Clone)]
pub struct InsecureSession {
    base_uri: hyper::Uri,
    jar: Arc<RwLock<cookie::CookieJar>>,
}

/// Create an `InsecureSession` which has already made a request
pub async fn future_session(base_uri: &str, token: AccessToken) -> Result<InsecureSession, Error> {
    let mut base = InsecureSession::new(base_uri);
    base.get_with_token("", token).await?;
    Ok(base)
}

impl InsecureSession {
    fn new(base_uri: &str) -> Self {
        let base_uri: hyper::Uri = base_uri.parse().expect("failed to parse uri");
        if let Some(pq) = base_uri.path_and_query() {
            assert_eq!(pq.path(), "/");
            assert!(pq.query().is_none());
        }
        Self {
            base_uri,
            jar: Arc::new(RwLock::new(cookie::CookieJar::new())),
        }
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
    async fn inner_get(
        &mut self,
        rel: &str,
        token: Option<AccessToken>,
    ) -> Result<hyper::Response<hyper::body::Incoming>, Error> {
        let uri = self.get_rel_uri(rel, token);

        let mut req = hyper::Request::new(body_from_buf(b""));
        *req.method_mut() = hyper::Method::GET;
        *req.uri_mut() = uri;
        self.make_request(req).await
    }
    pub async fn get(
        &mut self,
        rel: &str,
    ) -> Result<hyper::Response<hyper::body::Incoming>, Error> {
        self.inner_get(rel, None).await
    }
    async fn get_with_token(
        &mut self,
        rel: &str,
        token: AccessToken,
    ) -> Result<hyper::Response<hyper::body::Incoming>, Error> {
        self.inner_get(rel, Some(token)).await
    }

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

    async fn make_request(
        &mut self,
        mut req: hyper::Request<MyBody>,
    ) -> Result<hyper::Response<hyper::body::Incoming>, Error> {
        let client =
            hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
                .build_http();

        debug!("building request");
        {
            let jar = self.jar.read();
            for cookie in jar.iter() {
                debug!("adding cookie {}", cookie);
                req.headers_mut().insert(
                    COOKIE,
                    hyper::header::HeaderValue::from_str(&cookie.to_string()).unwrap(),
                );
            }
        }

        let jar2 = self.jar.clone();
        debug!("making request {:?}", req);
        let response = client.request(req).await?;

        debug!("handling response {:?}", response);
        handle_response(jar2, response)
    }
}

fn handle_response(
    jar2: Arc<RwLock<cookie::CookieJar>>,
    mut response: hyper::Response<hyper::body::Incoming>,
) -> Result<hyper::Response<hyper::body::Incoming>, Error> {
    debug!("starting to handle cookies in response {:?}", response);

    use hyper::header::Entry::*;
    match response.headers_mut().entry(SET_COOKIE) {
        Occupied(e) => {
            let (_key, drain) = e.remove_entry_mult();
            let mut jar = jar2.write();
            for cookie_raw in drain {
                let c = cookie::Cookie::parse(cookie_raw.to_str().unwrap().to_string()).unwrap();
                jar.add(c);
                // TODO FIXME do not reinsert same cookie again and again
                debug!("stored cookie {:?}", cookie_raw);
            }
        }
        Vacant(_) => {}
    }

    debug!("done handling cookies in response {:?}", response);
    Ok(response)
}
