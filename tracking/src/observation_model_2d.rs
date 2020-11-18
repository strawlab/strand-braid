use num_traits::{Zero, One};

use nalgebra::{DefaultAllocator, RealField};
use nalgebra::core::{VectorN, MatrixN, MatrixMN};
use nalgebra::core::dimension::DimMin;
use nalgebra::core::dimension::{U2, U4};
use nalgebra::allocator::Allocator;

use adskalman::ObservationModelLinear;

#[derive(Debug)]
pub struct ObservationModel2D<R: RealField>{
    observation_matrix: MatrixMN<R,U2,U4>,
    observation_matrix_transpose: MatrixMN<R,U4,U2>,
    observation_noise_covariance: MatrixN<R,U2>,
}

impl<R: RealField> ObservationModel2D<R> {
    pub fn new(observation_noise_covariance: MatrixN<R,U2>) -> Self
    {
        let zero: R = Zero::zero();
        let one: R = One::one();

        let observation_matrix = MatrixMN::<R,U2,U4>::new(
                          one, zero, zero, zero,
                         zero,  one, zero, zero);
        let observation_matrix_transpose = observation_matrix.transpose();
        Self {
            observation_matrix,
            observation_matrix_transpose,
            observation_noise_covariance,
        }
    }
}

impl<R: RealField> ObservationModelLinear<R, U4, U2> for ObservationModel2D<R>
    where DefaultAllocator: Allocator<R, U4, U4>,
          DefaultAllocator: Allocator<R, U4>,
          DefaultAllocator: Allocator<R, U2, U4>,
          DefaultAllocator: Allocator<R, U4, U2>,
          DefaultAllocator: Allocator<R, U2, U2>,
          DefaultAllocator: Allocator<R, U2>,
          DefaultAllocator: Allocator<(usize, usize), U2>,
          U2: DimMin<U2, Output = U2>,
{
    fn observation_matrix(&self) -> &MatrixMN<R,U2,U4> {
        &self.observation_matrix
    }
    fn observation_matrix_transpose(&self) -> &MatrixMN<R,U4,U2> {
        &self.observation_matrix_transpose
    }
    fn observation_noise_covariance(&self) -> &MatrixN<R,U2> {
        &self.observation_noise_covariance
    }
    fn evaluate(&self, state: &VectorN<R,U4>) -> VectorN<R,U2> {
        &self.observation_matrix * state
    }

}
