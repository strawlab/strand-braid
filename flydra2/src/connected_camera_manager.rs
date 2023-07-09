use parking_lot::{Mutex, RwLock};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::{debug, error, info};

use crate::{safe_u8, CamInfoRow, MyFloat};
use flydra_types::{
    CamHttpServerInfo, CamInfo, CamNum, ConnectedCameraSyncState, RawCamName, RecentStats,
    RosCamName, SyncFno,
};

pub(crate) trait HasCameraList {
    fn camera_list(&self) -> CameraList;
}

/// A set of cameras (stored by their CamNum) which is currently connected.
///
/// This struct implements PartialEq so multiple sets of cameras can be checked
/// to see if both groups are identical.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CameraList {
    pub(crate) inner: BTreeSet<u8>,
}

impl CameraList {
    pub(crate) fn new(cams: &[u8]) -> Self {
        let inner = cams.iter().copied().collect();
        Self { inner }
    }
}

impl HasCameraList for CameraList {
    fn camera_list(&self) -> CameraList {
        self.clone()
    }
}

#[derive(Debug)]
pub struct ConnectedCameraInfo {
    cam_num: CamNum,
    orig_cam_name: RawCamName,
    ros_cam_name: RosCamName,
    sync_state: ConnectedCameraSyncState,
    http_camserver_info: CamHttpServerInfo,
    frames_during_sync: u64,
}

impl ConnectedCameraInfo {
    fn copy_to_caminfo(&self) -> CamInfoRow {
        CamInfoRow {
            camn: self.cam_num,
            cam_id: self.ros_cam_name.as_str().to_string(),
        }
    }
}

#[derive(Debug)]
struct ConnectedCamerasManagerInner {
    all_expected_cameras: BTreeSet<RosCamName>,
    next_cam_num: CamNum,
    ccis: BTreeMap<RosCamName, ConnectedCameraInfo>,
    not_yet_connected: BTreeMap<RosCamName, CamNum>,
    all_expected_cameras_are_present: bool,
    all_expected_cameras_are_synced: bool,
    first_frame_arrived: BTreeSet<RosCamName>,
}

pub trait ConnectedCamCallback: Send {
    fn on_cam_changed(&self, _: Vec<CamInfo>);
}

/// keeps track of connected camera state
///
/// There should be a single call to `::new()` made in the app. Then, `clone()`
/// can be called to copy the outer wrapper which links to the actual inner
/// manager via `Arc<Mutex<_>>`.
#[derive(Clone)]
pub struct ConnectedCamerasManager {
    inner: Arc<RwLock<ConnectedCamerasManagerInner>>,
    on_cam_change_func: Arc<Mutex<Option<Box<dyn ConnectedCamCallback>>>>,
    recon: Option<flydra_mvg::FlydraMultiCameraSystem<MyFloat>>,
    signal_all_cams_present: Arc<AtomicBool>,
    signal_all_cams_synced: Arc<AtomicBool>,
}

impl HasCameraList for ConnectedCamerasManager {
    fn camera_list(&self) -> CameraList {
        let inner: BTreeSet<u8> = self
            .inner
            .read()
            .ccis
            .values()
            .map(|cci| cci.cam_num.0)
            .collect();
        CameraList { inner }
    }
}

impl ConnectedCamerasManager {
    pub fn new(
        recon: &Option<flydra_mvg::FlydraMultiCameraSystem<MyFloat>>,
        all_expected_cameras: BTreeSet<RosCamName>,
        signal_all_cams_present: Arc<AtomicBool>,
        signal_all_cams_synced: Arc<AtomicBool>,
    ) -> Self {
        let mut not_yet_connected = BTreeMap::new();

        // pre-reserve cam numbers for cameras in calibration
        let next_cam_num = if let Some(ref recon) = recon {
            for (base_num, cam_name) in recon.cam_names().enumerate() {
                let ros_cam_name = RosCamName::new(cam_name.to_string());
                let cam_num: CamNum = safe_u8(base_num).into();
                not_yet_connected.insert(ros_cam_name, cam_num);
            }
            safe_u8(recon.len())
        } else {
            0
        };

        Self {
            signal_all_cams_present,
            signal_all_cams_synced,
            inner: Arc::new(RwLock::new(ConnectedCamerasManagerInner {
                all_expected_cameras,
                next_cam_num: next_cam_num.into(),
                ccis: BTreeMap::new(),
                not_yet_connected,
                all_expected_cameras_are_present: false,
                all_expected_cameras_are_synced: false,
                first_frame_arrived: BTreeSet::new(),
            })),
            on_cam_change_func: Arc::new(Mutex::new(None)),
            recon: recon.clone(),
        }
    }

    /// The cameras are being (re)synchronized. Clear all inner data and reset camera numbers.
    pub fn reset_sync_data(&mut self) {
        info!("Camera manager dropping old cameras and expecting new cameras");

        let mut next_cam_num = { self.inner.read().next_cam_num.0 };
        let mut not_yet_connected = BTreeMap::new();

        // pre-reserve cam numbers for cameras in calibration
        if let Some(ref recon) = &self.recon {
            for cam_name in recon.cam_names() {
                let cam_num = next_cam_num;
                next_cam_num = safe_u8(next_cam_num as usize + 1);
                let ros_cam_name = RosCamName::new(cam_name.to_string());
                let cam_num: CamNum = cam_num.into();
                not_yet_connected.insert(ros_cam_name, cam_num);
            }
        }

        let old_ccis = {
            let mut inner = self.inner.write();
            inner.next_cam_num = next_cam_num.into();
            let old_ccis = std::mem::take(&mut inner.ccis);
            inner.not_yet_connected = not_yet_connected;
            old_ccis
        };

        for cam_info in old_ccis.values() {
            // This calls self.notify_cam_changed_listeners():
            self.register_new_camera(
                &cam_info.orig_cam_name,
                &cam_info.http_camserver_info,
                &cam_info.ros_cam_name,
            );
        }
    }

    /// Set callback to be called when connected cameras or their state changes
    pub fn set_cam_changed_callback(
        &mut self,
        f: Box<dyn ConnectedCamCallback>,
    ) -> Option<Box<dyn ConnectedCamCallback>> {
        info!("setting listener for new cameras info");
        let old = {
            let mut mutex_guard = self.on_cam_change_func.lock();
            mutex_guard.replace(f)
        };

        // set new data on initial connect
        self.notify_cam_changed_listeners();
        old
    }

    fn notify_cam_changed_listeners(&self) {
        let mutex_guard = self.on_cam_change_func.lock();
        let inner_ref: Option<&Box<dyn ConnectedCamCallback>> = mutex_guard.as_ref();
        if let Some(cb) = inner_ref {
            let cams = {
                // scope for read lock on self.inner
                self.inner
                    .read()
                    .ccis
                    .values()
                    .map(|cci| CamInfo {
                        name: cci.ros_cam_name.clone(),
                        state: cci.sync_state.clone(),
                        http_camserver_info: cci.http_camserver_info.clone(),
                        recent_stats: RecentStats::default(),
                    })
                    .collect()
            };
            cb.on_cam_changed(cams)
        }
    }

    /// Alternative constructor for use in case of a single camera.
    ///
    /// See `new` and `register_new_camera` for the case when multiple cameras
    /// will be added.
    pub fn new_single_cam(
        orig_cam_name: &RawCamName,
        http_camserver_info: &CamHttpServerInfo,
        recon: &Option<flydra_mvg::FlydraMultiCameraSystem<MyFloat>>,
    ) -> Self {
        let ros_cam_name = orig_cam_name.to_ros();

        let signal_all_cams_present = Arc::new(AtomicBool::new(false));
        let signal_all_cams_synced = Arc::new(AtomicBool::new(false));

        let mut all_expected_cameras = BTreeSet::new();
        all_expected_cameras.insert(ros_cam_name.clone());

        let this = Self::new(
            recon,
            all_expected_cameras,
            signal_all_cams_present,
            signal_all_cams_synced,
        );
        {
            let orig_cam_name = orig_cam_name.clone();

            let mut inner = this.inner.write();

            assert!(
                !inner.ccis.contains_key(&ros_cam_name),
                "camera connecting again?"
            );

            let cam_num = if let Some(pre_existing) = inner.not_yet_connected.remove(&ros_cam_name)
            {
                debug!(
                    "registering camera {}, which is in existing calibration",
                    ros_cam_name.as_str()
                );
                pre_existing
            } else {
                debug!(
                    "registering camera {}, which is not in existing calibration",
                    ros_cam_name.as_str()
                );
                // unknown (and thus un-calibrated) camera
                let cam_num = inner.next_cam_num;
                inner.next_cam_num.0 += 1;
                cam_num
            };

            inner.ccis.insert(
                ros_cam_name.clone(),
                ConnectedCameraInfo {
                    cam_num,
                    orig_cam_name,
                    ros_cam_name,
                    sync_state: ConnectedCameraSyncState::Unsynchronized,
                    http_camserver_info: http_camserver_info.clone(),
                    frames_during_sync: 0,
                },
            );
        }
        this
    }

    pub fn remove(&mut self, ros_cam_name: &RosCamName) {
        self.inner.write().ccis.remove(ros_cam_name);
        self.notify_cam_changed_listeners();
    }

    /// This is called to register a camera when it connects to the mainbrain.
    ///
    /// See `new_single_cam` for the case when only a single camera will be
    /// added.
    pub fn register_new_camera(
        &mut self,
        orig_cam_name: &RawCamName,
        http_camserver_info: &CamHttpServerInfo,
        ros_cam_name: &RosCamName,
    ) {
        info!("register_new_camera got {}", ros_cam_name.as_str());
        let orig_cam_name = orig_cam_name.clone();
        let ros_cam_name = ros_cam_name.clone();
        {
            // This scope is for the write lock on self.inner. Keep it minimal.
            let mut inner = self.inner.write();

            assert!(
                !inner.ccis.contains_key(&ros_cam_name),
                "camera {} already connected",
                ros_cam_name
            );

            let cam_num = if let Some(pre_existing) = inner.not_yet_connected.remove(&ros_cam_name)
            {
                debug!(
                    "registering camera {}, which is in existing calibration",
                    ros_cam_name.as_str()
                );
                pre_existing
            } else {
                if self.recon.is_some() {
                    tracing::warn!(
                        "Camera {} connected, but this is not in existing calibration.",
                        ros_cam_name.as_str()
                    );
                }
                // unknown (and thus un-calibrated) camera
                let cam_num_inner = inner.next_cam_num;
                inner.next_cam_num.0 += 1;
                cam_num_inner
            };

            inner.ccis.insert(
                ros_cam_name.clone(),
                ConnectedCameraInfo {
                    cam_num,
                    orig_cam_name,
                    ros_cam_name,
                    sync_state: ConnectedCameraSyncState::Unsynchronized,
                    http_camserver_info: http_camserver_info.clone(),
                    frames_during_sync: 0,
                },
            );
        }
        self.notify_cam_changed_listeners();
    }

    /// Register that a new frame was received
    ///
    /// Returns synced frame number
    pub fn got_new_frame_live<F>(
        &self,
        packet: &flydra_types::FlydraRawUdpPacket,
        sync_pulse_pause_started_arc: &Arc<RwLock<Option<std::time::Instant>>>,
        sync_time_min: std::time::Duration,
        sync_time_max: std::time::Duration,
        mut send_new_frame_offset: F,
    ) -> Option<SyncFno>
    where
        F: FnMut(&RosCamName, u64),
    {
        assert!(packet.framenumber >= 0);

        let ros_cam_name = RosCamName::new(packet.cam_name.clone());

        let cam_frame = packet.framenumber as u64;
        let mut synced_frame = None;
        let mut new_frame0 = None;
        let mut got_frame_during_sync_time = false;
        let mut do_check_if_all_cameras_present = false;
        let mut do_check_if_all_cameras_synchronized = false;
        {
            let inner = self.inner.read();
            if let Some(cci) = inner.ccis.get(&ros_cam_name) {
                // We know this camera already.
                use crate::ConnectedCameraSyncState::*;
                match cci.sync_state {
                    Unsynchronized => {
                        do_check_if_all_cameras_present = true;
                        let sync_pulse_pause_started = sync_pulse_pause_started_arc.read();
                        if let Some(pulse_time) = *sync_pulse_pause_started {
                            let elapsed = pulse_time.elapsed();
                            if sync_time_min < elapsed && elapsed < sync_time_max {
                                // Camera is not synchronized, but we are
                                // expecting a sync pulse. Therefore,
                                // synchronize the camera now.
                                new_frame0 = Some(cam_frame - crate::TRIGGERBOX_FIRST_PULSE);

                                // // `synced_frame` is the first pulsenumber.
                                synced_frame = Some(crate::TRIGGERBOX_FIRST_PULSE);
                            } else if std::time::Duration::from_millis(50) < elapsed {
                                // If we are 50 msec into the pause but we get a
                                // frame but it hasn't get been sync_time_min,
                                // we should complain.
                                got_frame_during_sync_time = true;
                            }
                        }
                    }
                    Synchronized(frame0) => {
                        if cam_frame >= frame0 {
                            // The camera is already synchronized, return synced frame number
                            let corrected_frame_number = cam_frame - frame0;

                            // if corrected_frame_number > crate::TRIGGERBOX_FIRST_PULSE {
                            if corrected_frame_number == u64::MAX {
                                // We have seen a bug in which the frame number is
                                // `u64::MAX`. This checks if this obviously wrong
                                // frame number is introduced after the present
                                // location or before. In any case, if we are
                                // getting frame numbers like this, clearly we
                                // cannot track anymore, so panicing here only
                                // raises the issue slightly earlier.
                                panic!(
                                    "Impossible frame number. cam_name: {}, cam_frame: {}, frame0: {}",
                                    ros_cam_name.as_str(),
                                    cam_frame,
                                    frame0,
                                );
                            }
                            //     synced_frame =
                            //         Some(corrected_frame_number - crate::TRIGGERBOX_FIRST_PULSE);
                            // }
                            synced_frame = Some(corrected_frame_number);
                        }
                    }
                };
            }
            // If we do not know the camera, it is because we are shutting down
            // and have already removed the camera and thus we should ignore
            // this new data.
        }

        if got_frame_during_sync_time {
            let frames_during_sync = {
                // This scope is for the write lock on self.inner. Keep it minimal.
                let mut inner = self.inner.write();
                let frames_during_sync = match inner.ccis.get_mut(&ros_cam_name) {
                    Some(cci) => {
                        cci.frames_during_sync += 1;
                        cci.frames_during_sync
                    }
                    None => {
                        panic!("reached impossible code.");
                    }
                };
                frames_during_sync
            };

            if frames_during_sync > 10 {
                error!(
                    "Many frames during sync period. Camera {} not \
                       being externally triggered?",
                    ros_cam_name.as_str()
                );
            }
        }

        if let Some(frame0) = new_frame0 {
            // Perform the book-keeping associated with synchronization.
            {
                // This scope is for the write lock on self.inner. Keep it minimal.
                let mut inner = self.inner.write();
                match inner.ccis.get_mut(&ros_cam_name) {
                    Some(cci) => {
                        cci.sync_state = ConnectedCameraSyncState::Synchronized(frame0);
                    }
                    None => {
                        panic!("reached impossible code.");
                    }
                }
            }

            self.notify_cam_changed_listeners();

            // Do notifications associated with synchronization.
            send_new_frame_offset(&ros_cam_name, frame0);
            info!(
                "cam {} synchronized with frame offset: {}",
                ros_cam_name.as_str(),
                frame0,
            );
            do_check_if_all_cameras_synchronized = true;
        }

        if do_check_if_all_cameras_present && !self.inner.read().all_expected_cameras_are_present {
            let mut inner = self.inner.write();
            let i2: &mut ConnectedCamerasManagerInner = &mut inner;
            if i2.first_frame_arrived.insert(ros_cam_name.clone()) {
                info!("first frame from camera {} arrived.", ros_cam_name);
                if i2.first_frame_arrived == i2.all_expected_cameras {
                    inner.all_expected_cameras_are_present = true;
                    self.signal_all_cams_present.store(true, Ordering::SeqCst);
                    info!("All expected cameras connected.");
                } else {
                    info!("All expected cameras NOT connected.");
                }
            }
        }

        if do_check_if_all_cameras_synchronized
            && !self.inner.read().all_expected_cameras_are_synced
        {
            let mut inner = self.inner.write();
            let i2: &mut ConnectedCamerasManagerInner = &mut inner;
            // if i2.first_frame_arrived.insert(ros_cam_name.clone()) {
            //     info!("first frame from camera {} arrived.", ros_cam_name);
            let mut all_synced = true;
            for ros_cam_name in i2.all_expected_cameras.iter() {
                let this_sync = i2
                    .ccis
                    .get(ros_cam_name)
                    .map(|cci| cci.sync_state.is_synchronized())
                    .unwrap_or(false);
                if !this_sync {
                    all_synced = false;
                    break;
                }
            }

            if all_synced {
                info!("All expected cameras synchronized.");
                self.signal_all_cams_synced.store(true, Ordering::SeqCst);
            } else {
                info!("All expected cameras NOT synchronized.");
            }
        }

        synced_frame.map(SyncFno)
    }

    pub fn get_ros_cam_name(&self, cam_num: CamNum) -> Option<RosCamName> {
        for cci in self.inner.read().ccis.values() {
            if cci.cam_num == cam_num {
                return Some(cci.ros_cam_name.clone());
            }
        }
        None
    }

    pub fn all_ros_cam_names(&self) -> Vec<RosCamName> {
        self.inner
            .read()
            .ccis
            .values()
            .map(|cci| cci.ros_cam_name.clone())
            .collect()
    }

    pub fn http_camserver_info(&self, ros_cam_name: &RosCamName) -> Option<CamHttpServerInfo> {
        self.inner
            .read()
            .ccis
            .get(ros_cam_name)
            .map(|cci| cci.http_camserver_info.clone())
    }

    pub fn cam_num(&self, ros_cam_name: &RosCamName) -> Option<CamNum> {
        let inner = self.inner.read();
        match inner.ccis.get(ros_cam_name) {
            Some(cci) => Some(cci.cam_num),
            None => inner.not_yet_connected.get(ros_cam_name).copied(),
        }
    }

    pub(crate) fn sample(&self) -> Vec<CamInfoRow> {
        self.inner
            .read()
            .ccis
            .values()
            .map(|cci| cci.copy_to_caminfo())
            .collect()
    }

    pub fn len(&self) -> usize {
        self.inner.read().ccis.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.read().ccis.is_empty()
    }
}

impl std::fmt::Debug for ConnectedCamerasManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        f.debug_struct("ConnectedCamerasManager")
            .field("inner", &self.inner)
            .finish()
    }
}

#[test]
fn test_camera_list() {
    let c1 = CameraList::new(&[1, 2, 3, 4]);
    let c2 = CameraList::new(&[4, 3, 2, 1]);
    assert_eq!(c1, c2);

    let c1 = CameraList::new(&[1, 2, 3, 4]);
    let c2 = CameraList::new(&[4, 3, 2]);
    assert!(c1 != c2);

    let c1 = CameraList::new(&[1, 2, 3, 4]);
    let c2 = CameraList::new(&[4, 3, 2, 5]);
    assert!(c1 != c2);
}
