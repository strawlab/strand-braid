use crate::Result;
use na::RealField;
use nalgebra as na;

#[derive(Debug, Clone)]
pub(crate) struct RootParams<R: RealField + Copy> {
    n1: R,
    n2: R,
    z1: R,
    h: R,
    z2: R,
    eps: R,
}

impl<R: RealField + Copy> RootParams<R> {
    pub(crate) fn new(n1: R, n2: R, z1: R, h: R, z2: R, eps: R) -> Self {
        Self {
            n1,
            n2,
            z1,
            h,
            z2,
            eps,
        }
    }
}

/// This code calculates rays according to [Fermat's principle of least
/// time](http://en.wikipedia.org/wiki/Fermat's_principle). Light
/// traveling from point 1 to point 2 (or vice-versa) takes the path of
/// least time.
///
/// ```text
///
///  1--
///  ^  \---                      medium 1
///  |       \----
///  |            \---
///  z1               \----
///  |                     \---
///  |                         \----
///  V                              \---
///  ====================================0====== interface
///  ^                                    \
///  |                                     \
///  z2                                     \
///  |      medium 2                         \
///  v                                        \
///  -                                         2
///
///  |<--------------h1----------------->|
///  |<--------------------h------------------>|
///                                      |<-h2>|
///
/// ```
///
/// Arguments:
///
/// * n1: refractive index of medium 1
/// * n2: refractive index of medium 2
/// * z1: height of point 1 (always positive)
/// * z2: depth  of point 2 (always positive)
/// * h:  horizontal distance between points 1,2 (always positive)
///
/// Returns:
///
/// * h1: horizontal distance between points 1,0 (always positive)
///
/// The solution was obtained by analytically finding the value of h1
/// for which the derivative of the duration is zero. Duration is
/// `n1*sqrt( h1*h1 + z1*z1 ) + n2*sqrt(z2*z2 + h2*h2)`. See
/// [this](https://github.com/strawlab/flydra/blob/master/flydra_core/sympy_demo/refraction_demo.py)
/// for the full factorization.
pub(crate) fn find_fastest_path_fermat<R: RealField + Copy>(params: &RootParams<R>) -> Result<R> {
    let RootParams {
        n1,
        n2,
        z1,
        h,
        z2,
        eps,
    } = params.clone();

    // TODO: implement this https://math.stackexchange.com/a/151249 and
    // forget about this quartic root approach.

    if z2 == na::convert(0.0) {
        return Ok(h);
    }

    let refraction_eq = refraction::RefractionEq {
        d: h,
        h: z2,
        w: z1,
        n: n2 / n1,
    };

    // Evaluate until a given tolerance.
    let h2 = refraction::find_root(
        na::convert(0.0), // Initial guess - start bound
        h,                // Initial guess - end bound
        refraction_eq,    // Parameters
        eps,              // Tolerance
    )
    .ok_or(crate::FlydraMvgError::NoValidRootFound)?; // Not really NotEnoughPoints
    let h1 = h - h2;
    Ok(h1)
}

#[cfg(test)]
#[test]
fn test_find_fastest_path_fermat() {
    {
        let n1 = 1.0;
        let n2 = 1.3;
        let z1 = 1.0;
        let h = 10.0;
        let z2 = 0.1;
        let eps = 1e-20;
        let h1_expected = 9.881096304310466;

        let params = RootParams::new(n1, n2, z1, h, z2, eps);
        let h1 = find_fastest_path_fermat::<f64>(&params).unwrap();
        assert!((h1 - h1_expected).abs() < 1e-10);
    }

    {
        let n1 = 1.0;
        let n2 = 1.3;
        let z1 = 1.0;
        let h = 10.0;
        let z2 = 1.0;
        let eps = 1e-20;
        let h1_expected = 8.814678829560554;

        let params = RootParams::new(n1, n2, z1, h, z2, eps);
        let h1 = find_fastest_path_fermat::<f64>(&params).unwrap();
        assert!((h1 - h1_expected).abs() < 1e-10);
    }
}
