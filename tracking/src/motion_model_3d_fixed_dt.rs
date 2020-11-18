use crate::na;
use crate::na::core::MatrixN;
use crate::na::core::dimension::U6;
use crate::na::{DefaultAllocator, RealField};
use crate::na::allocator::Allocator;

use adskalman::TransitionModelLinearNoControl;

/// constant velocity 3D motion model for fixed dt
///
/// The state vector is [x y z xvel yvel zvel]
#[derive(Debug)]
pub struct MotionModel3DFixedDt<R: RealField>
    where DefaultAllocator: Allocator<R, U6, U6>,
          DefaultAllocator: Allocator<R, U6>,
{
    pub transition_model: MatrixN<R,U6>,
    pub transition_model_transpose: MatrixN<R,U6>,
    pub transition_noise_covariance: MatrixN<R,U6>,
}

impl<R: RealField> TransitionModelLinearNoControl<R, U6> for MotionModel3DFixedDt<R>
    where DefaultAllocator: Allocator<R, U6, U6>,
          DefaultAllocator: Allocator<R, U6>,
{
    fn transition_model(&self) -> &MatrixN<R,U6> {
        &self.transition_model
    }
    fn transition_model_transpose(&self) -> &MatrixN<R,U6> {
        &self.transition_model_transpose
    }
    fn transition_noise_covariance(&self) -> &MatrixN<R,U6> {
        &self.transition_noise_covariance
    }
}

pub trait MotionModel3D<R> : Clone
    where R: na::RealField
{
    /// For a given `dt`, create a new instance of the motion model.
    fn calc_for_dt(&self, dt: R) -> MotionModel3DFixedDt<R>;
}
