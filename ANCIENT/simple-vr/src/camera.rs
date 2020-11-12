use eventual::Async;
use std::sync::{Arc, Mutex, mpsc};

use reactive_cam::{Frame, ReactiveCamera, HasNewCameraCallback, HasNewFrameCallback, CameraSource};

#[cfg(feature = "camiface")]
use reactive_camiface::CamIfaceReactiveCameraSource;
#[cfg(feature = "flycap")]
use reactive_cam_flycap::FlycapReactiveCameraSource;

use super::cam_view::{self, ExtractedFrame};
use super::tracker::Tracker;
use super::image_processing;
use super::config;

struct FrameGetter {
    tx: mpsc::Sender<Frame>,
}

impl HasNewFrameCallback for FrameGetter {
    fn on_new_frame(&mut self, frame: Frame) {
        // This is called immediately when the frame is ready and may be running in any thread.
        self.tx.send(frame).expect("tx.send");
    }
}

struct CamElem {
    rx: mpsc::Receiver<ExtractedFrame>,
}

pub struct CameraHolderApp {
    cams: Vec<CamElem>,
    view: Option<cam_view::CameraView>,
    tracker: Arc<Mutex<Tracker>>,
    cfg: config::ImageProcessingConfig,
}

impl CameraHolderApp {
    pub fn new(tracker: Arc<Mutex<Tracker>>,
               camview: bool,
               cfg: &config::ImageProcessingConfig)
               -> CameraHolderApp {
        let view = match camview {
            true => Some(cam_view::CameraView::new()),
            false => None,
        };
        CameraHolderApp {
            cams: vec![],
            view: view,
            tracker: tracker,
            cfg: cfg.clone(), // TODO just keep the reference
        }
    }
    pub fn get_n_cams(&self) -> usize {
        self.cams.len()
    }
    pub fn get_tracker_clone(&self) -> Arc<Mutex<Tracker>> {
        self.tracker.clone()
    }
    fn add_camera(&mut self, rx: mpsc::Receiver<ExtractedFrame>, _camera: Box<ReactiveCamera>) {
        let ce = CamElem { rx: rx };
        self.cams.push(ce);
    }
    pub fn camera_step(&mut self) -> bool {
        let cam = &self.cams[0];
        let maybe_frame = get_most_recent_frame(&cam.rx);

        match self.view {
            Some(ref mut view) => view.display_step(maybe_frame),
            None => true,
        }
    }
}

fn extract_image_features_loop(chain0_receiver: mpsc::Receiver<Frame>,
                               chain1_sender: mpsc::Sender<ExtractedFrame>,
                               tracker: Arc<Mutex<Tracker>>,
                               cfg: &config::ImageProcessingConfig) {
    while let Ok(frame) = chain0_receiver.recv() {
        let features = image_processing::process_frame(&frame, cfg);
        tracker.lock().unwrap().handle_new_observation(&features);
        let show = ExtractedFrame {
            frame: frame,
            draw_features: features,
        };
        chain1_sender.send(show).expect("sending extracted features");
    }
}

struct CamerasKeeper {
    parent_app: Arc<Mutex<CameraHolderApp>>,
}

impl HasNewCameraCallback for CamerasKeeper {
    fn on_new_camera(&mut self, mut camera: Box<ReactiveCamera>) {
        let (chain0_sender, chain0_receiver) = mpsc::channel();
        let (chain1_sender, chain1_receiver) = mpsc::channel();
        let tracker = self.parent_app.lock().unwrap().get_tracker_clone();
        let cfg = self.parent_app.lock().unwrap().cfg.clone();
        ::std::thread::spawn(move || {
            extract_image_features_loop(chain0_receiver, chain1_sender, tracker, &cfg);
        });
        debug!("got new camera");
        let modes = camera.get_available_modes().await().expect("modes");
        debug!("  modes {:?}", modes);
        let mode = camera.get_preferred_mode().await().expect("preferred mode");
        debug!("requesting mode {:?}", mode);
        camera.initialize(&*mode).await().expect("initialize");
        let fg = Box::new(FrameGetter { tx: chain0_sender });
        camera.set_new_frame_callback(fg).await().expect("set new frame callback");
        camera.start_streaming().await().expect("start streaming");
        self.parent_app.lock().unwrap().add_camera(chain1_receiver, camera);
    }
}

#[cfg(feature = "camiface")]
pub fn maybe_insert_cam_iface(sources: &mut Vec<Box<CameraSource>>,
                              parent_app: Arc<Mutex<CameraHolderApp>>) {
    debug!("with camiface");
    let ck = Box::new(CamerasKeeper { parent_app: parent_app });
    let cam_iface = CamIfaceReactiveCameraSource::new(ck).unwrap();
    sources.push(Box::new(cam_iface));
}

#[cfg(not(feature = "camiface"))]
pub fn maybe_insert_cam_iface(_sources: &mut Vec<Box<CameraSource>>,
                              _parent_app: Arc<Mutex<CameraHolderApp>>) {
    debug!("without camiface");
}

#[cfg(feature = "flycap")]
pub fn maybe_insert_flycap(sources: &mut Vec<Box<CameraSource>>,
                           parent_app: Arc<Mutex<CameraHolderApp>>) {
    debug!("with flycap");
    let ck = Box::new(CamerasKeeper { parent_app: parent_app });
    let flycap = FlycapReactiveCameraSource::new(ck).unwrap();
    sources.push(Box::new(flycap));
}

#[cfg(not(feature = "flycap"))]
pub fn maybe_insert_flycap(_sources: &mut Vec<Box<CameraSource>>,
                           _parent_app: Arc<Mutex<CameraHolderApp>>) {
    debug!("without flycap");
}

fn get_most_recent_frame(receiver: &mpsc::Receiver<ExtractedFrame>) -> Option<ExtractedFrame> {
    let mut result = None;
    while let Ok(frame) = receiver.try_recv() {
        result = Some(frame);
    }
    result
}
