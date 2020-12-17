use nalgebra as na;
use nalgebra::allocator::Allocator;
use nalgebra::core::dimension::U6;
use nalgebra::core::MatrixN;
use nalgebra::{DefaultAllocator, RealField};

use adskalman::TransitionModelLinearNoControl;

/// constant velocity 3D motion model for fixed dt
///
/// The state vector is [x y z xvel yvel zvel]
#[derive(Debug)]
pub struct MotionModel3DFixedDt<R: RealField>
where
    DefaultAllocator: Allocator<R, U6, U6>,
    DefaultAllocator: Allocator<R, U6>,
{
    pub transition_model: MatrixN<R, U6>,
    pub transition_model_transpose: MatrixN<R, U6>,
    pub transition_noise_covariance: MatrixN<R, U6>,
}

impl<R: RealField> TransitionModelLinearNoControl<R, U6> for MotionModel3DFixedDt<R>
where
    DefaultAllocator: Allocator<R, U6, U6>,
    DefaultAllocator: Allocator<R, U6>,
{
    fn F(&self) -> &MatrixN<R, U6> {
        &self.transition_model
    }
    fn FT(&self) -> &MatrixN<R, U6> {
        &self.transition_model_transpose
    }
    fn Q(&self) -> &MatrixN<R, U6> {
        &self.transition_noise_covariance
    }
}

pub trait MotionModel3D<R>: Clone
where
    R: na::RealField,
{
    /// For a given `dt`, create a new instance of the motion model.
    fn calc_for_dt(&self, dt: R) -> MotionModel3DFixedDt<R>;
}
