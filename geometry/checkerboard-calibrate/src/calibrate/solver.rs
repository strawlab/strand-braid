//! Joint Levenberg-Marquardt refinement and the top-level `calibrate_camera`.
//!
//! Pipeline (planar calibration target, Zhang's method):
//!   1. per-view homography ([`super::find_homography`]),
//!   2. initial intrinsics ([`super::init_intrinsics`]),
//!   3. initial per-view extrinsics ([`super::init_extrinsics`]),
//!   4. joint LM refinement of `(fx, fy, cx, cy, k1, k2, p1, p2)` plus each
//!      view's `(rvec, tvec)`, minimizing reprojection error.
//!
//! The distortion model matches OpenCV's `(k1, k2, p1, p2, k3)` with `k3` (and
//! the rational terms k4..k6) fixed at zero, exactly as OpenCV's
//! `CALIB_FIX_K3..K6` flags request. Because the refined optimum of
//! the reprojection cost is what we compare against OpenCV, the initialization
//! only has to be good enough to converge there.

use nalgebra::{DMatrix, DVector, Dyn, Matrix3, Owned, Rotation3, Vector3};

use levenberg_marquardt_sparse::{LeastSquaresProblem, LevenbergMarquardt, SparseJacobian};

use super::{find_homography, init_extrinsics, init_intrinsics};

/// One object<->image correspondence. The object point must be planar
/// (`z == 0`) for the homography-based initialization.
#[derive(Clone, Copy, Debug)]
pub struct CorrespondingPoint {
    pub object_point: (f64, f64, f64),
    pub image_point: (f64, f64),
}

/// Result of [`calibrate_camera`].
#[derive(Clone, Debug)]
pub struct CalibrationResult {
    /// Overall RMS reprojection error in pixels (matches OpenCV's return value).
    pub rms_reprojection_error: f64,
    /// Camera matrix `[fx 0 cx; 0 fy cy; 0 0 1]`, row-major.
    pub camera_matrix: [f64; 9],
    /// Distortion `(k1, k2, p1, p2, k3)`; `k3` is fixed at 0.
    pub distortion_coeffs: [f64; 5],
    /// Per-view Rodrigues rotation vectors.
    pub rvecs: Vec<[f64; 3]>,
    /// Per-view translation vectors.
    pub tvecs: Vec<[f64; 3]>,
    pub image_width: u32,
    pub image_height: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CalibrateError {
    TooFewViews,
    HomographyFailed,
    IntrinsicsFailed,
    ExtrinsicsFailed,
}

impl std::fmt::Display for CalibrateError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for CalibrateError {}

/// Number of shared (intrinsic + distortion) parameters: fx, fy, cx, cy, k1,
/// k2, p1, p2.
const NUM_SHARED: usize = 8;
/// Parameters per view: rvec(3) + tvec(3).
const NUM_PER_VIEW: usize = 6;

/// Shared intrinsic + distortion parameters: `[fx, fy, cx, cy, k1, k2, p1, p2]`.
type SharedParams = [f64; NUM_SHARED];

/// Project one object point. Mirrors OpenCV's radial+tangential model (k3 = 0).
fn project(
    s: &SharedParams,
    rot: &Rotation3<f64>,
    t: &Vector3<f64>,
    obj: (f64, f64, f64),
) -> (f64, f64) {
    let [fx, fy, cx, cy, k1, k2, p1, p2] = *s;
    let xc = rot * Vector3::new(obj.0, obj.1, obj.2) + t;
    let xp = xc.x / xc.z;
    let yp = xc.y / xc.z;
    let r2 = xp * xp + yp * yp;
    let radial = 1.0 + (k1 + k2 * r2) * r2;
    let xpp = xp * radial + 2.0 * p1 * xp * yp + p2 * (r2 + 2.0 * xp * xp);
    let ypp = yp * radial + p1 * (r2 + 2.0 * yp * yp) + 2.0 * p2 * xp * yp;
    (fx * xpp + cx, fy * ypp + cy)
}

struct CalibProblem {
    /// Per view: object points and observed image points.
    views: Vec<Vec<CorrespondingPoint>>,
    num_points: usize,
    params: DVector<f64>,
}

impl CalibProblem {
    fn rot_t(p: &DVector<f64>, view: usize) -> (Rotation3<f64>, Vector3<f64>) {
        let base = NUM_SHARED + view * NUM_PER_VIEW;
        let rvec = Vector3::new(p[base], p[base + 1], p[base + 2]);
        let tvec = Vector3::new(p[base + 3], p[base + 4], p[base + 5]);
        (Rotation3::new(rvec), tvec)
    }

    /// Stacked residuals (observed - projected), x then y per point, at `p`.
    fn residuals_at(&self, p: &DVector<f64>) -> DVector<f64> {
        let s: SharedParams = [p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7]];

        let mut out = DVector::zeros(2 * self.num_points);
        let mut i = 0;
        for (vi, view) in self.views.iter().enumerate() {
            let (rot, t) = Self::rot_t(p, vi);
            for cp in view {
                let (u, v) = project(&s, &rot, &t, cp.object_point);
                out[i] = cp.image_point.0 - u;
                out[i + 1] = cp.image_point.1 - v;
                i += 2;
            }
        }
        out
    }
}

impl LeastSquaresProblem<f64, Dyn, Dyn> for CalibProblem {
    type ParameterStorage = Owned<f64, Dyn>;
    type ResidualStorage = Owned<f64, Dyn>;

    fn set_params(&mut self, x: &DVector<f64>) {
        self.params = x.clone();
    }

    fn params(&self) -> DVector<f64> {
        self.params.clone()
    }

    fn residuals(&self) -> Option<DVector<f64>> {
        Some(self.residuals_at(&self.params))
    }

    fn jacobian(&self) -> Option<SparseJacobian<f64>> {
        // Central-difference Jacobian of the residuals w.r.t. each parameter.
        let n = self.params.len();
        let m = 2 * self.num_points;
        let mut j = DMatrix::<f64>::zeros(m, n);

        let mut p = self.params.clone();
        for col in 0..n {
            let x0 = p[col];
            // Step scaled to the parameter magnitude for good conditioning.
            let h = 1e-6 * x0.abs().max(1e-3);
            p[col] = x0 + h;
            let rp = self.residuals_at(&p);
            p[col] = x0 - h;
            let rm = self.residuals_at(&p);
            p[col] = x0;

            let inv = 0.5 / h;
            for row in 0..m {
                j[(row, col)] = (rp[row] - rm[row]) * inv;
            }
        }
        Some(SparseJacobian::from_dense(j))
    }
}

/// Calibrate a pinhole+distortion camera from planar-target correspondences.
pub fn calibrate_camera(
    views: &[Vec<CorrespondingPoint>],
    width: u32,
    height: u32,
) -> Result<CalibrationResult, CalibrateError> {
    if views.len() < 3 {
        return Err(CalibrateError::TooFewViews);
    }

    // 1. Homographies.
    let mut homographies = Vec::with_capacity(views.len());
    for view in views {
        let src: Vec<(f64, f64)> = view
            .iter()
            .map(|c| (c.object_point.0, c.object_point.1))
            .collect();
        let dst: Vec<(f64, f64)> = view.iter().map(|c| c.image_point).collect();
        let h = find_homography(&src, &dst).ok_or(CalibrateError::HomographyFailed)?;
        homographies.push(h);
    }

    // 2. Initial intrinsics.
    let intr =
        init_intrinsics(&homographies, width, height).ok_or(CalibrateError::IntrinsicsFailed)?;
    let k = Matrix3::new(intr.fx, 0.0, intr.cx, 0.0, intr.fy, intr.cy, 0.0, 0.0, 1.0);

    // 3. Initial extrinsics + parameter vector.
    let n = views.len();
    let mut params = DVector::<f64>::zeros(NUM_SHARED + n * NUM_PER_VIEW);
    params[0] = intr.fx;
    params[1] = intr.fy;
    params[2] = intr.cx;
    params[3] = intr.cy;
    // distortion starts at zero (indices 4..8).

    for (vi, h) in homographies.iter().enumerate() {
        let ext = init_extrinsics(&k, h).ok_or(CalibrateError::ExtrinsicsFailed)?;
        let rvec = ext.rotation.scaled_axis();
        let base = NUM_SHARED + vi * NUM_PER_VIEW;
        params[base] = rvec[0];
        params[base + 1] = rvec[1];
        params[base + 2] = rvec[2];
        params[base + 3] = ext.translation[0];
        params[base + 4] = ext.translation[1];
        params[base + 5] = ext.translation[2];
    }

    // 4. Joint LM refinement.
    let num_points: usize = views.iter().map(|v| v.len()).sum();
    let problem = CalibProblem {
        views: views.to_vec(),
        num_points,
        params,
    };
    let (problem, _report) = LevenbergMarquardt::new().minimize(problem);

    let p = problem.params();
    let ssq: f64 = problem.residuals_at(&p).iter().map(|r| r * r).sum();
    let rms = (ssq / num_points as f64).sqrt();

    let mut rvecs = Vec::with_capacity(n);
    let mut tvecs = Vec::with_capacity(n);
    for vi in 0..n {
        let base = NUM_SHARED + vi * NUM_PER_VIEW;
        rvecs.push([p[base], p[base + 1], p[base + 2]]);
        tvecs.push([p[base + 3], p[base + 4], p[base + 5]]);
    }

    Ok(CalibrationResult {
        rms_reprojection_error: rms,
        camera_matrix: [p[0], 0.0, p[2], 0.0, p[1], p[3], 0.0, 0.0, 1.0],
        distortion_coeffs: [p[4], p[5], p[6], p[7], 0.0],
        rvecs,
        tvecs,
        image_width: width,
        image_height: height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Generate a synthetic planar-target dataset with known intrinsics,
    /// distortion, and per-view poses, then check calibration recovers them.
    #[test]
    fn recovers_synthetic_calibration() {
        let (w, h) = (640u32, 480u32);
        let (fx, fy, cx, cy) = (525.0, 530.0, 320.0, 240.0);
        let (k1, k2, p1, p2) = (-0.25, 0.08, 0.001, -0.0015);

        // A 9x6 planar grid, unit spacing, centered at origin.
        let mut obj = Vec::new();
        for r in 0..6 {
            for c in 0..9 {
                obj.push((c as f64 - 4.0, r as f64 - 2.5, 0.0));
            }
        }

        let poses = [
            (
                Vector3::new(0.05, -0.1, 0.02),
                Vector3::new(-1.0, -0.5, 12.0),
            ),
            (
                Vector3::new(-0.2, 0.15, -0.05),
                Vector3::new(0.8, -1.0, 11.0),
            ),
            (Vector3::new(0.1, 0.25, 0.1), Vector3::new(1.0, 0.5, 13.0)),
            (
                Vector3::new(-0.15, -0.2, 0.07),
                Vector3::new(-0.7, 1.0, 10.5),
            ),
            (
                Vector3::new(0.22, 0.05, -0.12),
                Vector3::new(0.2, 0.3, 12.5),
            ),
        ];

        let views: Vec<Vec<CorrespondingPoint>> = poses
            .iter()
            .map(|(rvec, t)| {
                let rot = Rotation3::new(*rvec);
                let s = [fx, fy, cx, cy, k1, k2, p1, p2];
                obj.iter()
                    .map(|&o| {
                        let (u, v) = project(&s, &rot, t, o);
                        CorrespondingPoint {
                            object_point: o,
                            image_point: (u, v),
                        }
                    })
                    .collect()
            })
            .collect();

        let res = calibrate_camera(&views, w, h).expect("calibration");

        approx::assert_abs_diff_eq!(res.camera_matrix[0], fx, epsilon = 0.5);
        approx::assert_abs_diff_eq!(res.camera_matrix[4], fy, epsilon = 0.5);
        approx::assert_abs_diff_eq!(res.camera_matrix[2], cx, epsilon = 0.5);
        approx::assert_abs_diff_eq!(res.camera_matrix[5], cy, epsilon = 0.5);
        approx::assert_abs_diff_eq!(res.distortion_coeffs[0], k1, epsilon = 1e-3);
        approx::assert_abs_diff_eq!(res.distortion_coeffs[1], k2, epsilon = 1e-3);
        // Noise-free data: reprojection error should be ~0.
        assert!(
            res.rms_reprojection_error < 1e-3,
            "rms {}",
            res.rms_reprojection_error
        );
    }
}
