// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Per-view extrinsics (rotation + translation) from a homography and known
//! intrinsics.
//!
//! Decomposes `K^{-1} H = λ [r1 r2 t]` for a planar target: the first two
//! columns give the first two rotation columns (after scaling so they are
//! unit-norm), their cross product gives the third, and the result is projected
//! onto `SO(3)` by SVD. The sign is chosen so the target lies in front of the
//! camera (`t_z > 0`). This is an initializer for LM refinement.

use nalgebra::{Matrix3, Rotation3, Vector3};

/// Camera pose of the target plane relative to the camera.
#[derive(Clone, Copy, Debug)]
pub struct Extrinsics {
    pub rotation: Rotation3<f64>,
    pub translation: Vector3<f64>,
}

/// Recover the pose for one view from intrinsics `k` and homography `h`.
///
/// Returns `None` if `k` is singular or the homography is degenerate.
pub fn init_extrinsics(k: &Matrix3<f64>, h: &Matrix3<f64>) -> Option<Extrinsics> {
    let kinv = k.try_inverse()?;

    let kh1 = kinv * h.column(0);
    let kh2 = kinv * h.column(1);
    let kh3 = kinv * h.column(2);

    let n1 = kh1.norm();
    if n1 < f64::EPSILON {
        return None;
    }
    // Average the two column norms for a slightly more stable scale, matching
    // the spirit of OpenCV's decomposition.
    let lambda = 2.0 / (n1 + kh2.norm());

    let mut r1 = lambda * kh1;
    let mut r2 = lambda * kh2;
    let mut t = lambda * kh3;

    // Put the target in front of the camera.
    if t.z < 0.0 {
        r1 = -r1;
        r2 = -r2;
        t = -t;
    }
    let r3 = r1.cross(&r2);

    // Project [r1 r2 r3] onto SO(3).
    let mut q = Matrix3::zeros();
    q.set_column(0, &r1);
    q.set_column(1, &r2);
    q.set_column(2, &r3);

    let svd = q.svd(true, true);
    let u = svd.u?;
    let v_t = svd.v_t?;
    let mut rot = u * v_t;
    if rot.determinant() < 0.0 {
        // Flip the sign of the last U column to keep a right-handed rotation.
        let mut u2 = u;
        let last = u2.ncols() - 1;
        let col = -u2.column(last);
        u2.set_column(last, &col);
        rot = u2 * v_t;
    }

    let rotation = Rotation3::from_matrix_unchecked(rot);
    Some(Extrinsics {
        rotation,
        translation: t,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn homography_for_view(k: &Matrix3<f64>, r: &Rotation3<f64>, t: &Vector3<f64>) -> Matrix3<f64> {
        let rm = r.matrix();
        let mut m = Matrix3::zeros();
        m.set_column(0, &rm.column(0));
        m.set_column(1, &rm.column(1));
        m.set_column(2, t);
        k * m
    }

    #[test]
    fn recovers_pose() {
        let k = Matrix3::new(520.0, 0.0, 319.5, 0.0, 510.0, 239.5, 0.0, 0.0, 1.0);
        let r_true = Rotation3::from_euler_angles(0.12, -0.22, 0.07);
        let t_true = Vector3::new(-1.3, 0.6, 8.0);

        let h = homography_for_view(&k, &r_true, &t_true);
        let ext = init_extrinsics(&k, &h).expect("extrinsics");

        approx::assert_abs_diff_eq!(ext.translation, t_true, epsilon = 1e-9);
        approx::assert_abs_diff_eq!(ext.rotation.matrix(), r_true.matrix(), epsilon = 1e-9);
    }
}
