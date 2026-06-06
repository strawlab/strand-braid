// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Plane-to-plane homography estimation via the normalized DLT.
//!
//! Used to initialize camera calibration: for a planar calibration target
//! (all object points at `z = 0`), each view's object->image mapping is a
//! homography, from which initial intrinsics and extrinsics are recovered.
//!
//! This only needs to be a good *initialization* for the later
//! Levenberg-Marquardt refinement, so it does not have to bit-match OpenCV's
//! `cvFindHomography`; the normalized DLT (Hartley & Zisserman, Alg. 4.2) is a
//! standard, well-conditioned least-squares estimate.

use nalgebra::{DMatrix, Matrix3};

/// Similarity transform that maps a point set to zero centroid and mean
/// distance `sqrt(2)` from the origin, returned as a 3x3 matrix together with
/// the transformed points.
fn normalize(pts: &[(f64, f64)]) -> (Matrix3<f64>, Vec<(f64, f64)>) {
    let n = pts.len() as f64;
    let (mut cx, mut cy) = (0.0, 0.0);
    for &(x, y) in pts {
        cx += x;
        cy += y;
    }
    cx /= n;
    cy /= n;

    let mut mean_dist = 0.0;
    for &(x, y) in pts {
        mean_dist += ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
    }
    mean_dist /= n;

    // Degenerate (all points coincident): fall back to identity scale.
    let scale = if mean_dist > f64::EPSILON {
        2.0f64.sqrt() / mean_dist
    } else {
        1.0
    };

    let t = Matrix3::new(
        scale,
        0.0,
        -scale * cx,
        0.0,
        scale,
        -scale * cy,
        0.0,
        0.0,
        1.0,
    );

    let out = pts
        .iter()
        .map(|&(x, y)| (scale * (x - cx), scale * (y - cy)))
        .collect();

    (t, out)
}

/// Estimate the homography `H` mapping `src` points to `dst` points so that
/// `dst ~ H * src` (in homogeneous coordinates), normalized to `H[(2,2)] = 1`.
///
/// Returns `None` if fewer than 4 correspondences are given, the counts differ,
/// or the linear system is degenerate.
pub fn find_homography(src: &[(f64, f64)], dst: &[(f64, f64)]) -> Option<Matrix3<f64>> {
    if src.len() != dst.len() || src.len() < 4 {
        return None;
    }

    let (t_src, src_n) = normalize(src);
    let (t_dst, dst_n) = normalize(dst);

    // Build the 2n x 9 system A h = 0.
    let n = src_n.len();
    let mut a = DMatrix::<f64>::zeros(2 * n, 9);
    for (i, (&(x, y), &(u, v))) in src_n.iter().zip(dst_n.iter()).enumerate() {
        let r0 = 2 * i;
        let r1 = r0 + 1;
        a[(r0, 0)] = -x;
        a[(r0, 1)] = -y;
        a[(r0, 2)] = -1.0;
        a[(r0, 6)] = u * x;
        a[(r0, 7)] = u * y;
        a[(r0, 8)] = u;

        a[(r1, 3)] = -x;
        a[(r1, 4)] = -y;
        a[(r1, 5)] = -1.0;
        a[(r1, 6)] = v * x;
        a[(r1, 7)] = v * y;
        a[(r1, 8)] = v;
    }

    // The solution is the right singular vector of the smallest singular value.
    let svd = a.svd(false, true);
    let vt = svd.v_t?;
    let h = vt.row(vt.nrows() - 1).transpose();

    let h_norm = Matrix3::new(h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7], h[8]);

    // Denormalize: H = T_dst^{-1} * H_norm * T_src.
    let t_dst_inv = t_dst.try_inverse()?;
    let mut hmat = t_dst_inv * h_norm * t_src;

    let scale = hmat[(2, 2)];
    if scale.abs() < f64::EPSILON {
        return None;
    }
    hmat /= scale;
    Some(hmat)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(h: &Matrix3<f64>, p: (f64, f64)) -> (f64, f64) {
        let v = h * nalgebra::Vector3::new(p.0, p.1, 1.0);
        (v[0] / v[2], v[1] / v[2])
    }

    #[test]
    fn recovers_known_homography() {
        // A non-trivial homography (rotation + perspective).
        let h_true = Matrix3::new(0.8, -0.2, 30.0, 0.15, 0.9, -10.0, 0.0005, -0.0003, 1.0);

        let src = [
            (0.0, 0.0),
            (1.0, 0.0),
            (2.0, 0.0),
            (3.0, 1.0),
            (0.0, 1.0),
            (1.0, 2.0),
            (2.0, 3.0),
            (4.0, 4.0),
        ];
        let dst: Vec<(f64, f64)> = src.iter().map(|&p| apply(&h_true, p)).collect();

        let h = find_homography(&src, &dst).expect("homography");

        // Compare by action on points (H is only defined up to scale).
        for &p in &src {
            let (ex, ey) = apply(&h_true, p);
            let (gx, gy) = apply(&h, p);
            approx::assert_abs_diff_eq!(gx, ex, epsilon = 1e-9);
            approx::assert_abs_diff_eq!(gy, ey, epsilon = 1e-9);
        }
    }

    #[test]
    fn rejects_too_few_points() {
        let pts = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0)];
        assert!(find_homography(&pts, &pts).is_none());
    }
}
