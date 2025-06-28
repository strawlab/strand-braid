use std::{collections::BTreeMap, sync::Arc};

use crate::{tracking_core::HypothesisTest, CamAndDist, HypothesisTestResult};
use braid_types::{MyFloat, RawCamName, TrackingParams};

#[derive(Clone)]
pub(crate) struct NewObjectTestFlat3D {
    recon: flydra_mvg::FlydraMultiCameraSystem<MyFloat>,
}

impl NewObjectTestFlat3D {
    pub(crate) fn new(
        recon: flydra_mvg::FlydraMultiCameraSystem<MyFloat>,
        _params: Arc<TrackingParams>,
    ) -> Self {
        // `_params` is unused but required to have the same type signature as
        // the 3d version.
        Self { recon }
    }
}

impl HypothesisTest for NewObjectTestFlat3D {
    fn hypothesis_test(
        &self,
        good_points: &BTreeMap<RawCamName, braid_mvg::DistortedPixel<MyFloat>>,
    ) -> Option<HypothesisTestResult> {
        let recon_ref = &self.recon;
        assert!(good_points.len() < 2, "cannot have >1 camera for Flat3D");
        if let Some((cam_name, xy)) = good_points.iter().next() {
            let cam = recon_ref.cam_by_name(cam_name.as_str()).unwrap();
            if let Some(surface_pt) = crate::flat_2d::distorted_2d_to_flat_3d(&cam, xy) {
                let cams_and_reproj_dist = vec![CamAndDist {
                    raw_cam_name: cam_name.clone(),
                    reproj_dist: 0.0,
                }];
                return Some(HypothesisTestResult {
                    coords: surface_pt,
                    cams_and_reproj_dist,
                });
            }
        }
        None
    }
}
