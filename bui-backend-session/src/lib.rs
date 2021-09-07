#[macro_use]
extern crate log;

use bui_backend_types::AccessToken;
use parking_lot::RwLock;
use std::sync::Arc;

const SET_COOKIE: &str = "set-cookie";
const COOKIE: &str = "cookie";

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
pub async fn future_session(
    base_uri: &str,
    token: AccessToken,
) -> Result<InsecureSession, hyper::Error> {
    let mut base = InsecureSession::new(&base_uri);
    base.get_with_token("", token).await?;
    Ok(base)
}

impl InsecureSession {
    fn new(base_uri: &str) -> Self {
        let base_uri: hyper::Uri = base_uri.parse().expect("failed to parse uri");
        if let Some(ref pq) = base_uri.path_and_query() {
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
    ) -> Result<hyper::Response<hyper::Body>, hyper::Error> {
        let uri = self.get_rel_uri(rel, token);

        let body = hyper::Body::empty();
        let mut req = hyper::Request::new(body);
        *req.method_mut() = hyper::Method::GET;
        *req.uri_mut() = uri;
        self.make_request(req).await
    }
    pub async fn get(&mut self, rel: &str) -> Result<hyper::Response<hyper::Body>, hyper::Error> {
        self.inner_get(rel, None).await
    }
    async fn get_with_token(
        &mut self,
        rel: &str,
        token: AccessToken,
    ) -> Result<hyper::Response<hyper::Body>, hyper::Error> {
        self.inner_get(rel, Some(token)).await
    }

    pub async fn post(
        &mut self,
        rel: &str,
        body: hyper::Body,
    ) -> Result<hyper::Response<hyper::Body>, hyper::Error> {
        let uri = self.get_rel_uri(rel, None);

        let mut req = hyper::Request::new(body);
        *req.method_mut() = hyper::Method::POST;
        *req.uri_mut() = uri;
        self.make_request(req).await
    }

    async fn make_request(
        &mut self,
        mut req: hyper::Request<hyper::Body>,
    ) -> Result<hyper::Response<hyper::Body>, hyper::Error> {
        let client = hyper::Client::new();

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
    mut response: hyper::Response<hyper::Body>,
) -> Result<hyper::Response<hyper::Body>, hyper::Error> {
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
