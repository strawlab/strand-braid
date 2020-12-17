use num_traits::{One, Zero};

use nalgebra::allocator::Allocator;
use nalgebra::core::dimension::U4;
use nalgebra::core::MatrixN;
use nalgebra::{DefaultAllocator, RealField};

use adskalman::TransitionModelLinearNoControl;

/// constant velocity 2D motion model parameterized by `dt`
///
/// The important method is `calc_for_dt()`. Calling this
/// returns a motion model for a specific `dt`.
///
/// The state vector is [x y xvel yvel].
#[derive(Debug)]
pub struct ConstantVelocity2DModel<R: RealField>
where
    DefaultAllocator: Allocator<R, U4, U4>,
    DefaultAllocator: Allocator<R, U4>,
{
    motion_noise_scale: R,
}

impl<R: RealField> ConstantVelocity2DModel<R>
where
    DefaultAllocator: Allocator<R, U4, U4>,
    DefaultAllocator: Allocator<R, U4>,
{
    pub fn new(motion_noise_scale: R) -> Self {
        Self { motion_noise_scale }
    }

    /// For a given `dt`, create a new instance of the motion model.
    pub fn calc_for_dt(&self, dt: R) -> MotionModel2DFixedDt<R> {
        let zero: R = Zero::zero();
        let one: R = One::one();
        let two: R = one + one;
        let three: R = two + one;

        // Create transition model. 2D position and 2D velocity.
        // This is "A" in most Kalman filter descriptions.
        #[rustfmt::skip]
        let transition_model = MatrixN::<R,U4>::new(
                          one, zero,   dt, zero,
                         zero,  one, zero,   dt,
                         zero, zero,  one, zero,
                         zero, zero, zero,  one);
        let transition_model_transpose = transition_model.transpose();

        let t33 = (dt * dt * dt) / three;
        let t22 = (dt * dt) / two;

        // This form is after N. Shimkin's lecture notes in
        // Estimation and Identification in Dynamical Systems
        // http://webee.technion.ac.il/people/shimkin/Estimation09/ch8_target.pdf
        // See also eq. 43 on pg. 13 of
        // http://www.robots.ox.ac.uk/~ian/Teaching/Estimation/LectureNotes2.pdf

        // This is "Q" in most Kalman filter descriptions.
        #[rustfmt::skip]
        let transition_noise_covariance = MatrixN::<R,U4>::new(
                        t33,  zero,  t22, zero,
                        zero,  t33, zero,  t22,
                        t22,  zero,   dt, zero,
                        zero,  t22, zero,   dt) * self.motion_noise_scale;
        MotionModel2DFixedDt {
            transition_model,
            transition_model_transpose,
            transition_noise_covariance,
        }
    }
}

/// constant velocity 2D motion model for fixed dt
///
/// The state vector is [x y xvel yvel]
#[derive(Debug)]
pub struct MotionModel2DFixedDt<R: RealField>
where
    DefaultAllocator: Allocator<R, U4, U4>,
    DefaultAllocator: Allocator<R, U4>,
{
    transition_model: MatrixN<R, U4>,
    transition_model_transpose: MatrixN<R, U4>,
    transition_noise_covariance: MatrixN<R, U4>,
}

impl<R: RealField> TransitionModelLinearNoControl<R, U4> for MotionModel2DFixedDt<R>
where
    DefaultAllocator: Allocator<R, U4, U4>,
    DefaultAllocator: Allocator<R, U4>,
{
    fn F(&self) -> &MatrixN<R, U4> {
        &self.transition_model
    }
    fn FT(&self) -> &MatrixN<R, U4> {
        &self.transition_model_transpose
    }
    fn Q(&self) -> &MatrixN<R, U4> {
        &self.transition_noise_covariance
    }
}
