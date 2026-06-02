//! Initial intrinsic-matrix estimate from per-view homographies.
//!
//! Mirrors the strategy of OpenCV's `cvInitIntrinsicParams2D`: fix the
//! principal point at the image center and solve a linear least-squares for the
//! focal lengths using the rotation orthonormality constraints implied by each
//! homography. This is only an initializer for the subsequent
//! Levenberg-Marquardt refinement, which recovers the principal point too.
//!
//! For a homography `H = [h1 h2 h3]` mapping the planar target to the image,
//! `K^{-1} H = scale * [r1 r2 t]` with `r1 ⟂ r2` and `|r1| = |r2|`. With the
//! principal point known and zero skew, writing `a = 1/fx^2`, `c = 1/fy^2`, and
//! for each homography column `pⱼ = h0ⱼ - cx·h2ⱼ`, `qⱼ = h1ⱼ - cy·h2ⱼ`,
//! `rⱼ = h2ⱼ`, the two constraints become linear in `(a, c)`:
//!
//! ```text
//!   a·p1p2        + c·q1q2          = -r1·r2
//!   a·(p1²-p2²)   + c·(q1²-q2²)     = -(r1²-r2²)
//! ```

use nalgebra::{DMatrix, DVector, Matrix3};

/// Initial pinhole intrinsics `(fx, fy, cx, cy)`.
#[derive(Clone, Copy, Debug)]
pub struct InitialIntrinsics {
    pub fx: f64,
    pub fy: f64,
    pub cx: f64,
    pub cy: f64,
}

/// Estimate initial intrinsics from per-view homographies and the image size.
///
/// Returns `None` if there are too few views or the focal-length solution is
/// non-physical (non-positive squared focal length).
pub fn init_intrinsics(
    homographies: &[Matrix3<f64>],
    width: u32,
    height: u32,
) -> Option<InitialIntrinsics> {
    if homographies.is_empty() {
        return None;
    }

    let cx = (width as f64 - 1.0) * 0.5;
    let cy = (height as f64 - 1.0) * 0.5;

    let mut a = DMatrix::<f64>::zeros(2 * homographies.len(), 2);
    let mut b = DVector::<f64>::zeros(2 * homographies.len());

    for (i, h) in homographies.iter().enumerate() {
        // Column j of H, with the principal point removed.
        let col = |j: usize| {
            let h0 = h[(0, j)];
            let h1 = h[(1, j)];
            let h2 = h[(2, j)];
            (h0 - cx * h2, h1 - cy * h2, h2)
        };
        let (p1, q1, r1) = col(0);
        let (p2, q2, r2) = col(1);

        let row0 = 2 * i;
        let row1 = row0 + 1;

        a[(row0, 0)] = p1 * p2;
        a[(row0, 1)] = q1 * q2;
        b[row0] = -r1 * r2;

        a[(row1, 0)] = p1 * p1 - p2 * p2;
        a[(row1, 1)] = q1 * q1 - q2 * q2;
        b[row1] = -(r1 * r1 - r2 * r2);
    }

    // Least-squares solve for (a, c) = (1/fx^2, 1/fy^2) via the normal equations.
    let ata = a.transpose() * &a;
    let atb = a.transpose() * &b;
    let sol = ata.lu().solve(&atb)?;
    let (inv_fx2, inv_fy2) = (sol[0], sol[1]);

    if !(inv_fx2 > 0.0) || !(inv_fy2 > 0.0) {
        return None;
    }

    Some(InitialIntrinsics {
        fx: (1.0 / inv_fx2).sqrt(),
        fy: (1.0 / inv_fy2).sqrt(),
        cx,
        cy,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nalgebra::{Matrix3, Rotation3, Vector3};

    /// Build a homography `H = K [r1 r2 t]` for a planar target.
    fn homography_for_view(k: &Matrix3<f64>, r: &Rotation3<f64>, t: &Vector3<f64>) -> Matrix3<f64> {
        let rm = r.matrix();
        let mut m = Matrix3::zeros();
        m.set_column(0, &rm.column(0));
        m.set_column(1, &rm.column(1));
        m.set_column(2, t);
        k * m
    }

    #[test]
    fn recovers_focal_lengths() {
        let (w, h) = (640u32, 480u32);
        let cx = (w as f64 - 1.0) * 0.5;
        let cy = (h as f64 - 1.0) * 0.5;
        let (fx, fy) = (520.0, 510.0);
        let k = Matrix3::new(fx, 0.0, cx, 0.0, fy, cy, 0.0, 0.0, 1.0);

        // A handful of distinct views looking at the target.
        let views = [
            (
                Rotation3::from_euler_angles(0.1, -0.2, 0.05),
                Vector3::new(-1.0, -0.5, 8.0),
            ),
            (
                Rotation3::from_euler_angles(-0.15, 0.1, -0.1),
                Vector3::new(0.5, -1.0, 7.0),
            ),
            (
                Rotation3::from_euler_angles(0.2, 0.25, 0.0),
                Vector3::new(1.0, 0.5, 9.0),
            ),
            (
                Rotation3::from_euler_angles(-0.05, -0.3, 0.15),
                Vector3::new(-0.5, 1.0, 6.5),
            ),
        ];

        let homs: Vec<Matrix3<f64>> = views
            .iter()
            .map(|(r, t)| homography_for_view(&k, r, t))
            .collect();

        let est = init_intrinsics(&homs, w, h).expect("intrinsics");
        approx::assert_abs_diff_eq!(est.fx, fx, epsilon = 1e-6);
        approx::assert_abs_diff_eq!(est.fy, fy, epsilon = 1e-6);
        approx::assert_abs_diff_eq!(est.cx, cx, epsilon = 1e-12);
        approx::assert_abs_diff_eq!(est.cy, cy, epsilon = 1e-12);
    }
}
