//! Shared utility functions for MCSC.
//!
//! Ports of helper routines from `CoreFunctions/rq.m`,
//! `RadialDistortions/undoradial.m`, `CalTechCal/comp_distortion_oulu.m`,
//! `MartinecPajdla/fill_mm/normu.m`, and `RansacM/pointnormiso.m`.

use nalgebra::{Matrix3, Vector3};

/// RQ decomposition of a 3×3 matrix.
///
/// Returns `(R, Q)` where `R` is upper-triangular, `Q` is orthogonal,
/// and `X = R * Q`.
///
/// Equivalent to `CoreFunctions/rq.m` (Pajdla).
pub(crate) fn rq_decomposition(x: &Matrix3<f64>) -> (Matrix3<f64>, Matrix3<f64>) {
    // QR decomposition of X^T, then transpose back
    let qr = x.transpose().qr();
    let qt_raw = qr.q().transpose();
    let rt_raw = qr.r().transpose();

    // Fix signs to ensure proper orientation
    let mut qu = Matrix3::<f64>::zeros();

    let row2 = Vector3::new(rt_raw[(2, 0)], rt_raw[(2, 1)], rt_raw[(2, 2)]);
    let row1 = Vector3::new(rt_raw[(1, 0)], rt_raw[(1, 1)], rt_raw[(1, 2)]);

    // Qu(1,:) = cross(Rt(2,:), Rt(3,:)), normalized
    let qu_row0 = row1.cross(&row2);
    let qu_row0 = qu_row0 / qu_row0.norm();

    // Qu(2,:) = cross(Qu(1,:), Rt(3,:)), normalized
    let qu_row1 = qu_row0.cross(&row2);
    let qu_row1 = qu_row1 / qu_row1.norm();

    // Qu(3,:) = cross(Qu(1,:), Qu(2,:))
    let qu_row2 = qu_row0.cross(&qu_row1);

    for c in 0..3 {
        qu[(0, c)] = qu_row0[c];
        qu[(1, c)] = qu_row1[c];
        qu[(2, c)] = qu_row2[c];
    }

    let r = rt_raw * qu.transpose();
    let q = qu * qt_raw;

    (r, q)
}

/// Undo radial distortion for a single point.
///
/// Equivalent to `RadialDistortions/undoradial.m` calling
/// `CalTechCal/comp_distortion_oulu.m`.
///
/// * `x_kk` — 3×1 homogeneous pixel coordinates (distorted)
/// * `k` — 3×3 camera calibration matrix
/// * `kc` — 4-element distortion coefficients `[k1, k2, p1, p2]`
///
/// Returns 3×1 linearized (undistorted) pixel coordinates.
pub(crate) fn undo_radial(x_kk: &Vector3<f64>, k: &Matrix3<f64>, kc: &[f64; 4]) -> Vector3<f64> {
    let cc = [k[(0, 2)], k[(1, 2)]];
    let fc = [k[(0, 0)], k[(1, 1)]];

    // Normalize: subtract principal point, divide by focal length
    let x_distort = [(x_kk[0] - cc[0]) / fc[0], (x_kk[1] - cc[1]) / fc[1]];

    let kc_norm = kc.iter().map(|x| x * x).sum::<f64>().sqrt();
    let xn = if kc_norm != 0.0 {
        comp_distortion_oulu(&x_distort, kc)
    } else {
        x_distort
    };

    // Back to linear pixel coordinates: K * [xn; 1]
    Vector3::new(
        k[(0, 0)] * xn[0] + k[(0, 1)] * xn[1] + k[(0, 2)],
        k[(1, 0)] * xn[0] + k[(1, 1)] * xn[1] + k[(1, 2)],
        k[(2, 0)] * xn[0] + k[(2, 1)] * xn[1] + k[(2, 2)],
    )
}

/// Compensate for radial and tangential distortion using iterative method.
///
/// Equivalent to `CalTechCal/comp_distortion_oulu.m` (Oulu university
/// distortion model, 20 fixed iterations).
fn comp_distortion_oulu(xd: &[f64; 2], kc: &[f64; 4]) -> [f64; 2] {
    let k1 = kc[0];
    let k2 = kc[1];
    let p1 = kc[2];
    let p2 = kc[3];
    let k3 = 0.0; // kc(5) is 0 in our case

    let mut x = [xd[0], xd[1]];

    for _ in 0..20 {
        let r_2 = x[0] * x[0] + x[1] * x[1];
        let k_radial = 1.0 + k1 * r_2 + k2 * r_2 * r_2 + k3 * r_2 * r_2 * r_2;
        let delta_x = [
            2.0 * p1 * x[0] * x[1] + p2 * (r_2 + 2.0 * x[0] * x[0]),
            p1 * (r_2 + 2.0 * x[1] * x[1]) + 2.0 * p2 * x[0] * x[1],
        ];
        x[0] = (xd[0] - delta_x[0]) / k_radial;
        x[1] = (xd[1] - delta_x[1]) / k_radial;
    }

    x
}

/// Isotropic point normalization for the Martinec-Pajdla algorithm.
///
/// Equivalent to `MartinecPajdla/fill_mm/normu.m`.
///
/// Returns the 3×3 normalization matrix `T` such that the transformed
/// points `T * u` have zero mean and mean distance `√2` from the origin.
///
/// `u` is a 3 × N matrix of homogeneous points. If `u` has three rows,
/// the points are first converted to Euclidean coordinates (divided by
/// the third row) before computing the mean and scale.
pub(crate) fn normu(u: &nalgebra::DMatrix<f64>) -> Option<Matrix3<f64>> {
    let n = u.ncols();
    if n == 0 {
        return None;
    }

    // Convert to Euclidean (p2e: divide by third coordinate if it's 3xN)
    // The octave code does: if size(u,1)==3, u = p2e(u); end
    // p2e divides first two rows by third row
    let mut pts = Vec::new();
    for j in 0..n {
        let w = u[(2, j)];
        if w.abs() > 1e-15 {
            pts.push([u[(0, j)] / w, u[(1, j)] / w]);
        } else {
            pts.push([u[(0, j)], u[(1, j)]]);
        }
    }

    let n_pts = pts.len();
    if n_pts == 0 {
        return None;
    }

    let mx: f64 = pts.iter().map(|p| p[0]).sum::<f64>() / n_pts as f64;
    let my: f64 = pts.iter().map(|p| p[1]).sum::<f64>() / n_pts as f64;

    let mean_dist: f64 = pts
        .iter()
        .map(|p| ((p[0] - mx).powi(2) + (p[1] - my).powi(2)).sqrt())
        .sum::<f64>()
        / n_pts as f64;

    let r = mean_dist / std::f64::consts::SQRT_2;
    if r == 0.0 {
        return None;
    }

    let mut a = Matrix3::identity();
    a[(0, 0)] = 1.0 / r;
    a[(1, 1)] = 1.0 / r;
    a[(0, 2)] = -mx / r;
    a[(1, 2)] = -my / r;

    Some(a)
}

/// Isotropic point normalization for the DLT F-matrix estimation.
///
/// Equivalent to `RansacM/pointnormiso.m` (see Hartley, "In Defence of
/// the 8-Point Algorithm", ICCV’95).
///
/// `u` is a 3 × N matrix of homogeneous points. Returns
/// `(normalized_points, T)` where `T` is the 3×3 transformation.
pub(crate) fn point_norm_iso(u: &nalgebra::DMatrix<f64>) -> (nalgebra::DMatrix<f64>, Matrix3<f64>) {
    let n = u.ncols();

    let xmean: f64 = u.row(0).sum() / n as f64;
    let ymean: f64 = u.row(1).sum() / n as f64;

    let mut u2 = u.clone();
    for j in 0..n {
        u2[(0, j)] -= xmean;
        u2[(1, j)] -= ymean;
    }

    let mean_dist: f64 = (0..n)
        .map(|j| (u2[(0, j)].powi(2) + u2[(1, j)].powi(2)).sqrt())
        .sum::<f64>()
        / n as f64;

    let scale = std::f64::consts::SQRT_2 / mean_dist;

    for j in 0..n {
        u2[(0, j)] *= scale;
        u2[(1, j)] *= scale;
    }

    let mut t = Matrix3::identity();
    t[(0, 0)] = scale;
    t[(1, 1)] = scale;
    t[(0, 2)] = -scale * xmean;
    t[(1, 2)] = -scale * ymean;

    (u2, t)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify RQ decomposition: M = K * R, where K is upper-triangular
    /// and R is orthogonal.
    fn check_rq(m: &Matrix3<f64>) {
        let (k, r) = rq_decomposition(m);

        // K * R should reconstruct M
        let reconstructed = k * r;
        for i in 0..3 {
            for j in 0..3 {
                assert!(
                    (reconstructed[(i, j)] - m[(i, j)]).abs() < 1e-10,
                    "K*R != M at ({i},{j}): {} vs {}",
                    reconstructed[(i, j)],
                    m[(i, j)]
                );
            }
        }

        // R should be orthogonal: R * R' = I
        let rrt = r * r.transpose();
        for i in 0..3 {
            for j in 0..3 {
                let expected = if i == j { 1.0 } else { 0.0 };
                assert!(
                    (rrt[(i, j)] - expected).abs() < 1e-10,
                    "R*R' not identity at ({i},{j}): {}",
                    rrt[(i, j)]
                );
            }
        }

        // K should be upper-triangular
        assert!(
            k[(1, 0)].abs() < 1e-10,
            "K not upper-triangular: K(1,0)={}",
            k[(1, 0)]
        );
        assert!(
            k[(2, 0)].abs() < 1e-10,
            "K not upper-triangular: K(2,0)={}",
            k[(2, 0)]
        );
        assert!(
            k[(2, 1)].abs() < 1e-10,
            "K not upper-triangular: K(2,1)={}",
            k[(2, 1)]
        );
    }

    #[test]
    fn test_rq_identity_r() {
        // Upper-triangular M should decompose to K=M, R=I
        let m = Matrix3::new(500.0, -3.0, 320.0, 0.0, -510.0, 240.0, 0.0, 0.0, -1.0);
        check_rq(&m);
        let (k, _r) = rq_decomposition(&m);
        // K should be close to M itself
        for i in 0..3 {
            for j in 0..3 {
                assert!(
                    (k[(i, j)] - m[(i, j)]).abs() < 1e-10,
                    "K != M at ({i},{j}): {} vs {}",
                    k[(i, j)],
                    m[(i, j)]
                );
            }
        }
    }

    #[test]
    fn test_rq_general() {
        // Random-ish 3x3 matrix
        let m = Matrix3::new(
            617.88, -8.55, 18.39, //
            23.1, -639.52, 98.05, //
            0.3, -0.1, -1.0,
        );
        check_rq(&m);
    }

    #[test]
    fn test_rq_matches_octave() {
        // Compare with octave output for a known upper-triangular input
        let m = Matrix3::new(500.0, -3.0, 320.0, 0.0, -510.0, 240.0, 0.0, 0.0, -1.0);
        let (k, r) = rq_decomposition(&m);
        // Octave: K = [500 -3 320; 0 -510 240; 0 0 -1], R ≈ I
        assert!((k[(0, 0)] - 500.0).abs() < 1e-8);
        assert!((k[(1, 1)] - (-510.0)).abs() < 1e-8);
        assert!((k[(2, 2)] - (-1.0)).abs() < 1e-8);
        // R should be identity (to machine precision)
        for i in 0..3 {
            for j in 0..3 {
                let expected = if i == j { 1.0 } else { 0.0 };
                assert!(
                    (r[(i, j)] - expected).abs() < 1e-10,
                    "R({i},{j}) = {}, expected {expected}",
                    r[(i, j)]
                );
            }
        }
    }
}
