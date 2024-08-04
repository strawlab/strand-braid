use num_traits::{One, Zero};

use nalgebra::{allocator::Allocator, dimension::U6, DefaultAllocator, OMatrix, RealField};

use crate::motion_model_3d_fixed_dt::MotionModel3D;
use crate::motion_model_3d_fixed_dt::MotionModel3DFixedDt;

/// constant velocity 3D motion model parameterized by `dt`
///
/// The important method is `calc_for_dt()`. Calling this
/// returns a motion model for a specific `dt`.
///
/// The state vector is [x y z xvel yvel zvel].
#[derive(Debug, Clone)]
pub struct ConstantVelocity3DModel<R: RealField + Copy>
where
    DefaultAllocator: Allocator<U6, U6>,
    DefaultAllocator: Allocator<U6>,
{
    motion_noise_scale: R,
}

impl<R: RealField + Copy> ConstantVelocity3DModel<R>
where
    DefaultAllocator: Allocator<U6, U6>,
    DefaultAllocator: Allocator<U6>,
{
    pub fn new(motion_noise_scale: R) -> Self {
        Self { motion_noise_scale }
    }
}

impl<R: RealField + Copy> MotionModel3D<R> for ConstantVelocity3DModel<R>
where
    DefaultAllocator: Allocator<U6, U6>,
    DefaultAllocator: Allocator<U6>,
{
    fn calc_for_dt(&self, dt: R) -> MotionModel3DFixedDt<R> {
        let zero: R = Zero::zero();
        let one: R = One::one();
        let two: R = one + one;
        let three: R = two + one;

        // Create transition model. 3D position and 3D velocity.
        // This is "A" in most Kalman filter descriptions.
        #[rustfmt::skip]
        let transition_model = OMatrix::<R,U6,U6>::from_row_slice(
                          &[one, zero, zero,   dt, zero, zero,
                         zero,  one, zero, zero,   dt, zero,
                         zero, zero,  one, zero, zero,   dt,
                         zero, zero, zero,  one, zero, zero,
                         zero, zero, zero, zero,  one, zero,
                         zero, zero, zero, zero, zero,  one]);
        let transition_model_transpose = transition_model.transpose();

        let t33 = (dt * dt * dt) / three;
        let t22 = (dt * dt) / two;

        // This is "Q" in most Kalman filter descriptions.
        #[rustfmt::skip]
        let transition_noise_covariance = OMatrix::<R,U6,U6>::from_row_slice(
                        &[t33,  zero, zero, t22, zero,  zero,
                        zero,  t33, zero, zero,  t22, zero,
                        zero, zero,  t33, zero, zero,  t22,
                        t22,  zero, zero,   dt, zero, zero,
                        zero,  t22, zero, zero,   dt, zero,
                        zero, zero,  t22, zero, zero,   dt]) * self.motion_noise_scale;
        MotionModel3DFixedDt {
            transition_model,
            transition_model_transpose,
            transition_noise_covariance,
        }
    }
}
