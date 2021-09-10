use crate::connected_camera_manager::CameraList;
use crate::{
    contiguous_stream::Numbered, FrameData, FrameDataAndPoints, MyFloat, SyncFno,
    TimeDataPassthrough,
};

/// Complete set of detected points from all cameras in one frame.
///
/// The point data is raw from the camera and remains distorted.
#[derive(Debug, PartialEq)]
pub(crate) struct BundledAllCamsOneFrameDistorted {
    tdpt: TimeDataPassthrough,
    cameras: CameraList,
    inner: Vec<FrameDataAndPoints>,
}

impl Numbered for BundledAllCamsOneFrameDistorted {
    #[inline]
    fn number(&self) -> u64 {
        self.frame().0
    }
    fn new_empty(number: u64) -> Self {
        Self {
            tdpt: TimeDataPassthrough {
                frame: SyncFno(number),
                timestamp: None,
            },
            cameras: CameraList::new(&[]),
            inner: vec![],
        }
    }
}

/// Undistorted data from all cameras, single frame.
pub(crate) struct BundledAllCamsOneFrameUndistorted {
    pub(crate) tdpt: TimeDataPassthrough,
    pub(crate) inner: Vec<OneCamOneFrameUndistorted>,
    pub(crate) orig_distorted: Vec<FrameDataAndPoints>,
}

impl BundledAllCamsOneFrameUndistorted {
    #[inline]
    pub(crate) fn frame(&self) -> SyncFno {
        self.tdpt.synced_frame()
    }

    pub(crate) fn pretty_format<W: std::io::Write>(
        &self,
        mut f: W,
        indent: usize,
    ) -> std::io::Result<()> {
        // let i0 = std::str::from_utf8(vec![b" "; indent].as_slice()).unwrap();
        let i0 = " ".repeat(indent);
        writeln!(
            &mut f,
            "{}BundledBundledAllCamsOneFrameUndistorted, frame {}",
            i0, self.tdpt.frame.0
        )?;
        for cam_data in self.inner.iter() {
            writeln!(&mut f, "{}  frame {}", i0, self.tdpt.frame.0)?;
            cam_data.pretty_format(&mut f, indent + 4)?;
        }
        // writeln!(&mut f, "{}  frame {}", i0, stdpt.frame.0)?;
        Ok(())
    }
}

/// Undistorted data from a single camera, single frame (multiple detections).
#[derive(Debug, Clone)]
pub(crate) struct Undistorted {
    pub(crate) idx: u8,
    pub(crate) x: f64,
    pub(crate) y: f64,
}

/// Undistorted data from a single camera, single frame (multiple detections).
#[derive(Debug)]
pub(crate) struct OneCamOneFrameUndistorted {
    pub(crate) frame_data: FrameData,
    pub(crate) undistorted: Vec<Undistorted>,
}

impl OneCamOneFrameUndistorted {
    pub(crate) fn pretty_format<W: std::io::Write>(
        &self,
        mut f: W,
        indent: usize,
    ) -> std::io::Result<()> {
        // let i0 = std::str::from_utf8(vec![b" "; indent].as_slice()).unwrap();
        let i0 = " ".repeat(indent);
        for pt in self.undistorted.iter() {
            writeln!(&mut f, "{} {:?}", i0, pt)?;
            // cam_data.pretty_format(f, indent+4);
        }
        Ok(())
    }
}

impl BundledAllCamsOneFrameDistorted {
    /// A new (likely incomplete) BundledAllCamsOneFrameDistorted from first packet.
    pub(crate) fn new(initial: FrameDataAndPoints) -> Self {
        let frame = initial.frame_data.synced_frame;
        let timestamp = initial.frame_data.trigger_timestamp.clone();
        let tdpt = TimeDataPassthrough { frame, timestamp };
        let mut result = Self {
            tdpt,
            cameras: CameraList {
                inner: std::collections::BTreeSet::new(),
            },
            inner: vec![],
        };
        result.push(initial);
        result
    }

    #[cfg(test)]
    pub(crate) fn num_cameras(&self) -> usize {
        self.cameras.inner.len()
    }

    #[inline]
    pub(crate) fn frame(&self) -> SyncFno {
        self.tdpt.synced_frame()
    }

    pub(crate) fn push(&mut self, mut fdp: FrameDataAndPoints) {
        // remove all NaN points from further consideration
        let no_nans = fdp
            .points
            .into_iter()
            .filter(|x| !x.pt.x0_abs.is_nan())
            .collect();
        fdp.points = no_nans;
        let is_new = self.cameras.inner.insert(fdp.frame_data.cam_num.0);
        if !is_new {
            panic!(
                "Received data twice: camera={}, orig frame={}. \
                new frame={}",
                fdp.frame_data.cam_name,
                self.frame().0,
                fdp.frame_data.synced_frame.0
            );
        }
        if self.tdpt.timestamp.is_none() {
            self.tdpt.timestamp = fdp.frame_data.trigger_timestamp.clone();
        }
        self.inner.push(fdp)
    }

    pub(crate) fn cameras(&self) -> &CameraList {
        &self.cameras
    }

    pub(crate) fn undistort(
        self,
        recon: &flydra_mvg::FlydraMultiCameraSystem<MyFloat>,
    ) -> BundledAllCamsOneFrameUndistorted {
        let inner: Vec<OneCamOneFrameUndistorted> =
            self.inner.iter().map(|x| do_undistort(x, recon)).collect();
        let orig_distorted = self.inner;
        debug_assert!(inner.len() == orig_distorted.len());
        let tdpt = self.tdpt;
        BundledAllCamsOneFrameUndistorted {
            tdpt,
            inner,
            orig_distorted,
        }
    }
}

/// Convert a raw data from single camera, single frame to undistorted version.
fn do_undistort(
    distorted: &FrameDataAndPoints,
    recon: &flydra_mvg::FlydraMultiCameraSystem<MyFloat>,
) -> OneCamOneFrameUndistorted {
    let opt_cam = recon.cam_by_name(distorted.frame_data.cam_name.as_str());
    let undistorted: Vec<Undistorted> = if let Some(cam) = opt_cam {
        distorted
            .points
            .iter()
            .map(|x| {
                let pt = &x.pt;
                let undist = cam.undistort(&mvg::DistortedPixel {
                    coords: nalgebra::Point2::new(pt.x0_abs, pt.y0_abs),
                });
                Undistorted {
                    idx: x.idx,
                    x: undist.coords.x,
                    y: undist.coords.y,
                }
            })
            .collect()
    } else {
        // no calibration for this camera - cannot contribute to 3D
        vec![]
    };

    OneCamOneFrameUndistorted {
        frame_data: distorted.frame_data.clone(),
        undistorted,
    }
}
