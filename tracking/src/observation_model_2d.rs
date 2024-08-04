use num_traits::{One, Zero};

use nalgebra::{
    allocator::Allocator,
    dimension::{DimMin, U2, U4},
    DefaultAllocator, OMatrix, OVector, RealField,
};

use adskalman::ObservationModel;

#[derive(Debug)]
pub struct ObservationModel2D<R: RealField + Copy> {
    observation_matrix: OMatrix<R, U2, U4>,
    observation_matrix_transpose: OMatrix<R, U4, U2>,
    observation_noise_covariance: OMatrix<R, U2, U2>,
}

impl<R: RealField + Copy> ObservationModel2D<R> {
    pub fn new(observation_noise_covariance: OMatrix<R, U2, U2>) -> Self {
        let zero: R = Zero::zero();
        let one: R = One::one();

        #[rustfmt::skip]
        let observation_matrix = OMatrix::<R,U2,U4>::new(
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

impl<R: RealField + Copy> ObservationModel<R, U4, U2> for ObservationModel2D<R>
where
    DefaultAllocator: Allocator<U4, U4>,
    DefaultAllocator: Allocator<U4>,
    DefaultAllocator: Allocator<U2, U4>,
    DefaultAllocator: Allocator<U4, U2>,
    DefaultAllocator: Allocator<U2, U2>,
    DefaultAllocator: Allocator<U2>,
    U2: DimMin<U2, Output = U2>,
{
    fn H(&self) -> &OMatrix<R, U2, U4> {
        &self.observation_matrix
    }
    fn HT(&self) -> &OMatrix<R, U4, U2> {
        &self.observation_matrix_transpose
    }
    fn R(&self) -> &OMatrix<R, U2, U2> {
        &self.observation_noise_covariance
    }
    fn predict_observation(&self, state: &OVector<R, U4>) -> OVector<R, U2> {
        &self.observation_matrix * state
    }
}
