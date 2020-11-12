use nalgebra::allocator::Allocator;
use nalgebra::core::{MatrixMN, VectorN};
use nalgebra::dimension::{Dim, DimDiff, DimMin, DimMinimum, DimSub, U1};
use nalgebra::{DefaultAllocator, RealField};

pub struct Lstsq<R: RealField, N: Dim>
where
    DefaultAllocator: Allocator<R, N>,
{
    pub solution: VectorN<R, N>,
    pub residuals: R,
    pub rank: usize,
}

/// Return the least-squares solution to a linear matrix equation.
///
/// Usage is maximally compatible with Python's `numpy.linalg.lstsq`.
pub fn lstsq<R, M, N>(
    a: &MatrixMN<R, M, N>,
    b: &VectorN<R, M>,
    epsilon: R,
) -> Result<Lstsq<R, N>, &'static str>
where
    R: RealField,
    M: DimMin<N>,
    N: Dim,
    DimMinimum<M, N>: DimSub<U1>, // for Bidiagonal.
    DefaultAllocator: Allocator<R, M, N>
        + Allocator<R, N>
        + Allocator<R, M>
        + Allocator<R, DimDiff<DimMinimum<M, N>, U1>>
        + Allocator<R, DimMinimum<M, N>, N>
        + Allocator<R, M, DimMinimum<M, N>>
        + Allocator<R, DimMinimum<M, N>>,
{
    // calculate solution with epsilon
    let svd = nalgebra::linalg::SVD::new(a.clone(), true, true);
    let solution = svd.solve(&b, epsilon)?;

    // calculate residuals
    let model: VectorN<R, M> = a * &solution;
    let l1: VectorN<R, M> = model - b;
    let residuals: R = l1.dot(&l1);

    // calculate rank with epsilon
    let rank = svd.rank(epsilon);

    Ok(Lstsq {
        solution,
        residuals,
        rank,
    })
}

#[cfg(test)]
mod tests {
    use crate::lstsq;

    use na::{MatrixMN, RealField, VectorN, U2};
    use nalgebra as na;

    fn check_residuals<R: RealField>(epsilon: R) {
        /*
        import numpy as np
        A = np.array([[1.0, 1.0], [2.0, 1.0], [3.0, 1.0], [4.0, 1.0]])
        b = np.array([2.5, 4.4, 6.6, 8.5])
        x,residuals,rank,s = np.linalg.lstsq(A,b)
        */
        let a: Vec<R> = vec![1.0, 1.0, 2.0, 1.0, 3.0, 1.0, 4.0, 1.0]
            .into_iter()
            .map(na::convert)
            .collect();

        let a = MatrixMN::<R, na::Dynamic, U2>::from_row_slice(&a);

        let b_data: Vec<R> = vec![2.5, 4.4, 6.6, 8.5]
            .into_iter()
            .map(na::convert)
            .collect();
        let b = VectorN::<R, na::Dynamic>::from_row_slice(&b_data);

        let results = lstsq(&a, &b, R::default_epsilon()).unwrap();
        assert_eq!(results.solution.nrows(), 2);
        approx::assert_relative_eq!(results.solution[0], na::convert(2.02), epsilon = epsilon);
        approx::assert_relative_eq!(results.solution[1], na::convert(0.45), epsilon = epsilon);
        approx::assert_relative_eq!(results.residuals, na::convert(0.018), epsilon = epsilon);
        assert_eq!(results.rank, 2);
    }

    #[test]
    fn test_residuals_f64() {
        check_residuals::<f64>(1e-14)
    }

    #[test]
    fn test_residuals_f32() {
        check_residuals::<f32>(1e-5)
    }
}
