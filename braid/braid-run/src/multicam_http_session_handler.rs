use parking_lot::RwLock;
use std::{collections::BTreeMap, sync::Arc};

use bui_backend_session::{self, InsecureSession};
use flydra_types::{RosCamName, StrandCamHttpServerInfo};
use strand_cam_storetype::CallbackType;

/// Keeps HTTP sessions for all connected cameras.
#[derive(Clone)]
pub struct StrandCamHttpSessionHandler {
    cam_manager: flydra2::ConnectedCamerasManager,
    name_to_session: Arc<RwLock<BTreeMap<RosCamName, MaybeSession>>>,
}

#[derive(Clone)]
enum MaybeSession {
    Alive(InsecureSession),
    Errored,
}

use crate::mainbrain::{MainbrainError, MainbrainResult};

type MyBody = http_body_util::combinators::BoxBody<bytes::Bytes, bui_backend_session::Error>;

fn body_from_buf(body_buf: &[u8]) -> MyBody {
    let body = http_body_util::Full::new(bytes::Bytes::from(body_buf.to_vec()));
    use http_body_util::BodyExt;
    MyBody::new(body.map_err(|_: std::convert::Infallible| unreachable!()))
}

impl StrandCamHttpSessionHandler {
    pub fn new(cam_manager: flydra2::ConnectedCamerasManager) -> Self {
        Self {
            cam_manager,
            name_to_session: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }
    async fn open_session(&self, cam_name: &RosCamName) -> Result<MaybeSession, MainbrainError> {
        // Create a new session if it doesn't exist.
        let (base_url, token) = {
            if let Some(cam_addr) = self.cam_manager.http_camserver_info(cam_name) {
                match cam_addr {
                    StrandCamHttpServerInfo::NoServer => {
                        panic!("cannot connect to camera with no server");
                    }
                    StrandCamHttpServerInfo::Server(details) => {
                        (details.base_url(), details.token().clone())
                    }
                }
            } else {
                panic!("attempting post to unknown camera")
            }
        };

        info!(
            "opening session for cam {} to {}",
            cam_name.as_str(),
            base_url
        );

        let result = bui_backend_session::future_session(&base_url, token).await;
        match result {
            Ok(session) => {
                let mut name_to_session = self.name_to_session.write();
                let session = MaybeSession::Alive(session);
                name_to_session.insert(cam_name.clone(), session.clone());
                Ok(session)
            }
            Err(e) => {
                error!("could not create session to {}: {}", base_url, e);
                Err(e.into())
            }
        }
    }

    async fn get_or_open_session(
        &self,
        cam_name: &RosCamName,
    ) -> Result<MaybeSession, MainbrainError> {
        // Get session if it already exists.
        let opt_session = { self.name_to_session.read().get(cam_name).cloned() };

        // Create session if needed.
        match opt_session {
            Some(session) => Ok(session),
            None => self.open_session(cam_name).await,
        }
    }

    async fn post(
        &self,
        cam_name: &RosCamName,
        args: ci2_remote_control::CamArg,
    ) -> Result<(), MainbrainError> {
        let session = self.get_or_open_session(cam_name).await?;

        // Post to session
        match session {
            MaybeSession::Alive(mut session) => {
                let body =
                    body_from_buf(&serde_json::to_vec(&CallbackType::ToCamera(args)).unwrap());

                let result = session.post("callback", body).await;
                match result {
                    Ok(response) => {
                        debug!(
                            "StrandCamHttpSessionHandler::post() got response {:?}",
                            response
                        );
                    }
                    Err(err) => {
                        error!(
                            "For {cam_name}: StrandCamHttpSessionHandler::post() got error {err:?}"
                        );
                        let mut name_to_session = self.name_to_session.write();
                        name_to_session.insert(cam_name.clone(), MaybeSession::Errored);
                    }
                }
            }
            MaybeSession::Errored => {}
        };
        Ok(())
    }

    pub async fn send_frame_offset(
        &self,
        cam_name: &RosCamName,
        frame_offset: u64,
    ) -> Result<(), MainbrainError> {
        info!(
            "for cam {}, sending frame offset {}",
            cam_name.as_str(),
            frame_offset
        );
        let args = ci2_remote_control::CamArg::SetFrameOffset(frame_offset);
        self.post(cam_name, args).await
    }

    async fn send_quit(&mut self, cam_name: &RosCamName) -> Result<(), MainbrainError> {
        info!("for cam {}, sending quit", cam_name.as_str());
        let args = ci2_remote_control::CamArg::DoQuit;

        let cam_result = self.post(cam_name, args).await;

        // If we are telling the camera to quit, we don't want to keep its session around
        let mut name_to_session = self.name_to_session.write();
        name_to_session.remove(cam_name);
        self.cam_manager.remove(cam_name);
        // TODO: we should cancel the stream of incoming frames so that they
        // don't get processed after we have removed this camera
        // information.

        match cam_result {
            Ok(_) => Ok(()),
            Err(e) => {
                warn!(
                    "Ignoring error while sending quit command to {}: {}",
                    cam_name, e
                );
                Err(e.into())
            }
        }
    }

    pub async fn send_quit_all(&mut self) {
        use futures::{stream, StreamExt};
        // Based on https://stackoverflow.com/a/51047786
        const CONCURRENT_REQUESTS: usize = 5;
        let results = stream::iter(self.cam_manager.all_ros_cam_names())
            .map(|cam_name| {
                let mut session = self.clone();
                let cam_name = cam_name.clone();
                async move {
                    session
                        .send_quit(&cam_name)
                        .await
                        .map_err(|e| (cam_name, e))
                }
            })
            .buffer_unordered(CONCURRENT_REQUESTS);

        results
            .for_each(|r| async {
                match r {
                    Ok(()) => {}
                    Err((cam_name, e)) => warn!(
                        "Ignoring error When sending quit command to camera {}: {}",
                        cam_name, e
                    ),
                }
            })
            .await;
    }

    pub async fn toggle_saving_mp4_files_all(&self, start_saving: bool) -> MainbrainResult<()> {
        let cam_names = self.cam_manager.all_ros_cam_names();
        for cam_name in cam_names.iter() {
            self.toggle_saving_mp4_files(cam_name, start_saving).await?;
        }
        Ok(())
    }

    pub async fn toggle_saving_mp4_files(
        &self,
        cam_name: &RosCamName,
        start_saving: bool,
    ) -> MainbrainResult<()> {
        debug!(
            "for cam {}, sending save mp4 file {:?}",
            cam_name.as_str(),
            start_saving
        );
        let cam_name = cam_name.clone();

        let args = ci2_remote_control::CamArg::SetIsRecordingMp4(start_saving);
        self.post(&cam_name, args).await?;
        Ok(())
    }

    pub async fn send_clock_model_to_all(
        &self,
        clock_model: Option<rust_cam_bui_types::ClockModel>,
    ) -> MainbrainResult<()> {
        let cam_names = self.cam_manager.all_ros_cam_names();
        for cam_name in cam_names.iter() {
            self.send_clock_model(cam_name, clock_model.clone()).await?;
        }
        Ok(())
    }

    pub async fn send_clock_model(
        &self,
        cam_name: &RosCamName,
        clock_model: Option<rust_cam_bui_types::ClockModel>,
    ) -> MainbrainResult<()> {
        debug!(
            "for cam {}, sending clock model {:?}",
            cam_name.as_str(),
            clock_model
        );
        let cam_name = cam_name.clone();

        let args = ci2_remote_control::CamArg::SetClockModel(clock_model);
        self.post(&cam_name, args).await
    }

    pub async fn set_post_trigger_buffer_all(&self, num_frames: usize) -> MainbrainResult<()> {
        let cam_names = self.cam_manager.all_ros_cam_names();
        for cam_name in cam_names.iter() {
            self.set_post_trigger_buffer(cam_name, num_frames).await?;
        }
        Ok(())
    }

    pub async fn set_post_trigger_buffer(
        &self,
        cam_name: &RosCamName,
        num_frames: usize,
    ) -> MainbrainResult<()> {
        debug!(
            "for cam {}, sending set post trigger buffer {}",
            cam_name.as_str(),
            num_frames
        );
        let cam_name = cam_name.clone();

        let args = ci2_remote_control::CamArg::SetPostTriggerBufferSize(num_frames);
        self.post(&cam_name, args).await?;
        Ok(())
    }

    pub async fn initiate_post_trigger_mp4_all(&self) -> MainbrainResult<()> {
        let cam_names = self.cam_manager.all_ros_cam_names();
        for cam_name in cam_names.iter() {
            self.initiate_post_trigger_mp4(cam_name).await?;
        }
        Ok(())
    }

    pub async fn initiate_post_trigger_mp4(&self, cam_name: &RosCamName) -> MainbrainResult<()> {
        debug!(
            "for cam {}, initiating post trigger recording",
            cam_name.as_str(),
        );
        let cam_name = cam_name.clone();

        let args = ci2_remote_control::CamArg::PostTrigger;
        self.post(&cam_name, args).await?;
        Ok(())
    }
}
