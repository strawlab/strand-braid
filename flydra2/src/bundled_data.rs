use std::collections::BTreeMap;

use flydra_types::{MiniArenaConfig, RosCamName};
use nalgebra::Point2;

use crate::connected_camera_manager::CameraList;
use crate::mini_arenas::MiniArenaImage;
use crate::NumberedRawUdpPoint;
use crate::{
    contiguous_stream::Numbered, FrameDataAndPoints, MyFloat, SyncFno, TimeDataPassthrough,
};

#[derive(Clone, Debug)]
pub(crate) struct MiniArenaPointPerCam {
    pub(crate) undistorted: Undistorted,
    pub(crate) numbered_raw_udp_point: NumberedRawUdpPoint,
}

// impl MiniArenaPointPerCam {
//     pub(crate) fn pretty_format<W: std::io::Write>(
//         &self,
//         mut f: W,
//         indent: usize,
//     ) -> std::io::Result<()> {
//         let i0 = " ".repeat(indent);
//         writeln!(
//             f,
//             "{}MiniArenaPointPerCam{{ undistorted: ({}, {})}}",
//             i0, self.undistorted.x, self.undistorted.y
//         )?;
//         Ok(())
//     }
// }

// pub(crate) struct FailedData {
//     frame_data: FrameData,
//     points: Vec<MiniArenaPointPerCam>,
// }

#[derive(Debug, Default)]
pub(crate) struct PerMiniArenaAllCamsOneFrameUndistorted {
    pub(crate) per_cam: BTreeMap<RosCamName, Vec<MiniArenaPointPerCam>>,
}

// impl PerMiniArenaAllCamsOneFrameUndistorted {
//     pub(crate) fn pretty_format<W: std::io::Write>(
//         &self,
//         mut f: W,
//         indent: usize,
//         tdpt: &TimeDataPassthrough,
//     ) -> std::io::Result<()> {
//         // let i0 = std::str::from_utf8(vec![b" "; indent].as_slice()).unwrap();
//         let i0 = " ".repeat(indent);
//         writeln!(
//             f,
//             "{}PerMiniArenaAllCamsOneFrameUndistorted, frame {}",
//             i0, tdpt.frame.0
//         )?;
//         for (cam_name, pts) in self.per_cam.iter() {
//             writeln!(f, "{} {} frame {}", i0, cam_name.as_str(), tdpt.frame.0)?;

//             for pt in pts.iter() {
//                 pt.pretty_format(&mut f, indent + 4)?;
//             }
//         }
//         // writeln!(&mut f, "{}  frame {}", i0, stdpt.frame.0)?;
//         Ok(())
//     }
// }

#[derive(Debug)]
/// Undistorted data from all cameras, single frame.
pub(crate) struct BundledAllCamsOneFrameUndistorted {
    pub(crate) tdpt: TimeDataPassthrough,
    pub(crate) per_mini_arena: Vec<PerMiniArenaAllCamsOneFrameUndistorted>,
}

// impl BundledAllCamsOneFrameUndistorted {
//     pub(crate) fn pretty_format<W: std::io::Write>(
//         &self,
//         mut f: W,
//         indent: usize,
//     ) -> std::io::Result<()> {
//         // let i0 = std::str::from_utf8(vec![b" "; indent].as_slice()).unwrap();
//         let i0 = " ".repeat(indent);
//         writeln!(
//             f,
//             "{}BundledBundledAllCamsOneFrameUndistorted, frame {}",
//             i0, self.tdpt.frame.0
//         )?;
//         for cam_data in self.per_cam_all_undistorted.iter() {
//             writeln!(f, "{}  frame {}", i0, self.tdpt.frame.0)?;
//             cam_data.pretty_format(&mut f, indent + 4)?;
//         }
//         // writeln!(&mut f, "{}  frame {}", i0, stdpt.frame.0)?;
//         Ok(())
//     }
// }

/// Undistorted data from a single camera, single frame (multiple detections).
#[derive(Debug, Clone)]
pub(crate) struct Undistorted {
    pub(crate) idx: u8,
    pub(crate) x: f64,
    pub(crate) y: f64,
}

// /// Undistorted data from a single camera, single frame (multiple detections).
// #[derive(Debug)]
// pub(crate) struct OneCamOneFrameUndistorted {
//     pub(crate) frame_data: FrameData,
//     pub(crate) undistorted: Vec<Undistorted>,
// }

// impl OneCamOneFrameUndistorted {
//     pub(crate) fn pretty_format<W: std::io::Write>(
//         &self,
//         mut f: W,
//         indent: usize,
//     ) -> std::io::Result<()> {
//         // let i0 = std::str::from_utf8(vec![b" "; indent].as_slice()).unwrap();
//         let i0 = " ".repeat(indent);
//         for pt in self.undistorted.iter() {
//             writeln!(f, "{} {:?}", i0, pt)?;
//             // cam_data.pretty_format(f, indent+4);
//         }
//         Ok(())
//     }
// }

/// Complete set of detected points from all cameras in one frame.
///
/// The point data is raw from the camera and remains distorted.
#[derive(Debug, PartialEq)]
pub(crate) struct BundledAllCamsOneFrameDistorted {
    tdpt: TimeDataPassthrough,
    cameras: CameraList,
    inner: Vec<FrameDataAndPoints>,
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
        assert!(
            is_new,
            "Received data twice: camera={}, orig frame={}. \
                new frame={}",
            fdp.frame_data.cam_name,
            self.frame().0,
            fdp.frame_data.synced_frame.0
        );
        if self.tdpt.timestamp.is_none() {
            self.tdpt.timestamp = fdp.frame_data.trigger_timestamp.clone();
        }
        self.inner.push(fdp)
    }

    pub(crate) fn cameras(&self) -> &CameraList {
        &self.cameras
    }

    pub(crate) fn undistort_and_split_to_mini_arenas(
        self,
        recon: &flydra_mvg::FlydraMultiCameraSystem<MyFloat>,
        mini_arena_images: &std::collections::BTreeMap<String, MiniArenaImage>,
        mini_arena_config: &MiniArenaConfig,
    ) -> BundledAllCamsOneFrameUndistorted {
        // initialize structure to hold for per-mini-arena detections
        let mut per_mini_arena: Vec<_> = (0..mini_arena_config.len())
            .map(|_| PerMiniArenaAllCamsOneFrameUndistorted::default())
            .collect();

        for distorted in self.inner.into_iter() {
            undistort_points_and_assign_arena(
                distorted,
                recon,
                mini_arena_images,
                &mut per_mini_arena,
            )
        }
        let tdpt = self.tdpt;
        BundledAllCamsOneFrameUndistorted {
            tdpt,
            per_mini_arena,
        }
    }
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

/// Convert multiple points of raw data from single camera, single frame to
/// undistorted version. Saves results in correct item in vector of per-arena
/// data.
fn undistort_points_and_assign_arena(
    distorted_points: FrameDataAndPoints,
    recon: &flydra_mvg::FlydraMultiCameraSystem<MyFloat>,
    mini_arena_images: &BTreeMap<String, MiniArenaImage>,
    per_mini_arena: &mut [PerMiniArenaAllCamsOneFrameUndistorted],
) {
    let cam_name = distorted_points.frame_data.cam_name.clone();
    let opt_cam = recon.cam_by_name(cam_name.as_str());
    if let Some(cam) = opt_cam {
        // We can only undistort if we have camera calibration.
        let mini_arena_image = mini_arena_images.get(cam.name());

        for numbered_raw_udp_point in distorted_points.points.into_iter() {
            let pt = &numbered_raw_udp_point.pt;

            let mini_arena_idx = if let Some(mini_arena_image) = &mini_arena_image {
                // We are using mini arenas
                let xidx = pt.x0_abs.floor() as usize;
                let yidx = pt.y0_abs.floor() as usize;
                mini_arena_image.get_mini_arena(xidx, yidx)
            } else {
                // We are not using mini arenas
                None
            };

            let distorted = mvg::DistortedPixel {
                coords: Point2::new(pt.x0_abs, pt.y0_abs),
            };
            let undist = cam.undistort(&distorted);

            let undistorted = Undistorted {
                idx: numbered_raw_udp_point.idx,
                x: undist.coords.x,
                y: undist.coords.y,
            };

            let index = if let Some(mini_arena_idx) = mini_arena_idx {
                mini_arena_idx.idx()
            } else {
                // We are not using mini arenas but we still want to keep the
                // point. In this case we have a single "mini arena", which
                // actually encompasses the entire image, at index 0.
                0
            };

            let pt = MiniArenaPointPerCam {
                undistorted,
                numbered_raw_udp_point,
            };

            let my_vec = per_mini_arena
                .get_mut(index)
                .unwrap()
                .per_cam
                .entry(cam_name.clone())
                .or_default();
            my_vec.push(pt);
        }
    }
}
