use ::bui_backend_session::{future_session, InsecureSession};
use log::{debug, error};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("{0}")]
    FlydraTypesError(#[from] flydra_types::FlydraTypesError),
    #[error("{0}")]
    JsonError(#[from] serde_json::Error),
    #[error("{0}")]
    HyperError(#[from] hyper::Error),
    #[error("{0}")]
    BuiBackendSession(#[from] bui_backend_session::Error),
    #[error("HTTP error {0} when calling {1}")]
    HttpError(hyper::StatusCode, String),
}

/// Create a `MainbrainSession` which has already made a request
pub async fn mainbrain_future_session(
    dest: flydra_types::MainbrainBuiLocation,
) -> Result<MainbrainSession, bui_backend_session::Error> {
    let base_url = dest.0.base_url();
    let token = dest.0.token();
    debug!("requesting session with mainbrain at {}", base_url);
    let inner = future_session(&base_url, token.clone()).await?;
    Ok(MainbrainSession { inner })
}

type MyBody = http_body_util::combinators::BoxBody<bytes::Bytes, bui_backend_session::Error>;

fn body_from_buf(body_buf: &[u8]) -> MyBody {
    let body = http_body_util::Full::new(bytes::Bytes::from(body_buf.to_vec()));
    use http_body_util::BodyExt;
    MyBody::new(body.map_err(|_: std::convert::Infallible| unreachable!()))
}

/// This allows communicating with the Mainbrain over HTTP RPC.
///
/// This replaced the old ROS layer for camera -> mainbrain command and control
/// communication from flydra.
#[derive(Clone)]
pub struct MainbrainSession {
    inner: InsecureSession,
}

impl MainbrainSession {
    async fn do_post(&mut self, bytes: Vec<u8>) -> Result<(), Error> {
        let body = body_from_buf(&bytes);

        let resp = self.inner.post("callback", body).await?;

        debug!("called do_post and got response: {:?}", resp);
        if !resp.status().is_success() {
            error!(
                "error: POST response was not a success {}:{}",
                file!(),
                line!()
            );
            // TODO: return Err(_)?
        };
        Ok(())
    }

    pub async fn get_remote_info(
        &mut self,
        orig_cam_name: &flydra_types::RawCamName,
    ) -> Result<flydra_types::RemoteCameraInfoResponse, Error> {
        let path = format!(
            "{}?camera={}",
            flydra_types::REMOTE_CAMERA_INFO_PATH,
            orig_cam_name.as_str()
        );

        debug!(
            "Getting remote camera info for camera \"{}\".",
            orig_cam_name.as_str()
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
            flydra_types::RemoteCameraInfoResponse,
        >(&data)?)
    }

    pub async fn register_flydra_camnode(
        &mut self,
        msg: &flydra_types::RegisterNewCamera,
    ) -> Result<(), Error> {
        debug!("register_flydra_camnode with message {:?}", msg);
        let msg = flydra_types::HttpApiCallback::NewCamera(msg.clone());
        Ok(self.send_message(msg).await?)
    }

    pub async fn update_image(
        &mut self,
        ros_cam_name: flydra_types::RosCamName,
        current_image_png: flydra_types::PngImageData,
    ) -> Result<(), Error> {
        let msg = flydra_types::PerCam {
            ros_cam_name,
            inner: flydra_types::UpdateImage { current_image_png },
        };

        debug!("update_image with message {:?}", msg);
        let msg = flydra_types::HttpApiCallback::UpdateCurrentImage(msg);
        Ok(self.send_message(msg).await?)
    }

    pub async fn send_message(&mut self, msg: flydra_types::HttpApiCallback) -> Result<(), Error> {
        let bytes = serde_json::to_vec(&msg).unwrap();
        Ok(self.do_post(bytes).await?)
    }
}
