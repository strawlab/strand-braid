use num_traits::{One, Zero};

use nalgebra::allocator::Allocator;
use nalgebra::core::dimension::U6;
use nalgebra::{DefaultAllocator, OMatrix, RealField};

use crate::motion_model_3d_fixed_dt::MotionModel3D;
use crate::motion_model_3d_fixed_dt::MotionModel3DFixedDt;

/// constant velocity 3D motion model with Z fixed to 0
///
/// The important method is `calc_for_dt()`. Calling this
/// returns a motion model for a specific `dt`.
///
/// The state vector is [x y z xvel yvel zvel].
#[derive(Debug, Clone)]
pub struct FlatZZero3DModel<R: RealField>
where
    DefaultAllocator: Allocator<R, U6, U6>,
    DefaultAllocator: Allocator<R, U6>,
{
    motion_noise_scale: R,
}

impl<R: RealField> FlatZZero3DModel<R>
where
    DefaultAllocator: Allocator<R, U6, U6>,
    DefaultAllocator: Allocator<R, U6>,
{
    pub fn new(motion_noise_scale: R) -> Self {
        Self { motion_noise_scale }
    }
}

impl<R: RealField> MotionModel3D<R> for FlatZZero3DModel<R>
where
    DefaultAllocator: Allocator<R, U6, U6>,
    DefaultAllocator: Allocator<R, U6>,
{
    fn calc_for_dt(&self, dt: R) -> MotionModel3DFixedDt<R> {
        let zero: R = Zero::zero();
        let one: R = One::one();
        let two: R = one + one;
        let three: R = two + one;

        // Create transition model. 3D position and 3D velocity.
        // This is "A" in most Kalman filter descriptions.
        #[rustfmt::skip]
        let transition_model = {
            OMatrix::<R,U6,U6>::from_row_slice(
                        &[one, zero, zero,   dt, zero, zero,
                         zero,  one, zero, zero,   dt, zero,
                         zero, zero, zero, zero, zero, zero,
                         zero, zero, zero,  one, zero, zero,
                         zero, zero, zero, zero,  one, zero,
                         zero, zero, zero, zero, zero, zero])
            };
        let transition_model_transpose = transition_model.transpose();

        let t33 = (dt * dt * dt) / three;
        let t22 = (dt * dt) / two;

        // This is "Q" in most Kalman filter descriptions.
        #[rustfmt::skip]
        let transition_noise_covariance = {
            OMatrix::<R,U6,U6>::from_row_slice(
                       &[t33,  zero, zero, t22, zero,  zero,
                        zero,  t33, zero, zero,  t22, zero,
                        zero, zero, zero, zero, zero, zero,
                        t22,  zero, zero,   dt, zero, zero,
                        zero,  t22, zero, zero,   dt, zero,
                        zero, zero, zero, zero, zero, zero]) * self.motion_noise_scale
            };
        MotionModel3DFixedDt {
            transition_model,
            transition_model_transpose,
            transition_noise_covariance,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use adskalman::TransitionModelLinearNoControl;

    #[test]
    fn test_fix_z() {
        let model = FlatZZero3DModel::new(1.0);
        let m2 = model.calc_for_dt(1.0);
        let matrix = m2.F();

        let pos1 = na::OVector::<_, U6>::from_row_slice(&[0.1, 0.2, 0.3, 0.4, 0.5, 0.6]);
        let pos2 = matrix * pos1;

        // Check the z position is zero after update.
        assert_eq!(pos2[2], 0.0);

        // Check the z vel is zero after update.
        assert_eq!(pos2[5], 0.0);
    }
}
