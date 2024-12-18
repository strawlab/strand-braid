use std::{collections::BTreeMap, sync::Arc};
use tracing::error;

use flydra_types::{RawCamName, TrackingParams};

use mvg::{MvgError, PointWorldFrameWithSumReprojError};

use crate::{
    safe_u8, set_of_subsets, tracking_core::HypothesisTest, CamAndDist, HypothesisTestResult,
    MyFloat,
};

const HTEST_MAX_N_CAMS: u8 = 3;

type CamComboKey = RawCamName;
type CamComboList = Vec<Vec<RawCamName>>;

#[derive(Clone)]
pub(crate) struct NewObjectTestFull3D {
    cam_combinations_by_size: BTreeMap<u8, CamComboList>,
    recon: flydra_mvg::FlydraMultiCameraSystem<MyFloat>,
    params: Arc<TrackingParams>,
}

impl NewObjectTestFull3D {
    pub(crate) fn new(
        recon: flydra_mvg::FlydraMultiCameraSystem<MyFloat>,
        params: Arc<TrackingParams>,
    ) -> Self {
        {
            let mut cam_combinations_by_size = BTreeMap::new();

            {
                let mut useful_cams = BTreeMap::new();
                for raw_cam_name in recon.cam_names() {
                    let name = RawCamName::new(raw_cam_name.to_string());
                    let k: CamComboKey = name;
                    useful_cams.insert(k, ());
                }
                let cam_combinations_btree = set_of_subsets(&useful_cams);
                let cam_combinations: CamComboList = cam_combinations_btree
                    .into_iter()
                    .map(|v| v.into_iter().collect())
                    .collect();
                for cc in cam_combinations.iter() {
                    let size = safe_u8(cc.len());
                    if (2..=HTEST_MAX_N_CAMS).contains(&size) {
                        let size_entry = &mut cam_combinations_by_size
                            .entry(size)
                            .or_insert_with(Vec::new);
                        size_entry.push(cc.clone());
                    }
                }
            }

            Self {
                cam_combinations_by_size,
                params,
                recon,
            }
        }
    }
}

impl HypothesisTest for NewObjectTestFull3D {
    /// Use hypothesis testing algorithm to find best 3D point.
    ///
    /// Finds combination of cameras which uses the most number of cameras
    /// while minimizing mean reprojection error. Algorithm used accepts
    /// any camera combination with reprojection error less than the
    /// some acceptable distance.
    ///
    /// Returns at most a single object.
    ///
    /// We can safely make the assumption that all incoming data is from the same
    /// framenumber and timestamp.
    fn hypothesis_test(
        &self,
        good_points: &BTreeMap<RawCamName, mvg::DistortedPixel<MyFloat>>,
    ) -> Option<HypothesisTestResult> {
        // TODO: convert this to use undistorted points and then remove
        // orig_distorted, also from the structure it is in.

        let hypothesis_test_params =
            self.params.hypothesis_test_params.as_ref().expect(
                "calling NewObjectTestFull3D:hypothesis_test() without hypothesis_test_params",
            );

        let minimum_number_of_cameras = hypothesis_test_params.minimum_number_of_cameras;
        let hypothesis_test_max_acceptable_error =
            hypothesis_test_params.hypothesis_test_max_acceptable_error;

        let mut best_overall: Option<(
            PointWorldFrameWithSumReprojError<MyFloat>,
            Vec<CamComboKey>,
        )> = None;

        for n_cams in 2..(HTEST_MAX_N_CAMS + 1) {
            if n_cams < minimum_number_of_cameras {
                continue;
            }

            // Calculate the least reprojection error starting with all
            // possible combinations of 2 cameras, then start increasing
            // the number of cameras used.  For each number of cameras,
            // determine if there exists a combination with an acceptable
            // reprojection error.

            let combos = match self.cam_combinations_by_size.get(&n_cams) {
                Some(combos) => combos,
                None => {
                    // We are probably here because there are only 2 cameras connected
                    // and thus self.cam_combinations_by_size will have only an entry
                    // for the key '2', but in this loop n_cams can be more.
                    continue;
                }
            };

            let mut best_solution_so_far: Option<(
                PointWorldFrameWithSumReprojError<MyFloat>,
                Vec<CamComboKey>,
            )> = None;

            for cams_used in combos.iter() {
                let mut missing_cam_data = false;
                let mut points = Vec::with_capacity(cams_used.len());
                for cam_name in cams_used {
                    if let Some(pt) = good_points.get(cam_name) {
                        let new_pt = pt.clone();
                        points.push((cam_name.as_str().to_string(), new_pt));
                    } else {
                        missing_cam_data = true;
                        break;
                    }
                }

                if missing_cam_data {
                    continue;
                }

                let data = match self.recon.find3d_and_cum_reproj_dist_distorted(&points) {
                    Ok(data) => data,
                    Err(err) => {
                        if let flydra_mvg::FlydraMvgError::MvgError(MvgError::SvdFailed) = err {
                            error!("failed SVD in find3d with points {:?}", points);
                            continue;
                        }
                        error!("failed find3d {}", err);
                        return None;
                    }
                };

                best_solution_so_far = match best_solution_so_far {
                    Some((bssf, best_cams_so_far)) => {
                        if data.cum_reproj_dist < bssf.cum_reproj_dist {
                            // This new solution is better, keep it.
                            Some((data, cams_used.clone()))
                        } else {
                            // The previous best remains best.
                            Some((bssf, best_cams_so_far))
                        }
                    }
                    None => {
                        // No previous solutions, keep this one.
                        Some((data, cams_used.clone()))
                    }
                };
            }

            if let Some((bssf, best_cams_so_far)) = best_solution_so_far {
                if bssf.mean_reproj_dist > hypothesis_test_max_acceptable_error {
                    // Not possible for fitting N+1 points have less error than with N points,
                    // so abort early.
                    break;
                } else {
                    // We are iterating from least to most cameras. Anything within
                    // our acceptable distance with more cameras is here favored over
                    // any solution (with lower mean reprojection error) from more cams.
                    best_overall = Some((bssf, best_cams_so_far));
                }
            }
        }

        best_overall.map(|(bssf, cams_used)| {
            // Build CamAndDist struct for each camera.
            debug_assert!(cams_used.len() == bssf.reproj_dists.len());
            let cams_and_reproj_dist = cams_used
                .iter()
                .zip(bssf.reproj_dists.iter())
                .map(|(ros_cam_name, reproj_dist)| CamAndDist {
                    raw_cam_name: ros_cam_name.clone(),
                    reproj_dist: *reproj_dist,
                })
                .collect();
            HypothesisTestResult {
                coords: bssf.point.coords,
                cams_and_reproj_dist,
            }
        })
    }
}
