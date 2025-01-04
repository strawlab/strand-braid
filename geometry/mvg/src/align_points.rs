use nalgebra::{
    allocator::Allocator, DefaultAllocator, Dyn, Matrix, OMatrix, RealField, VecStorage, U1, U3,
};
use num_traits::float::TotalOrder;

use crate::{MvgError, Result};

pub enum Algorithm {
    /// The Kabsch-Umeyama algorithm
    KabschUmeyama,
    /// A robustly-sclaed variant of the Arun, Huang, and Blostein algorithm
    RobustArun,
}

/// Find the linear transformation that converts 3D points `x` as close as
/// possible to points `y`.
///
/// The best (scale, rotation, translation) are returned.
///
/// The Kabsch-Umeyama implementation is based on that in
/// https://github.com/clementinboittiaux/umeyama-python/blob/main/umeyama.py.
///
/// The robust Arun implementation is based on that in
/// https://github.com/strawlab/MultiCamSelfCal/blob/main/MultiCamSelfCal/CoreFunctions/estsimt.m.
/// That code claims to be an implementation of the Arun, Huang, and Blostein
/// algorithm, but contains an extra bit to determine scaling which works
/// differently, and in my experience is more robust than, the Kabsch-Umeyama
/// algorithm.
pub fn align_points<T>(
    x: &OMatrix<T, U3, Dyn>,
    y: &OMatrix<T, U3, Dyn>,
    algorithm: Algorithm,
) -> Result<(T, OMatrix<T, U3, U3>, OMatrix<T, U3, U1>)>
where
    T: RealField + Copy + TotalOrder,
{
    let n = x.ncols();

    if n != y.ncols() {
        return Err(MvgError::InvalidShape);
    }
    if n < 1 {
        return Err(MvgError::InvalidShape);
    }

    // Find centroids.
    let mu_x = x.column_mean();
    let mu_y = y.column_mean();

    // Move points to center.
    let x_center = x - bcast(&mu_x, n);
    let y_center = y - bcast(&mu_y, n);

    // Covariance of X,Y
    let (robust_scale, cov_xy) = match algorithm {
        Algorithm::RobustArun => {
            let dx = x.columns(1, n - 1) - x.columns(0, n - 1);
            let dy = y.columns(1, n - 1) - y.columns(0, n - 1);
            let dx = sqrt(&square(&dx).row_sum());
            let dy = sqrt(&square(&dy).row_sum());
            let scales = dy.component_div(&dx);

            let scale = median(&scales).unwrap();

            let x_centered_scaled = &x_center * scale;

            let cov_xy = &x_centered_scaled * y_center.transpose();
            (Some(scale), cov_xy)
        }
        Algorithm::KabschUmeyama => {
            let cov_xy = (y_center * x_center.transpose()) / nalgebra::convert::<_, T>(n as f64);
            (None, cov_xy)
        }
    };

    // Decomposition of covariance matrix.
    let svd = if let Some(svd) =
        nalgebra::linalg::SVD::try_new(cov_xy, true, true, nalgebra::convert(1e-7), 0)
    {
        svd
    } else {
        return Err(MvgError::SvdFailed);
    };
    let u = svd.u.unwrap();
    let d = svd.singular_values;
    let vh = svd.v_t.unwrap();

    // Generate rotation matrix
    let (c, r) = if let Some(scale) = robust_scale {
        let v = vh.transpose();
        let ut = u.transpose();
        (scale, v * ut)
    } else {
        let mut s = nalgebra::Matrix3::<T>::identity();

        // Are the points reflected?
        if u.determinant() * vh.determinant() < nalgebra::convert(0.0) {
            s[(2, 2)] = nalgebra::convert(-1.0);
        }

        // Variance of X
        let var_x = square(&x_center).row_sum().mean();
        let c = (nalgebra::Matrix3::from_diagonal(&d) * s).trace() / var_x;
        (c, u * s * vh)
    };

    // Translation
    let t = mu_y - (r * mu_x) * c;

    Ok((c, r, t))
}

fn bcast<T, R>(m: &OMatrix<T, R, U1>, n: usize) -> OMatrix<T, R, Dyn>
where
    T: RealField + Copy,
    R: nalgebra::DimName,
    DefaultAllocator: Allocator<R>,
{
    // this is far from efficient
    let mut result = OMatrix::<T, R, Dyn>::zeros(n);
    for i in 0..R::dim() {
        for j in 0..n {
            result[(i, j)] = m[(i, 0)];
        }
    }
    result
}

fn sqrt<T, R, C>(m: &OMatrix<T, R, C>) -> OMatrix<T, R, C>
where
    T: RealField + Copy,
    R: nalgebra::Dim,
    C: nalgebra::Dim,
    DefaultAllocator: Allocator<R, C>,
{
    let mut result = m.clone();
    sqrt_in_place(&mut result);
    result
}

fn sqrt_in_place<T, R, C>(m: &mut OMatrix<T, R, C>)
where
    T: RealField + Copy,
    R: nalgebra::Dim,
    C: nalgebra::Dim,
    DefaultAllocator: Allocator<R, C>,
{
    for el in m.iter_mut() {
        let val: T = *el;
        *el = val.sqrt();
    }
}

fn square<T, R, C>(m: &OMatrix<T, R, C>) -> OMatrix<T, R, C>
where
    T: RealField + Copy,
    R: nalgebra::Dim,
    C: nalgebra::Dim,
    DefaultAllocator: Allocator<R, C>,
{
    m.component_mul(m)
}

fn median<T, C>(scales: &Matrix<T, U1, C, VecStorage<T, U1, C>>) -> Option<T>
where
    T: RealField + Copy + TotalOrder,
    C: nalgebra::Dim,
    DefaultAllocator: Allocator<U1, C>,
{
    let mut scales = scales.data.as_slice().to_vec(); // clone data to vec

    scales.as_mut_slice().sort_by(|a, b| a.total_cmp(b));

    let n = scales.len();
    if n == 0 {
        None
    } else if n == 1 {
        Some(scales[0])
    } else if n % 2 == 0 {
        let s1 = scales[n / 2 - 1];
        let s2 = scales[n / 2];
        Some((s1 + s2) * nalgebra::convert(0.5))
    } else {
        // odd
        Some(scales[n / 2])
    }
}

#[test]
fn test_median() {
    let mut a = OMatrix::<f64, U1, Dyn>::zeros(3);
    a[(0, 0)] = 1.0;
    a[(0, 1)] = 2.0;
    a[(0, 2)] = 3.0;
    assert_eq!(median(&a), Some(2.0));

    let mut a = OMatrix::<f64, U1, Dyn>::zeros(2);
    a[(0, 0)] = 1.0;
    a[(0, 1)] = 2.0;
    assert_eq!(median(&a), Some(1.5));
}

#[test]
fn test_square() {
    let a = nalgebra::Matrix2::new(0., 1., 2., 3.);
    let b = square(&a);
    assert_eq!(b, nalgebra::Matrix2::new(0., 1., 4., 9.));
}

#[test]
fn test_align_points() {
    use nalgebra::{Matrix3, Vector3};

    #[rustfmt::skip]
    // This is transposed because we are using `from_column_slice()`.
    let x1 = nalgebra::base::Matrix3xX::from_column_slice(&[
        3.36748406,1.61036404,3.55147255,
        3.58702265,0.06676394,3.64695356,
        0.28452026,-0.11188296,3.78947735,
        0.25482713,1.57828256,3.6900808,
        3.54938525,1.74057692,5.13329681,
        3.6855626,0.10335229,5.26344841,
        0.25025385,-0.06146044,5.57085135,
        0.20742481,1.71073272,5.41823085]);

    #[rustfmt::skip]
    let x2_noisy = nalgebra::base::Matrix3xX::from_column_slice(&[
        3.048,1.524,1.524,
        3.048,0.0,1.524,
        0.0,0.0,1.524,
        0.0,1.524,1.524,
        3.048,1.524,0.0,
        3.048,0.0,0.0,
        0.0,0.0,0.0,
        0.0,1.524,0.0]);

    for algorithm in [Algorithm::KabschUmeyama, Algorithm::RobustArun] {
        // Test in noise-free conditions with generated data.
        let c_expected = 0.1;
        let r_expected =
            nalgebra::geometry::Rotation3::from_euler_angles(std::f64::consts::FRAC_PI_4, 0.0, 0.0)
                .matrix()
                .clone();
        let t_expected = Vector3::new(-0.2, 0.3, -0.4);

        let x2 = c_expected * r_expected * &x1 + bcast(&t_expected, 8);

        let (c, r, t) = align_points(&x1, &x2, algorithm).unwrap();

        approx::assert_abs_diff_eq!(c, c_expected);
        approx::assert_abs_diff_eq!(r, r_expected, epsilon = 1e-10);
        approx::assert_abs_diff_eq!(t, t_expected, epsilon = 1e-10);
    }

    // Test on some real data which seems problematic for Kabsch-Umeyama using
    // the robust scale option.
    let (c, r, t) = align_points(&x1, &x2_noisy, Algorithm::RobustArun).unwrap();

    // These values were generated by running on this data using `estsimt()` in
    // `align.py` from flydra.
    let c_expected = 0.920734586302497;
    #[rustfmt::skip]
        let r_expected = {
            Matrix3::new(
                0.997554805278945, 0.03689676080610408, -0.05935519780863721,
                -0.04056669686950421, 0.9972599534887207, -0.06186217158144404,
                -0.05691004805816868, -0.06411875084319214, -0.9963182384260189,
            )
        };
    let t_expected = Vector3::new(
        -0.0013862645696010034,
        0.3279319869522358,
        5.0458138154244985,
    );

    approx::assert_abs_diff_eq!(c, c_expected);
    approx::assert_abs_diff_eq!(r, r_expected, epsilon = 1e-10);
    approx::assert_abs_diff_eq!(t, t_expected, epsilon = 1e-10);

    // let xformed = c * r * x1 + bcast(&t, 8);
    // println!("xformed{xformed}");
    // let p = c * r;
    // let mut pp = nalgebra::Matrix4::zeros();
    // let mut ul = pp.fixed_view_mut::<3, 3>(0, 0);
    // ul.set_row(0, &p.row(0));
    // ul.set_row(1, &p.row(1));
    // ul.set_row(2, &p.row(2));
    // pp[(0, 3)] = t[0];
    // pp[(1, 3)] = t[1];
    // pp[(2, 3)] = t[2];

    // println!("pp\n{}", &pp);
}
