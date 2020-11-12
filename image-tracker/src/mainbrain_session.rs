use ::serde_json;
use hyper;

use ::bui_backend_session::{future_session, InsecureSession};
use flydra_types;

// TODO: move this into strand-cam (and out of image-tracker).

/// Create a `MainbrainSession` which has already made a request
pub async fn mainbrain_future_session(
    dest: flydra_types::MainbrainBuiLocation,
) -> Result<MainbrainSession, hyper::Error> {
    let base_url = dest.0.base_url();
    let token = dest.0.token();
    debug!("requesting session with mainbrain at {}", base_url);
    let inner = future_session(&base_url, token.clone()).await?;
    Ok(MainbrainSession { inner })
}

/// This allows communicating with the Mainbrain over HTTP RPC.
///
/// This will take the place of ROS for camera -> mainbrain command and control
/// communication.
#[derive(Clone)]
pub struct MainbrainSession {
    inner: InsecureSession,
}

impl MainbrainSession {
    async fn do_post(&mut self, bytes: Vec<u8>) -> Result<(), hyper::Error> {
        let body = hyper::Body::from(bytes);

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

    pub async fn register_flydra_camnode(
        &mut self,
        orig_cam_name: flydra_types::RawCamName,
        http_camserver_info: flydra_types::CamHttpServerInfo,
        ros_cam_name: flydra_types::RosCamName,
    ) -> Result<(), hyper::Error> {
        let msg = flydra_types::RegisterNewCamera {
            orig_cam_name,
            http_camserver_info,
            ros_cam_name,
        };

        debug!("register_flydra_camnode with message {:?}", msg);
        let msg = flydra_types::HttpApiCallback::NewCamera(msg);
        let bytes = serde_json::to_vec(&msg).unwrap();
        self.do_post(bytes).await
    }

    pub async fn update_image(
        &mut self,
        ros_cam_name: flydra_types::RosCamName,
        current_image_png: Vec<u8>,
    ) -> Result<(), hyper::Error> {
        let msg = flydra_types::UpdateImage {
            ros_cam_name,
            current_image_png,
        };

        debug!("update_image with message {:?}", msg);
        let msg = flydra_types::HttpApiCallback::UpdateCurrentImage(msg);
        let bytes = serde_json::to_vec(&msg).unwrap();
        self.do_post(bytes).await
    }
}
