use std::{collections::BTreeMap, sync::Arc};
use tracing::trace;

use nalgebra::core::dimension::{U2, U6};
use nalgebra::{Matrix6, OMatrix, OVector, Point3, RealField, Vector6};

use nalgebra_mvn::MultivariateNormal;

use pretty_print_nalgebra::pretty_print;

use tracking::motion_model_3d_fixed_dt::{MotionModel3D, MotionModel3DFixedDt};

use tracking::flat_motion_model_3d::FlatZZero3DModel;
use tracking::motion_model_3d::ConstantVelocity3DModel;

use adskalman::ObservationModel as ObservationModelTrait;
use adskalman::{StateAndCovariance, TransitionModelLinearNoControl};

use flydra_types::{
    CamNum, DataAssocRow, FlydraFloatTimestampLocal, FlydraRawUdpPoint, KalmanEstimatesRow,
    RosCamName, SyncFno, TrackingParams, Triggerbox,
};

use crate::bundled_data::{MiniArenaPointPerCam, PerMiniArenaAllCamsOneFrameUndistorted};
use crate::{
    mini_arenas::MiniArenaIndex,
    model_server::{SendKalmanEstimatesRow, SendType},
    new_object_test_2d::NewObjectTestFlat3D,
    new_object_test_3d::NewObjectTestFull3D,
    to_world_point, CameraObservationModel, ConnectedCamerasManager, HypothesisTestResult,
    KalmanEstimateRecord, MyFloat, SaveToDiskMsg, TimeDataPassthrough,
};

// -----------------------------------------------------------------------------

#[derive(Debug)]
pub(crate) struct UnusedDataPerArena(PerMiniArenaAllCamsOneFrameUndistorted);

// LivingModel -----------------------------------------------------------------

/// The implementation specifies in what state we are in terms of handling a frame of data.
trait ModelState: std::fmt::Debug {}

/// finished computing one frame, have not started on next
#[derive(Debug)]
struct ModelFrameDone {}

/// motion model has updated prior
#[derive(Debug)]
struct ModelFrameStarted {
    prior: StateAndCovariance<MyFloat, U6>,
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
enum ObservationModel {
    ObservationModelAndLikelihoods(ObservationModelAndLikelihoods),
    NoObservations,
}

#[derive(Debug)]
struct ObservationModelAndLikelihoods {
    /// linearized observation model for a given camera
    observation_model: CameraObservationModel<MyFloat>,
    /// likelihood for all observations from this camera
    likelihoods: nalgebra::RowDVector<f64>,
}

/// has linearized observation model, computed expected value, computed residuals, computed likelihood
#[derive(Debug)]
struct ModelFrameWithObservationLikes {
    /// Vec with one element per camera.
    obs_models_and_likelihoods: Vec<ObservationModel>,
    /// The estimate prior to update from observation.
    prior: StateAndCovariance<MyFloat, U6>,
}

#[derive(Debug)]
struct DataAssocInfo {
    pt_idx: u8,
    cam_num: CamNum,
    /// Reprojection distance. Calculated on undistorted pixel coords.
    reproj_dist: MyFloat,
}

/// have posterior distribution for this object on this frame
#[derive(Debug)]
struct ModelFramePosteriors {
    posterior: StampedEstimate,
    /// data association info to link the original 2d observation as "used" for 3d reconstruction.
    data_assoc_this_timestamp: Vec<DataAssocInfo>,
}

impl ModelFramePosteriors {
    fn covariance_size(&self) -> MyFloat {
        covariance_size(self.posterior.estimate.covariance())
    }
}

fn covariance_size<R: RealField + Copy>(mat: &OMatrix<R, U6, U6>) -> R {
    // XXX should probably use trace/N (mean of variances) or determinant (volume of variance)
    let v1 = [mat[(0, 0)], mat[(1, 1)], mat[(2, 2)]];
    v1.iter()
        .map(|i| i.powi(2))
        .fold(nalgebra::convert(0.0), |acc: R, el| acc + el)
        .sqrt()
}

impl ModelState for ModelFrameDone {}
impl ModelState for ModelFrameStarted {}
impl ModelState for ModelFrameWithObservationLikes {}
impl ModelState for ModelFramePosteriors {}

/// A live model of something we are tracking.
#[derive(Debug)]
struct LivingModel<S: ModelState> {
    /// If not yet visible, number of observations made so far. If visible, this
    /// is `None`.
    gestation_age: Option<u8>,
    /// The state of the model. Storage for stage-specific data.
    state: S,
    /// Current and all past estimates
    posteriors: Vec<StampedEstimate>,
    /// The number of frames (since start_frame) that an observation was made.
    last_observation_offset: usize,
    lmi: LMInner,
}

#[derive(Debug, Clone)]
struct StampedEstimate {
    estimate: StateAndCovariance<MyFloat, U6>,
    tdpt: TimeDataPassthrough,
}

impl StampedEstimate {
    #[inline]
    fn frame(&self) -> SyncFno {
        self.tdpt.synced_frame()
    }

    #[inline]
    fn trigger_timestamp(&self) -> Option<FlydraFloatTimestampLocal<Triggerbox>> {
        self.tdpt.trigger_timestamp()
    }
}

/// Inner data for `LivingModel`
#[derive(Debug, Clone)]
struct LMInner {
    /// The unique object id for this model
    obj_id: u32,
    /// Initial start frame number
    _start_frame: SyncFno,
}

impl LivingModel<ModelFrameStarted> {
    /// linearize the observation model for camera about myself and then project
    /// myself through the linearization to get expected_observation.
    fn compute_expected_observation(
        &self,
        camera: flydra_mvg::MultiCamera<MyFloat>,
        ekf_observation_covariance_pixels: f64,
    ) -> (
        CameraObservationModel<MyFloat>,
        Option<MultivariateNormal<MyFloat, U2>>,
    ) {
        use adskalman::ObservationModel;

        let prior = &self.state.prior;

        // TODO: update to handle water here. See tag "laksdfjasl".
        let undistorted = camera.project_3d_to_pixel(&to_world_point(prior.state()));

        //  - linearize observation_model about prior
        let obs_model = crate::generate_observation_model(
            &camera,
            prior.state(),
            ekf_observation_covariance_pixels,
        )
        .expect("jacobian evaluation");

        //  - compute expected observation through `frame_data.camera` given prior
        let projected_covariance = {
            let h = obs_model.H();
            let ht = obs_model.HT();
            let p = prior.covariance();
            (h * p) * ht
        };

        // Note: in some cases, `projected_covariance` is not positive definite
        // and thus the next step fails. In this case, we then don't create a
        // `MultivariateNormal` and later evaluate the likelihood of any
        // obervation to zero. In theory, it might be better to fix
        // `projected_covariance` to be positive definite, but since this seems
        // to happen so rarely, I didn't bother. I guess in cases where it does
        // happen, the observation is really wacky anyway. If true, even if the
        // issue was frequent, we probably would effectively do the same anyway.

        // Crate a 2D Gaussian centered at our expectation.
        let mvn = MultivariateNormal::from_mean_and_covariance(
            &undistorted.coords.coords,
            &projected_covariance,
        )
        .ok();
        (obs_model, mvn)
    }

    fn compute_observation_likelihoods(
        self,
        arena_bundle: &PerMiniArenaAllCamsOneFrameUndistorted,
        recon: &flydra_mvg::FlydraMultiCameraSystem<MyFloat>,
        ekf_observation_covariance_pixels: f64,
    ) -> LivingModel<ModelFrameWithObservationLikes> {
        // for each camera with data:
        //  - compute likelihood of each real observation given expected observation

        let obs_models_and_likelihoods: Vec<ObservationModel> = arena_bundle
            .per_cam
            .iter()
            .map(|(cam_name, my_points)| {
                // outer loop: cameras

                if my_points.is_empty() {
                    ObservationModel::NoObservations
                } else {
                    let cam = recon.cam_by_name(cam_name.as_str()).unwrap();
                    let (observation_model, eo) =
                        self.compute_expected_observation(cam, ekf_observation_covariance_pixels);

                    let likes: Vec<f64> = if let Some(expected_observation) = eo {
                        trace!(
                            "object {} {} expects ({},{})",
                            self.lmi.obj_id,
                            cam_name,
                            expected_observation.mean()[0],
                            expected_observation.mean()[1]
                        );

                        my_points
                            .iter()
                            .map(|mappc: &MiniArenaPointPerCam| {
                                let pt = &mappc.undistorted;
                                // inner loop: points

                                // Because we keep all points in order (and do not drop
                                // NaNs, for example), we know the resulting vector has
                                // the same ordering and length as the original
                                // Vec<Undistorted> and thus we can index there to get
                                // the point index.

                                // Put our observation into an nalgebra::Vector2 type.
                                let obs = OVector::<_, U2>::new(pt.x, pt.y);

                                // Compute the likelihood of this observation given our model.
                                let likelihood = expected_observation.pdf(&obs.transpose())[0];

                                nalgebra::convert(likelihood)
                            })
                            .collect()
                    } else {
                        vec![0.0; my_points.len()]
                    };
                    // trace!("incoming points: {:?}", my_points);
                    // trace!("likelihoods: {:?}", likes);
                    ObservationModel::ObservationModelAndLikelihoods(
                        ObservationModelAndLikelihoods {
                            observation_model,
                            likelihoods: nalgebra::RowDVector::from_iterator(likes.len(), likes),
                        },
                    )
                }
            })
            .collect();

        LivingModel {
            gestation_age: self.gestation_age,
            state: ModelFrameWithObservationLikes {
                obs_models_and_likelihoods,
                prior: self.state.prior,
            },
            posteriors: self.posteriors,
            last_observation_offset: self.last_observation_offset,
            lmi: self.lmi,
        }
    }
}

#[inline]
fn get_kalman_estimates_row(obj_id: u32, posterior: &StampedEstimate) -> KalmanEstimatesRow {
    let state = posterior.estimate.state();
    let p = posterior.estimate.covariance();
    let timestamp = posterior.trigger_timestamp();

    KalmanEstimatesRow {
        obj_id,
        frame: posterior.frame(),
        timestamp,
        x: state[0],
        y: state[1],
        z: state[2],
        xvel: state[3],
        yvel: state[4],
        zvel: state[5],
        P00: p[(0, 0)],
        P01: p[(0, 1)],
        P02: p[(0, 2)],
        P11: p[(1, 1)],
        P12: p[(1, 2)],
        P22: p[(2, 2)],
        P33: p[(3, 3)],
        P44: p[(4, 4)],
        P55: p[(5, 5)],
    }
}

impl LivingModel<ModelFramePosteriors> {
    fn finish_frame(
        mut self,
        num_observations_to_visibility: u8,
    ) -> (
        LivingModel<ModelFrameDone>,
        Vec<(SendType, TimeDataPassthrough)>,
        Vec<SaveToDiskMsg>,
    ) {
        let mut result_messages = Vec::new();
        let mut result_save_msgs = Vec::new();

        // save data -------------------------------
        let obj_id = self.lmi.obj_id;
        let frame = self.state.posterior.frame();
        let mut new_gestation_age = self.gestation_age;

        let r: Vec<f64> = self
            .state
            .data_assoc_this_timestamp
            .iter()
            .map(|x| x.reproj_dist)
            .collect();
        let cum_reproj = mvg::vec_sum(&r);
        let n_pts = r.len();
        let mean_reproj_dist_100x = if n_pts == 0 {
            None
        } else {
            let mut mean_reproj_dist_100x = (100.0 * cum_reproj / r.len() as f64).round() as u64;
            if mean_reproj_dist_100x == 0 {
                mean_reproj_dist_100x = 1;
            }
            Some(mean_reproj_dist_100x)
        };

        let data_assoc_rows: Vec<_> = self
            .state
            .data_assoc_this_timestamp
            .into_iter()
            .map(|da_info| DataAssocRow {
                obj_id,
                frame,
                cam_num: da_info.cam_num,
                pt_idx: da_info.pt_idx,
            })
            .collect();

        let record = get_kalman_estimates_row(self.lmi.obj_id, &self.state.posterior);
        let send_kalman_estimate_row: SendKalmanEstimatesRow = record.clone().into();

        // Save kalman estimates and data association data to disk iff there
        // were one or more observations.
        if !data_assoc_rows.is_empty() {
            // We had an observation.

            let mut do_become_visible = false;
            if let Some(n_obs) = &new_gestation_age {
                // Update our gestation age with another observation.
                let new_num_observations = n_obs + 1;
                new_gestation_age = if new_num_observations > num_observations_to_visibility {
                    // We now have enough observations to become visible. (We
                    // have reached the gestation period.)
                    do_become_visible = true;
                    None
                } else {
                    // We do not yet have enough observations to become visible.
                    Some(n_obs + 1)
                };
            }

            if do_become_visible {
                result_messages.push((
                    SendType::Birth(send_kalman_estimate_row.clone()),
                    self.state.posterior.tdpt.clone(),
                ));
            }

            // Handle backlog of frames with no observations.

            // We used to allow skipping data (i.e. not saving every frame when
            // there was no observation). But now this is no longer true. The
            // code here is a bit confusing due to that history. Now we can
            // assume that `self.posteriors.len()` is the number of frames we
            // have seen until now (whether or not they had observations). So
            // probably we could simply delete the following block after testing
            // in real world use that `start_idx==end_idx`.

            if new_gestation_age.is_none() {
                // We are not in gestation period, so we are visible and should
                // save data.

                // Also save previous kalman estimates up to now so that the
                // record on disk is continuous with no frames skipped, even when
                // an observation was missing.

                // Calculate backlog of posterior estimates not yet saved to disk.
                let start_idx = self.last_observation_offset + 1;
                let end_idx = self.posteriors.len();
                for idx in start_idx..end_idx {
                    let posterior = &self.posteriors[idx];

                    // println!("saving row with no observations {} {}", self.lmi.obj_id, fno);
                    // println!("   start idx end {} {} {}", start_idx, idx, end_idx);
                    let no_obs_record = get_kalman_estimates_row(self.lmi.obj_id, posterior);
                    let msg = SaveToDiskMsg::KalmanEstimate(KalmanEstimateRecord {
                        record: no_obs_record,
                        data_assoc_rows: vec![],
                        mean_reproj_dist_100x: None,
                    });
                    result_save_msgs.push(msg);
                }

                // Now save the final row (with observations).
                // println!("saving row with observations {} {}", self.lmi.obj_id, frame.0);
                result_save_msgs.push(SaveToDiskMsg::KalmanEstimate(KalmanEstimateRecord {
                    record,
                    data_assoc_rows,
                    mean_reproj_dist_100x,
                }));
            }
            self.last_observation_offset = self.posteriors.len();
        }

        if new_gestation_age.is_none() {
            // We are not in gestation period, so we are visible and should
            // save data.

            // Regardless of whether there was a new observation, send the updated
            // posterior estimate to the network.

            // Here is the realtime pose output when using the HTTP
            // model server.
            result_messages.push((
                SendType::Update(send_kalman_estimate_row.clone()),
                self.state.posterior.tdpt.clone(),
            ));
        }

        // convert to ModelFrameDone -------------------------------

        // current posterior is appended to list of posteriors.
        let mut posteriors = self.posteriors;
        posteriors.push(self.state.posterior);
        (
            LivingModel {
                gestation_age: new_gestation_age,
                state: ModelFrameDone {},
                posteriors,
                last_observation_offset: self.last_observation_offset,
                lmi: self.lmi,
            },
            result_messages,
            result_save_msgs,
        )
    }
}

// ModelCollection -------------------------------------------------------------

pub(crate) trait CollectionState: std::fmt::Debug {}

#[derive(Debug)]
pub(crate) struct CollectionFrameDone {
    models: Vec<LivingModel<ModelFrameDone>>,
}

#[derive(Debug)]
pub(crate) struct CollectionFrameStarted {
    models: Vec<LivingModel<ModelFrameStarted>>,
}

#[derive(Debug)]
pub(crate) struct CollectionFrameWithObservationLikes {
    models_with_obs_likes: Vec<LivingModel<ModelFrameWithObservationLikes>>,
    // bundle: BundledAllCamsOneFrameUndistorted,
}

#[derive(Debug)]
pub(crate) struct CollectionFramePosteriors {
    models_with_posteriors: Vec<LivingModel<ModelFramePosteriors>>,
}

impl CollectionState for CollectionFrameDone {}
impl CollectionState for CollectionFrameStarted {}
impl CollectionState for CollectionFrameWithObservationLikes {}
impl CollectionState for CollectionFramePosteriors {}

impl<S> std::fmt::Debug for ModelCollection<S>
where
    S: CollectionState + std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        f.debug_struct("ModelCollection").finish()
    }
}

pub(crate) trait HypothesisTest: Send + dyn_clone::DynClone {
    fn hypothesis_test(
        &self,
        good_points: &BTreeMap<RosCamName, mvg::DistortedPixel<MyFloat>>,
    ) -> Option<HypothesisTestResult>;
}

dyn_clone::clone_trait_object!(HypothesisTest);

pub(crate) fn initialize_model_collection(
    params: Arc<TrackingParams>,
    recon: flydra_mvg::FlydraMultiCameraSystem<MyFloat>,
    fps: f32,
    cam_manager: ConnectedCamerasManager,
    mini_arena_idx: MiniArenaIndex,
) -> ModelCollection<CollectionFrameDone> {
    let motion_noise_scale = params.motion_noise_scale;
    let dt = 1.0 / fps as f64;

    let (new_obj, motion_model) = if params.hypothesis_test_params.is_some() {
        // full 3d tracking
        let new_obj = NewObjectTestFull3D::new(recon.clone(), params.clone());
        let motion_model_generator = ConstantVelocity3DModel::new(motion_noise_scale);
        (
            Box::new(new_obj) as Box<dyn HypothesisTest + Send + Sync>,
            motion_model_generator.calc_for_dt(dt),
        )
    } else {
        // "flat 3d" (2d) tracking
        let new_obj = NewObjectTestFlat3D::new(recon.clone(), params.clone());
        let motion_model_generator = FlatZZero3DModel::new(motion_noise_scale);
        (
            Box::new(new_obj) as Box<dyn HypothesisTest + Send + Sync>,
            motion_model_generator.calc_for_dt(dt),
        )
    };

    ModelCollection {
        state: CollectionFrameDone { models: vec![] },
        mcinner: MCInner {
            mini_arena_idx,
            params,
            recon,
            new_obj,
            motion_model,
            cam_manager,
        },
    }
}

#[derive(Clone)]
pub(crate) struct ModelCollection<S: CollectionState> {
    state: S,
    pub(crate) mcinner: MCInner,
}

#[derive(Clone)]
pub(crate) struct MCInner {
    pub(crate) mini_arena_idx: MiniArenaIndex,
    params: Arc<TrackingParams>,
    pub(crate) recon: flydra_mvg::FlydraMultiCameraSystem<MyFloat>,
    new_obj: Box<dyn HypothesisTest + Send + Sync>,
    motion_model: MotionModel3DFixedDt<MyFloat>,
    cam_manager: ConnectedCamerasManager,
}

impl ModelCollection<CollectionFrameDone> {
    #[tracing::instrument]
    pub(crate) fn predict_motion(self) -> ModelCollection<CollectionFrameStarted> {
        let mcinner = self.mcinner;
        let models = self
            .state
            .models
            .into_iter()
            .map(|x| {
                let last = &x.posteriors[x.posteriors.len() - 1];
                let prior = mcinner.motion_model.predict(&last.estimate);
                LivingModel {
                    gestation_age: x.gestation_age,
                    state: ModelFrameStarted { prior },
                    posteriors: x.posteriors,
                    last_observation_offset: x.last_observation_offset,
                    lmi: x.lmi,
                }
            })
            .collect();
        ModelCollection {
            state: CollectionFrameStarted { models },
            mcinner,
        }
    }
}

impl ModelCollection<CollectionFrameStarted> {
    #[tracing::instrument]
    pub(crate) fn compute_observation_likes(
        self,
        tdpt: &TimeDataPassthrough,
        arena_bundle: &PerMiniArenaAllCamsOneFrameUndistorted,
    ) -> ModelCollection<CollectionFrameWithObservationLikes> {
        trace!(
            "---- arena {} computing observation likelihoods from frame {} -----",
            self.mcinner.mini_arena_idx.idx(),
            tdpt.frame.0
        );

        let (mcinner, state) = (self.mcinner, self.state);
        let models_with_obs_likes: Vec<LivingModel<_>> = state
            .models
            .into_iter()
            .map(|x| {
                x.compute_observation_likelihoods(
                    arena_bundle,
                    &mcinner.recon,
                    mcinner.params.ekf_observation_covariance_pixels,
                )
            })
            .collect();
        ModelCollection {
            state: CollectionFrameWithObservationLikes {
                models_with_obs_likes,
            },
            mcinner,
        }
    }
}

impl ModelCollection<CollectionFrameWithObservationLikes> {
    #[tracing::instrument]
    pub(crate) fn solve_data_association_and_update(
        self,
        tdpt: &TimeDataPassthrough,
        arena_bundle: PerMiniArenaAllCamsOneFrameUndistorted,
    ) -> (
        ModelCollection<CollectionFramePosteriors>,
        UnusedDataPerArena,
    ) {
        // We have likelihoods for all objects on all cameras for each point.

        // Do something like the hungarian algorithm.

        if self.state.models_with_obs_likes.is_empty() {
            // Short-circuit stuff below when no data.
            let state = CollectionFramePosteriors {
                models_with_posteriors: vec![],
            };
            let mcinner = self.mcinner;
            (
                ModelCollection { state, mcinner },
                UnusedDataPerArena(arena_bundle),
            )
        } else {
            // loop camera-by-camera to get MxN matrix of live model and num observations.
            // currently, we can loop by model and then (obs_num x cam_num)

            // we will fill this cam-by-cam
            let mut unused_bundle_per_cam = BTreeMap::new();

            // Initialize updated models in which no observation
            // was used and thus the posteriors are just the priors.
            let (mut models_with_posteriors, old_states) = self
                .state
                .models_with_obs_likes
                .into_iter()
                .map(|old_model| {
                    // Destructure old model into constituent parts.
                    let LivingModel {
                        gestation_age,
                        state,
                        posteriors,
                        last_observation_offset,
                        lmi,
                    } = old_model;

                    // Create new model with new state (type
                    // `ModelFramePosteriors`), moving in the relevant parts
                    // from the old model.
                    let new_model = LivingModel {
                        gestation_age,
                        state: ModelFramePosteriors {
                            posterior: StampedEstimate {
                                estimate: state.prior.clone(), // just the prior initially
                                tdpt: tdpt.clone(),
                            },
                            data_assoc_this_timestamp: vec![], // no observations yet
                        },
                        posteriors,
                        last_observation_offset,
                        lmi,
                    };

                    // Return the new model and the old state
                    (new_model, state)
                })
                .unzip::<_, _, Vec<_>, Vec<_>>();

            let zero = nalgebra::convert(0.0);

            // outer loop here iterates over the per-camera data, So we compute
            // the "wantedness" matrix for each camera one at a time, considering
            // the models and set of observations for this camera.
            for (cam_idx, (cam_name, arena_data)) in arena_bundle.per_cam.into_iter().enumerate() {
                //     let (frame_cam_points, fdp): (&OneCamOneFrameUndistorted, &FrameDataAndPoints) =
                //         per_cam;

                if arena_data.is_empty() {
                    continue;
                }

                let cam_num = self.mcinner.cam_manager.cam_num(&cam_name).unwrap();

                trace!(
                    "camera {} ({}): {} points",
                    cam_name,
                    cam_num,
                    arena_data.len()
                );

                // Get pre-computed likelihoods for each model for this camera.
                // There are N elements in the outer vector, one for each model
                // and M elements in each inner container, corresponding to the M
                // detected points for this camera on this frame.
                let wantedness = old_states
                    .iter()
                    .map(|model| match &model.obs_models_and_likelihoods[cam_idx] {
                        ObservationModel::ObservationModelAndLikelihoods(oml) => {
                            oml.likelihoods.clone()
                        }
                        ObservationModel::NoObservations => {
                            nalgebra::RowDVector::zeros(arena_data.len())
                        }
                    })
                    .collect::<Vec<_>>();

                // debug!("wantedness1 {:?}", wantedness);

                let mut wantedness =
                    nalgebra::OMatrix::<f64, nalgebra::Dyn, nalgebra::Dyn>::from_rows(
                        wantedness.as_slice(),
                    );

                debug_assert!(arena_data.len() == wantedness.ncols());

                trace!(
                    "wantedness (N x M where N is num live models and M is num points)\n{}",
                    pretty_print!(wantedness)
                );

                // Consume all incoming points either into a observation or into unconsumed_points.

                let mut unused_col_idxs =
                    std::collections::BTreeSet::from_iter(0..wantedness.ncols());

                // Iterate over the models
                for (row_idx, next_model) in models_with_posteriors.iter_mut().enumerate() {
                    // Each incoming point can only be assigned to a single
                    // model, so iterate over columns and select the best row.
                    // Also, each model can only get a single observation (from
                    // this camera).
                    let likelihoods = wantedness.row(row_idx); // extract likelihood for all points
                    let best_col = arg_max_col(&likelihoods.iter().copied().collect::<Vec<_>>()); // select best point
                    trace!("row_idx {}, best_col {:?}", row_idx, best_col);

                    if let Some((best_idx, best_wantedness)) = best_col {
                        if best_wantedness > self.mcinner.params.accept_observation_min_likelihood {
                            // don't take unwanted point
                            unused_col_idxs.remove(&best_idx);

                            // this point can no longer be used for other models
                            for tmp_i in 0..wantedness.nrows() {
                                wantedness[(tmp_i, best_idx)] = zero;
                            }

                            let this_pt = &arena_data[best_idx];
                            let undist_pt = &this_pt.undistorted;
                            trace!(
                                "object {} is accepting undistorted point {:?}",
                                next_model.lmi.obj_id,
                                undist_pt
                            );

                            let observation_undistorted =
                                OVector::<_, U2>::new(undist_pt.x, undist_pt.y);

                            let model = &old_states[row_idx];
                            let obs_model = match &model.obs_models_and_likelihoods[cam_idx] {
                                ObservationModel::ObservationModelAndLikelihoods(oml) => {
                                    &oml.observation_model
                                }
                                ObservationModel::NoObservations => {
                                    // This should never happen.
                                    panic!("non-zero wantedness for non-existent observation.");
                                }
                            };

                            let estimate = &next_model.state.posterior;

                            let form = adskalman::CovarianceUpdateMethod::JosephForm;
                            let posterior = obs_model
                                .update(&estimate.estimate, &observation_undistorted, form)
                                // .map_err(|e| {
                                //     format!(
                                //         "While computing posterior for frame {}, camera {}: {}.",
                                //         frame_cam_points.frame_data.synced_frame,
                                //         frame_cam_points.frame_data.cam_name,
                                //         e
                                //     )
                                // })
                                .unwrap();

                            trace!("previous estimate {:?}", estimate.estimate.state());
                            trace!(" updated estimate {:?}", posterior.state());

                            // Compute the coords of the estimated state.
                            let reproj_undistorted =
                                obs_model.predict_observation(posterior.state());
                            let reproj_dist = ((reproj_undistorted.x - undist_pt.x).powi(2)
                                + (reproj_undistorted.y - undist_pt.y).powi(2))
                            .sqrt();

                            next_model.state.posterior.estimate = posterior;
                            let assoc = DataAssocInfo {
                                pt_idx: undist_pt.idx,
                                cam_num,
                                reproj_dist,
                            };

                            // trace!(
                            //     "object {} at frame {} using: {:?}",
                            //     next_model.lmi.obj_id,
                            //     bundle.frame().0,
                            //     assoc
                            // );

                            next_model.state.data_assoc_this_timestamp.push(assoc);
                        }
                    }
                }

                // we will fill this point-by-point
                let mut unused = vec![];

                for col_idx in unused_col_idxs.into_iter() {
                    unused.push(arena_data[col_idx].clone());
                }
                unused_bundle_per_cam.insert(cam_name, unused);
            }

            let state = CollectionFramePosteriors {
                models_with_posteriors,
            };

            let mcinner = self.mcinner;
            (
                ModelCollection { state, mcinner },
                UnusedDataPerArena(PerMiniArenaAllCamsOneFrameUndistorted {
                    per_cam: unused_bundle_per_cam,
                }),
            )
        }
    }
}

fn arg_max_col(a: &[f64]) -> Option<(usize, f64)> {
    let mut r = None;
    for (i, val) in a.iter().enumerate() {
        r = match r {
            None => Some((i, *val)),
            Some(testr) => {
                if *val > testr.1 {
                    Some((i, *val))
                } else {
                    Some(testr)
                }
            }
        };
    }
    r
}

fn to_bayesian_estimate(
    coords: Point3<MyFloat>,
    params: &TrackingParams,
) -> StateAndCovariance<MyFloat, U6> {
    // initial state estimate
    let state = Vector6::new(coords.x, coords.y, coords.z, 0.0, 0.0, 0.0);
    // initial covariance estimate.
    let initial_position_covar = params.initial_position_std_meters.powi(2);
    let mut covar = initial_position_covar * Matrix6::<MyFloat>::identity();

    let initial_vel_covar = params.initial_vel_std_meters_per_sec.powi(2);
    for i in 3..6 {
        covar[(i, i)] = initial_vel_covar;
    }
    StateAndCovariance::new(state, covar)
}

impl ModelCollection<CollectionFramePosteriors> {
    pub(crate) fn births_and_deaths<F>(
        mut self,
        tdpt: &TimeDataPassthrough,
        unused: UnusedDataPerArena,
        next_obj_id_func: F,
    ) -> (
        ModelCollection<CollectionFrameDone>,
        Vec<(SendType, TimeDataPassthrough)>,
        Vec<SaveToDiskMsg>,
    )
    where
        F: Fn() -> u32,
    {
        // Instead of a `#[tracing::instrument]` attribute on this method, which
        // we cannot do because F does not implement Debug, here we enter a
        // span.
        let _span = tracing::span!(tracing::Level::INFO, "births_and_deaths").entered();

        let mut result_messages = Vec::new();

        // Check deaths before births so we do not check if we kill a
        // just-created model.
        let orig_models = std::mem::take(&mut self.state.models_with_posteriors);

        let mut to_kill = Vec::with_capacity(orig_models.len());
        let mut to_live = Vec::with_capacity(orig_models.len() + 1);

        let max_variance = self.mcinner.params.max_position_std_meters.powi(2) as f64; // square so that it is in variance units

        for model in orig_models.into_iter() {
            let covar_size = model.state.covariance_size();
            // trace!(
            //     "frame: {}, obj_id: {}, covar_size: {}, max_variance: {}",
            //     unused.0.frame().0,
            //     model.lmi.obj_id,
            //     covar_size,
            //     max_variance
            // );
            if covar_size <= max_variance {
                to_live.push(model);
            } else {
                to_kill.push(model);
            }
        }

        // ---------------------------------
        // Handle births

        {
            // if log_enabled!(Trace) {
            //     trace!("before filtering");
            //     let mut f = Vec::<u8>::new();
            //     unused.0.pretty_format(&mut f, 0).unwrap();
            //     trace!("{}", std::str::from_utf8(&f).unwrap());
            // }

            let good_points = {
                // Use `minimum_pixel_abs_zscore` from hypothesis_test_params if
                // present, otherwise 0.
                let minimum_pixel_abs_zscore = self
                    .mcinner
                    .params
                    .hypothesis_test_params
                    .as_ref()
                    .map(|p| p.minimum_pixel_abs_zscore)
                    .unwrap_or(0.0);

                // let fdp_vec: &Vec<FrameDataAndPoints> = &unused.0.orig_distorted;

                // get single (best) point per camera
                // filter_points_and_take_first(fdp_vec, minimum_pixel_abs_zscore)
                filter_points_and_take_first(&unused, minimum_pixel_abs_zscore)
            };

            // if log_enabled!(Trace) {
            //     trace!("after filtering");
            //     let mut f = Vec::<u8>::new();
            //     unused.0.pretty_format(&mut f, 0).unwrap();
            //     trace!(
            //         "{} points considered for hypothesis test: {:?}",
            //         good_points.len(),
            //         good_points
            //     );
            // }

            if let Some(new_obj) = self.mcinner.new_obj.hypothesis_test(&good_points) {
                let HypothesisTestResult {
                    coords,
                    cams_and_reproj_dist,
                } = new_obj;

                // We were able to compute an acceptable solution, so spawn ("give birth")
                // to a new model.
                let data_assoc_this_timestamp = cams_and_reproj_dist
                    .iter()
                    .map(|ci| {
                        let pt_idx = 0;
                        let cam_num = self.mcinner.cam_manager.cam_num(&ci.ros_cam_name).unwrap();
                        DataAssocInfo {
                            pt_idx,
                            cam_num,
                            reproj_dist: ci.reproj_dist,
                        }
                    })
                    .collect();

                let estimate = to_bayesian_estimate(coords, &self.mcinner.params);

                let obj_id = next_obj_id_func();
                // trace!(
                //     "birth of object {} at frame {} (using: {:?})",
                //     obj_id,
                //     unused.0.tdpt.frame.0,
                //     data_assoc_this_timestamp
                // );

                // let mini_arena_idx = self
                //     .mcinner
                //     .params
                //     .mini_arena_config
                //     .get_arena_index(&coords);

                let model = LivingModel {
                    gestation_age: Some(1),
                    state: ModelFramePosteriors {
                        posterior: StampedEstimate {
                            estimate,
                            tdpt: tdpt.clone(),
                        },
                        data_assoc_this_timestamp,
                    },
                    posteriors: vec![],
                    last_observation_offset: 0,
                    lmi: LMInner {
                        obj_id,
                        _start_frame: tdpt.frame,
                    },
                };

                to_live.push(model);
            } else {
                trace!("no acceptable new object from hypothesis test");
            }
        }

        if !to_kill.is_empty() {
            for model in &to_kill {
                if model.gestation_age.is_none() {
                    result_messages.push((
                        SendType::Death(model.lmi.obj_id),
                        model.state.posterior.tdpt.clone(),
                    ));
                }
            }
        }

        let num_observations_to_visibility = self.mcinner.params.num_observations_to_visibility;

        let mut models = vec![];
        let mut save_messages = Vec::new();
        for x in to_live.into_iter() {
            let (this_models, this_result_messages, this_sav_msgs) =
                x.finish_frame(num_observations_to_visibility);
            save_messages.extend(this_sav_msgs);
            result_messages.extend(this_result_messages);
            models.push(this_models);
        }

        (
            ModelCollection {
                state: CollectionFrameDone { models },
                mcinner: self.mcinner,
            },
            result_messages,
            save_messages,
        )
    }
}

fn filter_points_and_take_first(
    // fdp_vec: &[FrameDataAndPoints],
    fdp_vec: &UnusedDataPerArena,
    minimum_pixel_abs_zscore: f64,
) -> BTreeMap<RosCamName, mvg::DistortedPixel<MyFloat>> {
    fdp_vec
        .0
        .per_cam
        .iter()
        .filter_map(|(cam_name, fdp)| {
            fdp.iter()
                .filter_map(|pt| {
                    // filter here
                    // trace!(
                    //     "pt: {:?}, pixel_zscore.abs(): {}",
                    //     pt.pt,
                    //     pixel_abszscore(&pt.pt)
                    // );
                    if pixel_abszscore(&pt.numbered_raw_udp_point.pt) < minimum_pixel_abs_zscore {
                        None
                    } else {
                        Some(convert_pt(&pt.numbered_raw_udp_point.pt))
                    }
                })
                .next()
                .map(|pt| (cam_name.clone(), pt))
        })
        .collect()
}

/// Calculate how far the current value is away from the mean
///
/// The result is a Z score.
fn pixel_abszscore(pt: &FlydraRawUdpPoint) -> f64 {
    let cur_val = pt.cur_val as f64;
    ((cur_val - pt.mean_val) / pt.sumsqf_val).abs()
}

fn convert_pt(input: &flydra_types::FlydraRawUdpPoint) -> mvg::DistortedPixel<MyFloat> {
    mvg::DistortedPixel {
        coords: nalgebra::geometry::Point2::new(input.x0_abs, input.y0_abs),
    }
}
