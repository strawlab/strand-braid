use parking_lot::RwLock;
use std::{collections::BTreeMap, sync::Arc};

use bui_backend_session::{self, InsecureSession};
use flydra_types::{CamHttpServerInfo, RosCamName};
use strand_cam_storetype::CallbackType;

/// Keeps HTTP sessions for all connected cameras.
#[derive(Clone)]
pub struct HttpSessionHandler {
    cam_manager: flydra2::ConnectedCamerasManager,
    name_to_session: Arc<RwLock<BTreeMap<RosCamName, InsecureSession>>>,
}

type MyError = std::io::Error; // anything that implements std::error::Error and Send

impl HttpSessionHandler {
    pub fn new(cam_manager: flydra2::ConnectedCamerasManager) -> Self {
        Self {
            cam_manager,
            name_to_session: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }

    async fn post(
        &mut self,
        cam_name: &RosCamName,
        args: ci2_remote_control::CamArg,
    ) -> Result<hyper::Response<hyper::Body>, hyper::Error> {
        let do_quit_cam_session = args == ci2_remote_control::CamArg::DoQuit;
        let data = CallbackType::ToCamera(args);
        let buf = serde_json::to_vec(&data).unwrap();
        let chunks = vec![Ok::<_, MyError>(buf)];
        let stream = futures::stream::iter(chunks);
        let body = hyper::body::Body::wrap_stream(stream);

        // Get session if it already exists.
        let opt_session = {
            let name_to_session = self.name_to_session.read();
            match name_to_session.get(cam_name) {
                Some(session) => Some(session.clone()),
                None => None,
            }
        };

        let name_to_session_arc = self.name_to_session.clone();

        // Create a future that completes when session is ready.
        match opt_session {
            None => {
                // Create a new session if it doesn't exist.
                let (base_url, token) = {
                    if let Some(cam_addr) = self.cam_manager.http_camserver_info(cam_name) {
                        match cam_addr {
                            CamHttpServerInfo::NoServer => {
                                panic!("cannot connect to camera with no server");
                            }
                            CamHttpServerInfo::Server(details) => {
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

                let cam_name_str = cam_name.clone();
                let name_to_session_arc = self.name_to_session.clone();

                let result = bui_backend_session::future_session(&base_url, token).await;
                match result {
                    Ok(session) => {
                        let mut name_to_session = name_to_session_arc.write();
                        name_to_session.insert(cam_name_str.clone(), session.clone());
                    }
                    Err(e) => {
                        error!("could not create session to {}: {}", base_url, e);
                        return Err(e);
                    }
                }
            }
            Some(_session_ref) => {}
        }

        // Create post request (when the session is available).
        let cam_name_str = cam_name.clone();

        let cam_name_str2 = cam_name_str.clone();

        let mut session = {
            let mut name_to_session = name_to_session_arc.write();
            name_to_session.get_mut(&cam_name_str2).unwrap().clone()
        };

        // If we are telling the camera to quit, we don't want to keep its session around
        if do_quit_cam_session {
            let mut name_to_session = name_to_session_arc.write();
            name_to_session.remove(&cam_name_str);
            self.cam_manager.remove(&cam_name_str);
            // TODO: we should cancel the stream of incoming frames so that they
            // don't get processed after we have removed this camera
            // information.
        }

        let result = session.post("callback", body).await;
        match &result {
            Ok(response) => {
                debug!("HttpSessionHandler::post() got response {:?}", response);
            }
            Err(err) => {
                error!("HttpSessionHandler::post() got error {:?}", err);
            }
        };
        result
    }

    pub async fn send_frame_offset(
        &mut self,
        cam_name: &RosCamName,
        frame_offset: u64,
    ) -> Result<hyper::Response<hyper::Body>, hyper::Error> {
        info!(
            "for cam {}, sending frame offset {}",
            cam_name.as_str(),
            frame_offset
        );
        let args = ci2_remote_control::CamArg::SetFrameOffset(frame_offset);
        self.post(cam_name, args).await
    }

    // pub async fn send_is_recording_ufmf(
    //     &mut self,
    //     cam_name: &RosCamName,
    //     is_recording_ufmf: bool,
    // ) -> Result<hyper::Response<hyper::Body>, hyper::Error> {
    //     info!(
    //         "for cam {}, sending is recording ufmf {}",
    //         cam_name.as_str(),
    //         is_recording_ufmf
    //     );
    //     let args = ci2_remote_control::CamArg::SetIsRecordingUfmf(is_recording_ufmf);
    //     self.post(cam_name, args).await
    // }

    async fn send_quit(&mut self, cam_name: &RosCamName) -> Result<(), hyper::Error> {
        info!("for cam {}, sending quit", cam_name.as_str());
        let args = ci2_remote_control::CamArg::DoQuit;

        let cam_result = self.post(cam_name, args).await;
        match cam_result {
            Ok(_) => Ok(()),
            Err(e) => {
                warn!(
                    "Ignoring error while sending quit command to {}: {}",
                    cam_name, e
                );
                Err(e)
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

    pub async fn send_clock_model_to_all(
        &mut self,
        clock_model: Option<rust_cam_bui_types::ClockModel>,
    ) -> Result<hyper::Response<hyper::Body>, hyper::Error> {
        let cam_names = self.cam_manager.all_ros_cam_names();
        for cam_name in cam_names.iter() {
            self.send_clock_model(cam_name, clock_model.clone()).await?;
        }
        Ok(hyper::Response::new(hyper::Body::empty()))
    }

    pub async fn send_clock_model(
        &mut self,
        cam_name: &RosCamName,
        clock_model: Option<rust_cam_bui_types::ClockModel>,
    ) -> Result<hyper::Response<hyper::Body>, hyper::Error> {
        debug!(
            "for cam {}, sending clock model {:?}",
            cam_name.as_str(),
            clock_model
        );
        let cam_name = cam_name.clone();

        let args = ci2_remote_control::CamArg::SetClockModel(clock_model);
        self.post(&cam_name, args).await
    }
}
