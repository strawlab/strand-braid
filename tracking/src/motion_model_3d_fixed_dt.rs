use nalgebra as na;
use nalgebra::allocator::Allocator;
use nalgebra::core::dimension::U6;
use nalgebra::{DefaultAllocator, OMatrix, RealField};

use adskalman::TransitionModelLinearNoControl;

/// constant velocity 3D motion model for fixed dt
///
/// The state vector is [x y z xvel yvel zvel]
#[derive(Debug)]
pub struct MotionModel3DFixedDt<R: RealField + Copy>
where
    DefaultAllocator: Allocator<R, U6, U6>,
    DefaultAllocator: Allocator<R, U6>,
{
    pub transition_model: OMatrix<R, U6, U6>,
    pub transition_model_transpose: OMatrix<R, U6, U6>,
    pub transition_noise_covariance: OMatrix<R, U6, U6>,
}

impl<R: RealField + Copy> TransitionModelLinearNoControl<R, U6> for MotionModel3DFixedDt<R>
where
    DefaultAllocator: Allocator<R, U6, U6>,
    DefaultAllocator: Allocator<R, U6>,
{
    fn F(&self) -> &OMatrix<R, U6, U6> {
        &self.transition_model
    }
    fn FT(&self) -> &OMatrix<R, U6, U6> {
        &self.transition_model_transpose
    }
    fn Q(&self) -> &OMatrix<R, U6, U6> {
        &self.transition_noise_covariance
    }
}

pub trait MotionModel3D<R>: Clone
where
    R: na::RealField + Copy,
{
    /// For a given `dt`, create a new instance of the motion model.
    fn calc_for_dt(&self, dt: R) -> MotionModel3DFixedDt<R>;
}
