extern crate tracking;
extern crate nalgebra as na;
extern crate adskalman;
#[macro_use]
extern crate approx;

use crate::na::core::{Matrix4, Matrix6, Vector4, Vector6};

use adskalman::{StateAndCovariance, TransitionModelLinearNoControl};

use tracking::motion_model_3d_fixed_dt::MotionModel3D;

/// Test that doing updates every frame without observations
/// is equal to doing an update with a longer dt.
#[test]
fn test_missing_frames_via_large_dt_2d() {
    use tracking::motion_model_2d::ConstantVelocity2DModel;

    let motion_noise_scale = 1.234;
    let model = ConstantVelocity2DModel::new(motion_noise_scale);

    let dt1 = 5.678;
    let state0 = Vector4::new(1.2, 3.4, 5.6, 7.8);
    let covar0 = 42.0*Matrix4::<f64>::identity();

    let est0 = StateAndCovariance::new(state0, covar0);

    // Run two time steps of duration dt.
    let mm1 = model.calc_for_dt(dt1);
    let est1_1 = mm1.predict(&est0);
    let est1_2 = mm1.predict(&est1_1);

    // Run one time step of duration 2*dt.
    let mm2 = model.calc_for_dt(2.0*dt1);
    let est2_2 = mm2.predict(&est0);

    assert_relative_eq!(est1_2.state(), est2_2.state());
    assert_relative_eq!(est1_2.covariance(), est2_2.covariance());
}

/// Test that doing updates every frame without observations
/// is equal to doing an update with a longer dt.
#[test]
fn test_missing_frames_via_large_dt_3d() {
    use tracking::motion_model_3d::ConstantVelocity3DModel;

    let motion_noise_scale = 1.234;
    let model = ConstantVelocity3DModel::new(motion_noise_scale);

    let dt1 = 5.678;
    let state0 = Vector6::new(1.2, 3.4, 5.6, 7.8, 9.10, 11.12);
    let covar0 = 42.0*Matrix6::<f64>::identity();

    let est0 = StateAndCovariance::new(state0, covar0);

    // Run two time steps of duration dt.
    let mm1 = model.calc_for_dt(dt1);
    let est1_1 = mm1.predict(&est0);
    let est1_2 = mm1.predict(&est1_1);

    // Run one time step of duration 2*dt.
    let mm2 = model.calc_for_dt(2.0*dt1);
    let est2_2 = mm2.predict(&est0);

    assert_relative_eq!(est1_2.state(), est2_2.state());
    assert_relative_eq!(est1_2.covariance(), est2_2.covariance());
}

/// Test that doing updates every frame without observations
/// is equal to doing an update with a longer dt.
#[test]
fn test_missing_frames_via_large_dt_flat3d() {
    use tracking::flat_motion_model_3d::FlatZZero3DModel;

    let motion_noise_scale = 1.234;
    let model = FlatZZero3DModel::new(motion_noise_scale);

    let dt1 = 5.678;
    let state0 = Vector6::new(1.2, 3.4, 0.0, 7.8, 9.10, 0.0);
    let mut covar0 = 42.0*Matrix6::<f64>::identity();
    covar0[(2,2)] = 0.0;
    covar0[(5,5)] = 0.0;

    let est0 = StateAndCovariance::new(state0, covar0);

    // Run two time steps of duration dt.
    let mm1 = model.calc_for_dt(dt1);
    let est1_1 = mm1.predict(&est0);
    let est1_2 = mm1.predict(&est1_1);

    // Run one time step of duration 2*dt.
    let mm2 = model.calc_for_dt(2.0*dt1);
    let est2_2 = mm2.predict(&est0);

    assert_relative_eq!(est1_2.state(), est2_2.state());
    assert_relative_eq!(est1_2.covariance(), est2_2.covariance());
}
