use preferences_serde1::Preferences;
use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock},
};
use tracing::{debug, error, info, warn};

use bui_backend_session::HttpSession;
use flydra_types::{BuiServerInfo, RawCamName};
use strand_cam_storetype::CallbackType;

/// Keeps HTTP sessions for all connected cameras.
#[derive(Clone)]
pub(crate) struct StrandCamHttpSessionHandler {
    cam_manager: flydra2::ConnectedCamerasManager,
    pub(crate) name_to_session: Arc<RwLock<BTreeMap<RawCamName, MaybeSession>>>,
    jar: Arc<RwLock<cookie_store::CookieStore>>,
}

#[derive(Clone)]
pub(crate) enum MaybeSession {
    Alive(HttpSession),
    Errored,
}

use crate::mainbrain::{MainbrainError, MainbrainResult};

impl StrandCamHttpSessionHandler {
    pub(crate) fn new(
        cam_manager: flydra2::ConnectedCamerasManager,
        jar: Arc<RwLock<cookie_store::CookieStore>>,
    ) -> Self {
        Self {
            cam_manager,
            name_to_session: Arc::new(RwLock::new(BTreeMap::new())),
            jar,
        }
    }
    async fn open_session(&self, cam_name: &RawCamName) -> Result<MaybeSession, MainbrainError> {
        // Create a new session if it doesn't exist.
        let bui_server_addr_info = {
            if let Some(cam_addr) = self.cam_manager.http_camserver_info(cam_name) {
                match cam_addr {
                    BuiServerInfo::NoServer => {
                        panic!("cannot connect to camera with no server");
                    }
                    BuiServerInfo::Server(details) => details,
                }
            } else {
                return Err(MainbrainError::UnknownCamera {
                    cam_name: cam_name.clone(),
                });
            }
        };

        info!(
            "opening session for cam {} to {}",
            cam_name.as_str(),
            bui_server_addr_info.addr(),
        );

        let result =
            bui_backend_session::create_session(&bui_server_addr_info, self.jar.clone()).await;
        let session = match result {
            Ok(session) => {
                let mut name_to_session = self.name_to_session.write().unwrap();
                let session = MaybeSession::Alive(session);
                name_to_session.insert(cam_name.clone(), session.clone());
                session
            }
            Err(e) => {
                error!(
                    "could not create session to {}: {}",
                    bui_server_addr_info.addr(),
                    e
                );
                return Err(e.into());
            }
        };
        {
            // We have the cookie from braid now, so store it to disk.
            let jar = self.jar.read().unwrap();
            Preferences::save(
                &*jar,
                &crate::mainbrain::APP_INFO,
                crate::mainbrain::STRAND_CAM_COOKIE_KEY,
            )?;
            tracing::debug!(
                "saved cookie store {}",
                crate::mainbrain::STRAND_CAM_COOKIE_KEY
            );
        }
        Ok(session)
    }

    pub(crate) async fn get_or_open_session(
        &self,
        cam_name: &RawCamName,
    ) -> Result<MaybeSession, MainbrainError> {
        // Get session if it already exists.
        let opt_session = { self.name_to_session.read().unwrap().get(cam_name).cloned() };

        // Create session if needed.
        match opt_session {
            Some(session) => Ok(session),
            None => self.open_session(cam_name).await,
        }
    }

    async fn post(
        &self,
        cam_name: &RawCamName,
        args: strand_cam_remote_control::CamArg,
    ) -> Result<(), MainbrainError> {
        let session = self.get_or_open_session(cam_name).await?;

        // Post to session
        match session {
            MaybeSession::Alive(mut session) => {
                let body = axum::body::Body::new(http_body_util::Full::new(bytes::Bytes::from(
                    serde_json::to_vec(&CallbackType::ToCamera(args)).unwrap(),
                )));

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
                            "For \"{}\": StrandCamHttpSessionHandler::post() got error {err:?}",
                            cam_name.as_str(),
                        );
                        let mut name_to_session = self.name_to_session.write().unwrap();
                        name_to_session.insert(cam_name.clone(), MaybeSession::Errored);
                        // return Err(MainbrainError::blarg);
                    }
                }
            }
            MaybeSession::Errored => {
                // TODO: should an error be raised here?
                // return Err(MainbrainError::blarg);
            }
        };
        Ok(())
    }

    pub(crate) async fn send_frame_offset(
        &self,
        cam_name: &RawCamName,
        frame_offset: u64,
    ) -> Result<(), MainbrainError> {
        info!(
            "for cam {}, sending frame offset {}",
            cam_name.as_str(),
            frame_offset
        );
        let args = strand_cam_remote_control::CamArg::SetFrameOffset(frame_offset);
        self.post(cam_name, args).await
    }

    async fn send_quit(&mut self, cam_name: &RawCamName) -> Result<(), MainbrainError> {
        info!("for cam {}, sending quit", cam_name.as_str());
        let args = strand_cam_remote_control::CamArg::DoQuit;

        let cam_result = self.post(cam_name, args).await;

        // If we are telling the camera to quit, we don't want to keep its session around
        let mut name_to_session = self.name_to_session.write().unwrap();
        name_to_session.remove(cam_name);
        self.cam_manager.remove(cam_name);
        // TODO: we should cancel the stream of incoming frames so that they
        // don't get processed after we have removed this camera
        // information.

        match cam_result {
            Ok(_) => Ok(()),
            Err(e) => {
                warn!(
                    "Ignoring error while sending quit command to \"{}\": {}",
                    cam_name.as_str(),
                    e
                );
                Err(e)
            }
        }
    }

    pub(crate) async fn send_quit_all(&mut self) {
        use futures::{stream, StreamExt};
        // Based on https://stackoverflow.com/a/51047786
        const CONCURRENT_REQUESTS: usize = 5;
        let results = stream::iter(self.cam_manager.all_raw_cam_names())
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
                        "Ignoring error When sending quit command to camera \"{}\": {}",
                        cam_name.as_str(),
                        e
                    ),
                }
            })
            .await;
    }

    pub(crate) async fn toggle_saving_mp4_files_all(
        &self,
        start_saving: bool,
    ) -> MainbrainResult<()> {
        let cam_names = self.cam_manager.all_raw_cam_names();
        for cam_name in cam_names.iter() {
            self.toggle_saving_mp4_files(cam_name, start_saving).await?;
        }
        Ok(())
    }

    pub(crate) async fn toggle_saving_mp4_files(
        &self,
        cam_name: &RawCamName,
        start_saving: bool,
    ) -> MainbrainResult<()> {
        debug!(
            "for cam {}, sending save mp4 file {:?}",
            cam_name.as_str(),
            start_saving
        );
        let cam_name = cam_name.clone();

        let args = strand_cam_remote_control::CamArg::SetIsRecordingMp4(start_saving);
        self.post(&cam_name, args).await?;
        Ok(())
    }

    pub(crate) async fn send_clock_model_to_all(
        &self,
        clock_model: Option<rust_cam_bui_types::ClockModel>,
    ) -> MainbrainResult<()> {
        let cam_names = self.cam_manager.all_raw_cam_names();
        for cam_name in cam_names.iter() {
            self.send_triggerbox_clock_model(cam_name, clock_model.clone())
                .await?;
        }
        Ok(())
    }

    pub(crate) async fn send_triggerbox_clock_model(
        &self,
        cam_name: &RawCamName,
        clock_model: Option<rust_cam_bui_types::ClockModel>,
    ) -> MainbrainResult<()> {
        debug!(
            "for cam {}, sending clock model {:?}",
            cam_name.as_str(),
            clock_model
        );
        let cam_name = cam_name.clone();

        let args = strand_cam_remote_control::CamArg::SetTriggerboxClockModel(clock_model);
        self.post(&cam_name, args).await
    }

    pub(crate) async fn set_post_trigger_buffer_all(
        &self,
        num_frames: usize,
    ) -> MainbrainResult<()> {
        let cam_names = self.cam_manager.all_raw_cam_names();
        for cam_name in cam_names.iter() {
            self.set_post_trigger_buffer(cam_name, num_frames).await?;
        }
        Ok(())
    }

    pub(crate) async fn set_post_trigger_buffer(
        &self,
        cam_name: &RawCamName,
        num_frames: usize,
    ) -> MainbrainResult<()> {
        debug!(
            "for cam {}, sending set post trigger buffer {}",
            cam_name.as_str(),
            num_frames
        );
        let cam_name = cam_name.clone();

        let args = strand_cam_remote_control::CamArg::SetPostTriggerBufferSize(num_frames);
        self.post(&cam_name, args).await?;
        Ok(())
    }

    pub(crate) async fn initiate_post_trigger_mp4_all(&self) -> MainbrainResult<()> {
        let cam_names = self.cam_manager.all_raw_cam_names();
        for cam_name in cam_names.iter() {
            self.initiate_post_trigger_mp4(cam_name).await?;
        }
        Ok(())
    }

    pub(crate) async fn initiate_post_trigger_mp4(
        &self,
        cam_name: &RawCamName,
    ) -> MainbrainResult<()> {
        debug!(
            "for cam {}, initiating post trigger recording",
            cam_name.as_str(),
        );
        let cam_name = cam_name.clone();

        let args = strand_cam_remote_control::CamArg::PostTrigger;
        self.post(&cam_name, args).await?;
        Ok(())
    }
}
