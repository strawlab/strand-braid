use log::{log_enabled, trace, Level::Trace};
use std::{collections::BTreeMap, sync::Arc};

use nalgebra::core::dimension::{U2, U6};
use nalgebra::{Matrix6, OMatrix, OVector, Point3, RealField, Vector6};

use nalgebra_mvn::MultivariateNormal;

use pretty_print_nalgebra::pretty_print;

use tracking::motion_model_3d_fixed_dt::{MotionModel3D, MotionModel3DFixedDt};

#[cfg(feature = "flat-3d")]
use tracking::flat_motion_model_3d::FlatZZero3DModel;
#[cfg(feature = "full-3d")]
use tracking::motion_model_3d::ConstantVelocity3DModel;
#[cfg(not(any(feature = "full-3d", feature = "flat-3d")))]
compile_error!("must either have feature full-3d or flat-3d");

use adskalman::ObservationModel as ObservationModelTrait;
use adskalman::{StateAndCovariance, TransitionModelLinearNoControl};

use flydra_types::{
    CamNum, FlydraFloatTimestampLocal, FlydraRawUdpPoint, KalmanEstimatesRow, RosCamName, SyncFno,
    Triggerbox,
};

use crate::{
    to_world_point, CameraObservationModel, ConnectedCamerasManager, DataAssocRow,
    FrameDataAndPoints, KalmanEstimateRecord, MyFloat, SaveToDiskMsg, SwitchingTrackingParams,
    TimeDataPassthrough,
};
use crossbeam_ok::CrossbeamOk;

use crate::bundled_data::{
    BundledAllCamsOneFrameUndistorted, OneCamOneFrameUndistorted, Undistorted,
};
use crate::model_server::{GetsUpdates, SendKalmanEstimatesRow, SendType};
use crate::{new_object_test::NewObjectTest, HypothesisTestResult};

// -----------------------------------------------------------------------------

pub(crate) struct UnusedData(pub(crate) BundledAllCamsOneFrameUndistorted);

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
    let v1 = vec![mat[(0, 0)], mat[(1, 1)], mat[(2, 2)]];
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
        ekf_observation_covariance_pixels: f32,
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
        all_cam_data: &BundledAllCamsOneFrameUndistorted,
        recon: &flydra_mvg::FlydraMultiCameraSystem<MyFloat>,
        ekf_observation_covariance_pixels: f32,
    ) -> LivingModel<ModelFrameWithObservationLikes> {
        // for each camera with data:
        //  - compute likelihood of each real observation given expected observation

        let obs_models_and_likelihoods: Vec<ObservationModel> = all_cam_data
            .inner
            .iter()
            .map(|cam_data| {
                // outer loop: cameras

                if cam_data.undistorted.is_empty() {
                    ObservationModel::NoObservations
                } else {
                    let cam = recon
                        .cam_by_name(cam_data.frame_data.cam_name.as_str())
                        .unwrap();
                    let (observation_model, eo) =
                        self.compute_expected_observation(cam, ekf_observation_covariance_pixels);

                    let likes: Vec<f64> = if let Some(expected_observation) = eo {
                        trace!(
                            "object {} {} expects ({},{})",
                            self.lmi.obj_id,
                            cam_data.frame_data.cam_name,
                            expected_observation.mean()[0],
                            expected_observation.mean()[1]
                        );

                        cam_data
                            .undistorted
                            .iter()
                            .map(|pt: &Undistorted| {
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
                        vec![0.0; cam_data.undistorted.len()]
                    };
                    trace!("incoming points: {:?}", cam_data.undistorted);
                    trace!("likelihoods: {:?}", likes);
                    ObservationModel::ObservationModelAndLikelihoods(
                        ObservationModelAndLikelihoods {
                            observation_model,
                            likelihoods: nalgebra::RowDVector::from_iterator(
                                likes.len(),
                                likes.into_iter(),
                            ),
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
        save_data_tx: &mut channellib::Sender<SaveToDiskMsg>,
        model_servers: &[Box<dyn GetsUpdates>],
        num_observations_to_visibility: u8,
    ) -> LivingModel<ModelFrameDone> {
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
                // send birth event
                for ms in model_servers.iter() {
                    ms.send_update(
                        SendType::Birth(send_kalman_estimate_row.clone()),
                        &self.state.posterior.tdpt,
                    )
                    .expect("send update");
                }
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
                    save_data_tx
                        .send(SaveToDiskMsg::KalmanEstimate(KalmanEstimateRecord {
                            record: no_obs_record,
                            data_assoc_rows: vec![],
                            mean_reproj_dist_100x: None,
                        }))
                        .cb_ok();
                }

                // Now save the final row (with observations).
                // println!("saving row with observations {} {}", self.lmi.obj_id, frame.0);
                save_data_tx
                    .send(SaveToDiskMsg::KalmanEstimate(KalmanEstimateRecord {
                        record,
                        data_assoc_rows,
                        mean_reproj_dist_100x,
                    }))
                    .cb_ok();
            }
            self.last_observation_offset = self.posteriors.len();
        }

        if new_gestation_age.is_none() {
            // We are not in gestation period, so we are visible and should
            // save data.

            // Regardless of whether there was a new observation, send the updated
            // posterior estimate to the network.
            for ms in model_servers.iter() {
                // Here is the realtime pose output when using the HTTP
                // model server.
                ms.send_update(
                    SendType::Update(send_kalman_estimate_row.clone()),
                    &self.state.posterior.tdpt,
                )
                .expect("send update");
            }
        }

        // convert to ModelFrameDone -------------------------------

        // current posterior is appended to list of posteriors.
        let mut posteriors = self.posteriors;
        posteriors.push(self.state.posterior);
        LivingModel {
            gestation_age: new_gestation_age,
            state: ModelFrameDone {},
            posteriors,
            last_observation_offset: self.last_observation_offset,
            lmi: self.lmi,
        }
    }
}

// ModelCollection -------------------------------------------------------------

pub(crate) trait CollectionState {}
pub(crate) struct CollectionFrameDone {
    models: Vec<LivingModel<ModelFrameDone>>,
}
pub(crate) struct CollectionFrameStarted {
    models: Vec<LivingModel<ModelFrameStarted>>,
}
pub(crate) struct CollectionFrameWithObservationLikes {
    models_with_obs_likes: Vec<LivingModel<ModelFrameWithObservationLikes>>,
    bundle: BundledAllCamsOneFrameUndistorted,
}
pub(crate) struct CollectionFramePosteriors {
    models_with_posteriors: Vec<LivingModel<ModelFramePosteriors>>,
}

impl CollectionState for CollectionFrameDone {}
impl CollectionState for CollectionFrameStarted {}
impl CollectionState for CollectionFrameWithObservationLikes {}
impl CollectionState for CollectionFramePosteriors {}

pub(crate) fn initialize_model_collection(
    params: Arc<SwitchingTrackingParams>,
    recon: flydra_mvg::FlydraMultiCameraSystem<MyFloat>,
    fps: f32,
    cam_manager: ConnectedCamerasManager,
    save_data_tx: channellib::Sender<SaveToDiskMsg>,
) -> ModelCollection<CollectionFrameDone> {
    let new_obj = NewObjectTest::new(recon.clone(), params.clone());

    let motion_noise_scale = params.motion_noise_scale;

    #[cfg(feature = "full-3d")]
    let motion_model_generator = ConstantVelocity3DModel::new(motion_noise_scale);

    #[cfg(feature = "flat-3d")]
    let motion_model_generator = FlatZZero3DModel::new(motion_noise_scale);

    let dt = 1.0 / fps as f64;
    let motion_model = motion_model_generator.calc_for_dt(dt);

    ModelCollection {
        state: CollectionFrameDone { models: vec![] },
        mcinner: MCInner {
            params,
            recon,
            new_obj,
            motion_model,
            cam_manager,
            next_obj_id: 0,
            save_data_tx,
            // model_sender,
        },
    }
}

pub(crate) struct ModelCollection<S: CollectionState> {
    state: S,
    pub(crate) mcinner: MCInner,
}

pub(crate) struct MCInner {
    params: Arc<SwitchingTrackingParams>,
    pub(crate) recon: flydra_mvg::FlydraMultiCameraSystem<MyFloat>,
    new_obj: NewObjectTest,
    motion_model: MotionModel3DFixedDt<MyFloat>,
    cam_manager: ConnectedCamerasManager,
    next_obj_id: u32,
    save_data_tx: channellib::Sender<SaveToDiskMsg>,
}

impl ModelCollection<CollectionFrameDone> {
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
    pub(crate) fn compute_observation_likes(
        self,
        bundle: BundledAllCamsOneFrameUndistorted,
    ) -> ModelCollection<CollectionFrameWithObservationLikes> {
        trace!(
            "---- computing observation likelihoods from frame {} -----",
            bundle.tdpt.frame.0
        );

        let (mcinner, state) = (self.mcinner, self.state);
        let models_with_obs_likes: Vec<LivingModel<_>> = state
            .models
            .into_iter()
            .map(|x| {
                x.compute_observation_likelihoods(
                    &bundle,
                    &mcinner.recon,
                    mcinner.params.ekf_observation_covariance_pixels,
                )
            })
            .collect();
        ModelCollection {
            state: CollectionFrameWithObservationLikes {
                models_with_obs_likes,
                bundle,
            },
            mcinner,
        }
    }
}

impl ModelCollection<CollectionFrameWithObservationLikes> {
    pub(crate) fn solve_data_association_and_update(
        self,
    ) -> (ModelCollection<CollectionFramePosteriors>, UnusedData) {
        let zero = nalgebra::convert(0.0);

        // We have likelihoods for all objects on all cameras for each point.
        //
        // Do something like the hungarian algorithm.

        if self.state.models_with_obs_likes.is_empty() {
            // Short-circuit stuff below when no data.
            let state = CollectionFramePosteriors {
                models_with_posteriors: vec![],
            };
            let mcinner = self.mcinner;
            (
                ModelCollection { state, mcinner },
                UnusedData(self.state.bundle),
            )
        } else {
            let bundle: &BundledAllCamsOneFrameUndistorted = &self.state.bundle;

            // loop camera-by-camera to get MxN matrix of live model and num observations.
            // currently, we can loop by model and then (obs_num x cam_num)

            // we will fill this cam-by-cam
            let mut unused_bundle = BundledAllCamsOneFrameUndistorted {
                tdpt: bundle.tdpt.clone(),
                inner: vec![],
                orig_distorted: vec![],
            };

            // Initialize updated models in which no observation
            // was used and thus the posteriors are just the priors.
            let mut models_with_posteriors: Vec<_> = self
                .state
                .models_with_obs_likes
                .iter()
                .map(|model| {
                    LivingModel {
                        gestation_age: model.gestation_age,
                        state: ModelFramePosteriors {
                            posterior: StampedEstimate {
                                estimate: model.state.prior.clone(), // just the prior initially
                                tdpt: bundle.tdpt.clone(),
                            },
                            data_assoc_this_timestamp: vec![], // no observations yet
                        },
                        posteriors: model.posteriors.clone(),
                        last_observation_offset: model.last_observation_offset,
                        lmi: model.lmi.clone(),
                    }
                })
                .collect();

            // outer loop here iterates over the per-camera data, So we compute
            // the "wantedness" matrix for each camera one at a time, considering
            // the models and set of observations for this camera.
            for (cam_idx, per_cam) in bundle
                .inner
                .iter()
                .zip(bundle.orig_distorted.iter())
                .enumerate()
            {
                let (frame_cam_points, fdp): (&OneCamOneFrameUndistorted, &FrameDataAndPoints) =
                    per_cam;

                if frame_cam_points.undistorted.is_empty() {
                    continue;
                }

                let cam_num = self
                    .mcinner
                    .cam_manager
                    .cam_num(&frame_cam_points.frame_data.cam_name)
                    .unwrap();

                trace!(
                    "camera {} ({}): {} points",
                    frame_cam_points.frame_data.cam_name,
                    cam_num,
                    frame_cam_points.undistorted.len()
                );

                debug_assert!(frame_cam_points.frame_data.cam_name == fdp.frame_data.cam_name);

                // Get pre-computed likelihoods for each model for this camera.
                // There are N elements in the outer vector, one for each model
                // and M elements in each inner container, corresponding to the M
                // detected points for this camera on this frame.
                let wantedness: Vec<_> = self
                    .state
                    .models_with_obs_likes
                    .iter()
                    .map(
                        |model| match &model.state.obs_models_and_likelihoods[cam_idx] {
                            ObservationModel::ObservationModelAndLikelihoods(oml) => {
                                oml.likelihoods.clone()
                            }
                            ObservationModel::NoObservations => {
                                nalgebra::RowDVector::zeros(frame_cam_points.undistorted.len())
                            }
                        },
                    )
                    .collect();

                debug_assert!(wantedness.len() == self.state.models_with_obs_likes.len());

                // debug!("wantedness1 {:?}", wantedness);

                let mut wantedness =
                    nalgebra::OMatrix::<f64, nalgebra::Dynamic, nalgebra::Dynamic>::from_rows(
                        wantedness.as_slice(),
                    );

                debug_assert!(self.state.models_with_obs_likes.len() == wantedness.nrows());
                debug_assert!(frame_cam_points.undistorted.len() == wantedness.ncols());

                trace!(
                    "wantedness (N x M where N is num live models and M is num points)\n{}",
                    pretty_print!(&wantedness)
                );

                // Consume all incoming points either into a observation or into unconsumed_points.

                use std::iter::FromIterator;
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

                            let undist_pt = &frame_cam_points.undistorted[best_idx];
                            trace!(
                                "object {} is accepting undistorted point {:?}",
                                next_model.lmi.obj_id,
                                undist_pt
                            );

                            let observation_undistorted =
                                OVector::<_, U2>::new(undist_pt.x, undist_pt.y);

                            let model = &self.state.models_with_obs_likes[row_idx];
                            let obs_model = match &model.state.obs_models_and_likelihoods[cam_idx] {
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
                                .map_err(|e| {
                                    format!(
                                        "While computing posterior for frame {}, camera {}: {}.",
                                        frame_cam_points.frame_data.synced_frame,
                                        frame_cam_points.frame_data.cam_name,
                                        e
                                    )
                                })
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

                            trace!(
                                "object {} at frame {} using: {:?}",
                                next_model.lmi.obj_id,
                                bundle.frame().0,
                                assoc
                            );

                            next_model.state.data_assoc_this_timestamp.push(assoc);
                        }
                    }
                }

                // we will fill this point-by-point
                let mut undist = OneCamOneFrameUndistorted {
                    frame_data: fdp.frame_data.clone(),
                    undistorted: vec![],
                };
                let mut orig = FrameDataAndPoints {
                    frame_data: fdp.frame_data.clone(),
                    points: vec![],
                };

                for col_idx in unused_col_idxs.into_iter() {
                    let undist_pt = frame_cam_points.undistorted[col_idx].clone();
                    let orig_dist = fdp.points[col_idx].clone();
                    undist.undistorted.push(undist_pt.clone());
                    orig.points.push(orig_dist.clone());
                }
                unused_bundle.inner.push(undist);
                unused_bundle.orig_distorted.push(orig);
            }

            let state = CollectionFramePosteriors {
                models_with_posteriors,
            };

            let mcinner = self.mcinner;
            (
                ModelCollection { state, mcinner },
                UnusedData(unused_bundle),
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
    params: &SwitchingTrackingParams,
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
    fn next_obj_id(&mut self) -> u32 {
        let next = self.mcinner.next_obj_id;
        self.mcinner.next_obj_id += 1;
        next
    }

    pub(crate) fn births_and_deaths(
        mut self,
        unused: UnusedData,
        model_servers: &[Box<dyn GetsUpdates>],
    ) -> ModelCollection<CollectionFrameDone> {
        // Check deaths before births so we do not check if we kill a
        // just-created model.
        let orig_models = std::mem::replace(
            &mut self.state.models_with_posteriors,
            Vec::with_capacity(0),
        );

        let mut to_kill = Vec::with_capacity(orig_models.len());
        let mut to_live = Vec::with_capacity(orig_models.len());

        let max_variance = self.mcinner.params.max_position_std_meters.powi(2) as f64; // square so that it is in variance units

        for model in orig_models.into_iter() {
            let covar_size = model.state.covariance_size();
            trace!(
                "frame: {}, obj_id: {}, covar_size: {}, max_variance: {}",
                unused.0.frame().0,
                model.lmi.obj_id,
                covar_size,
                max_variance
            );
            if covar_size <= max_variance {
                to_live.push(model);
            } else {
                to_kill.push(model);
            }
        }

        // ---------------------------------
        // Handle births

        {
            if log_enabled!(Trace) {
                trace!("before filtering");
                let mut f = Vec::<u8>::new();
                unused.0.pretty_format(&mut f, 0).unwrap();
                trace!("{}", std::str::from_utf8(&f).unwrap());
            }

            let good_points = {
                #[cfg(feature = "full-3d")]
                let minimum_pixel_abs_zscore = self
                    .mcinner
                    .params
                    .hypothesis_test_params
                    .minimum_pixel_abs_zscore;
                #[cfg(feature = "flat-3d")]
                let minimum_pixel_abs_zscore = 0.0;

                let fdp_vec: &Vec<FrameDataAndPoints> = &unused.0.orig_distorted;

                // get single (best) point per camera
                filter_points_and_take_first(fdp_vec, minimum_pixel_abs_zscore)
            };

            if log_enabled!(Trace) {
                trace!("after filtering");
                let mut f = Vec::<u8>::new();
                unused.0.pretty_format(&mut f, 0).unwrap();
                trace!(
                    "{} points considered for hypothesis test: {:?}",
                    good_points.len(),
                    good_points
                );
            }

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

                let obj_id = self.next_obj_id();
                trace!(
                    "birth of object {} at frame {} (using: {:?})",
                    obj_id,
                    unused.0.tdpt.frame.0,
                    data_assoc_this_timestamp
                );

                let model = LivingModel {
                    gestation_age: Some(1),
                    state: ModelFramePosteriors {
                        posterior: StampedEstimate {
                            estimate,
                            tdpt: unused.0.tdpt.clone(),
                        },
                        data_assoc_this_timestamp,
                    },
                    posteriors: vec![],
                    last_observation_offset: 0,
                    lmi: LMInner {
                        obj_id,
                        _start_frame: unused.0.tdpt.frame,
                    },
                };

                to_live.push(model);
            } else {
                trace!("no acceptable new object from hypothesis test");
            }
        }

        if !to_kill.is_empty() {
            for ms in model_servers.iter() {
                for model in &to_kill {
                    if model.gestation_age.is_none() {
                        ms.send_update(
                            SendType::Death(model.lmi.obj_id),
                            &model.state.posterior.tdpt,
                        )
                        .expect("send update");
                    }
                }
            }
        }

        let num_observations_to_visibility = self.mcinner.params.num_observations_to_visibility;

        let models = to_live
            .into_iter()
            .map(|x| {
                x.finish_frame(
                    &mut self.mcinner.save_data_tx,
                    model_servers,
                    num_observations_to_visibility,
                )
            })
            .collect();

        ModelCollection {
            state: CollectionFrameDone { models },
            mcinner: self.mcinner,
        }
    }
}

fn filter_points_and_take_first(
    fdp_vec: &[FrameDataAndPoints],
    minimum_pixel_abs_zscore: f64,
) -> BTreeMap<RosCamName, mvg::DistortedPixel<MyFloat>> {
    fdp_vec
        .iter()
        .filter_map(|fdp| {
            fdp.points
                .iter()
                .filter_map(|pt| {
                    // filter here
                    trace!(
                        "pt: {:?}, pixel_zscore.abs(): {}",
                        pt.pt,
                        pixel_abszscore(&pt.pt)
                    );
                    if pixel_abszscore(&pt.pt) < minimum_pixel_abs_zscore {
                        None
                    } else {
                        Some(convert_pt(&pt.pt))
                    }
                })
                .next()
                .map(|pt| (fdp.frame_data.cam_name.clone(), pt))
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
