use std::{collections::BTreeMap, sync::Arc};

use nalgebra::Vector3;
use ncollide3d::shape::Plane;

use crate::{tracking_core::HypothesisTest, CamAndDist, HypothesisTestResult};
use flydra_types::{MyFloat, RosCamName, TrackingParams};

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
        good_points: &BTreeMap<RosCamName, mvg::DistortedPixel<MyFloat>>,
    ) -> Option<HypothesisTestResult> {
        let recon_ref = &self.recon;

        assert!(good_points.len() < 2, "cannot have >1 camera");

        if let Some((cam_name, xy)) = good_points.iter().next() {
            let z0 = Plane::new(Vector3::z_axis()); // build a plane from its center and normal, plane z==0 here.
            let eye = nalgebra::Isometry3::identity();

            let cam = recon_ref.cam_by_name(cam_name.as_str()).unwrap();

            {
                let air_ray = cam.project_distorted_pixel_to_ray(xy);
                let solid = false; // will intersect either side of plane

                use ncollide3d::query::RayCast;
                let opt_surface_pt_toi: Option<MyFloat> =
                    z0.toi_with_ray(&eye, &air_ray, std::f64::MAX, solid);

                if let Some(toi) = opt_surface_pt_toi {
                    let mut surface_pt = air_ray.origin + air_ray.dir * toi;
                    // Due to numerical error, Z is not exactly zero. Here
                    // we clamp it to zero.
                    surface_pt.coords[2] = nalgebra::zero();
                    let cams_and_reproj_dist = vec![CamAndDist {
                        ros_cam_name: cam_name.clone(),
                        reproj_dist: 0.0,
                    }];
                    return Some(HypothesisTestResult {
                        coords: surface_pt,
                        cams_and_reproj_dist,
                    });
                }
            }
        }
        None
    }
}
