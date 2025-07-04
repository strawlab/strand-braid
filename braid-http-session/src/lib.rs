use std::sync::{Arc, RwLock};
use strand_bui_backend_session::HttpSession;
use tracing::{debug, error};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("{0}")]
    FlydraTypesError(#[from] braid_types::FlydraTypesError),
    #[error("{0}")]
    JsonError(#[from] serde_json::Error),
    #[error("{0}")]
    HyperError(#[from] hyper::Error),
    #[error("{0}")]
    BuiBackendSession(#[from] strand_bui_backend_session::Error),
    #[error("HTTP error {0} when calling {1}")]
    HttpError(hyper::StatusCode, String),
}

/// Create a `MainbrainSession` which has already made a request
#[tracing::instrument(level = "info")]
pub async fn create_mainbrain_session(
    dest: strand_bui_backend_session_types::BuiServerAddrInfo,
    jar: Arc<RwLock<cookie_store::CookieStore>>,
) -> Result<MainbrainSession, strand_bui_backend_session::Error> {
    debug!("requesting session with mainbrain at {:?}", dest);
    let inner = strand_bui_backend_session::create_session(&dest, jar).await?;
    Ok(MainbrainSession { inner })
}

fn body_from_buf(body_buf: &[u8]) -> axum::body::Body {
    axum::body::Body::new(http_body_util::Full::new(bytes::Bytes::from(
        body_buf.to_vec(),
    )))
}

/// This allows communicating with the Mainbrain over HTTP RPC.
///
/// This replaced the old ROS layer for camera -> mainbrain command and control
/// communication from flydra.
#[derive(Clone, Debug)]
pub struct MainbrainSession {
    inner: HttpSession,
}

impl MainbrainSession {
    #[tracing::instrument(skip_all)]
    async fn do_post(&mut self, bytes: Vec<u8>) -> Result<(), Error> {
        let body = body_from_buf(&bytes);

        debug!("calling mainbrain callback handler");
        let _resp = self.inner.post("callback", body).await?;
        Ok(())
    }

    pub async fn get_remote_info(
        &mut self,
        raw_cam_name: &braid_types::RawCamName,
    ) -> Result<braid_types::RemoteCameraInfoResponse, Error> {
        let path = format!(
            "{}/{}",
            braid_types::braid_http::REMOTE_CAMERA_INFO_PATH,
            braid_types::braid_http::encode_cam_name(raw_cam_name)
        );

        debug!(
            "Getting remote camera info for camera \"{}\".",
            raw_cam_name.as_str()
        );

        let resp = self.inner.get(&path).await?;

        if !resp.status().is_success() {
            error!("error: GET was not a success {}:{}", file!(), line!());
            return Err(Error::HttpError(resp.status(), path));
        };

        // fold all chunks into one Vec<u8>
        let body = resp.into_body();
        let chunks: Result<http_body_util::Collected<bytes::Bytes>, hyper::Error> = {
            use http_body_util::BodyExt;
            body.collect().await
        };
        let data = chunks?.to_bytes();

        // parse data
        Ok(serde_json::from_slice::<
            braid_types::RemoteCameraInfoResponse,
        >(&data)?)
    }

    #[tracing::instrument(skip_all)]
    pub async fn post_callback_message(
        &mut self,
        msg: braid_types::BraidHttpApiCallback,
    ) -> Result<(), Error> {
        let bytes = serde_json::to_vec(&msg).unwrap();
        self.do_post(bytes).await
    }
}
