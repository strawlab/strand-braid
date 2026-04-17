//! RANSAC-based epipolar geometry validation.
//!
//! Port of the Octave files in `RansacM/` and
//! `MultiCamSelfCal/CoreFunctions/findinl.m`.

use nalgebra::{DMatrix, Matrix3, SVD};
use rand::Rng;
use rand::SeedableRng;

use crate::utils;

/// Find inliers in the joint image matrix by pairwise epipolar geometry.
///
/// Equivalent to `CoreFunctions/findinl.m`. For each camera (starting with
/// the one that sees the most points), the camera is paired with the one
/// sharing the most correspondences, and RANSAC is used to estimate the
/// fundamental matrix. Points consistent with the epipolar geometry are
/// marked as inliers.
pub(crate) fn find_inliers(ws: &DMatrix<f64>, id_mat: &DMatrix<bool>, tol: f64) -> DMatrix<bool> {
    let n_cams = id_mat.nrows();
    let n_frames = id_mat.ncols();

    // Track which cameras haven't been processed yet
    let mut not_used: Vec<i64> = (0..n_cams)
        .map(|i| {
            let count = (0..n_frames).filter(|&j| id_mat[(i, j)]).count();
            count as i64
        })
        .collect();

    let mut id_mat_in = DMatrix::<bool>::from_element(n_cams, n_frames, false);
    let mut id_mat_work = id_mat.clone();

    while not_used.iter().any(|&v| v >= 0) {
        // Find camera with most unused points
        let cam_max = not_used
            .iter()
            .enumerate()
            .max_by_key(|&(_, v)| *v)
            .unwrap()
            .0;

        // Mark as used
        not_used[cam_max] = -1;

        // Find camera to pair with: max correspondences with cam_max
        let mut corresp_counts = vec![0usize; n_cams];
        for i in 0..n_cams {
            if i == cam_max {
                continue;
            }
            for j in 0..n_frames {
                if id_mat_work[(cam_max, j)] && id_mat_work[(i, j)] {
                    corresp_counts[i] += 1;
                }
            }
        }

        let cam_to_pair = corresp_counts
            .iter()
            .enumerate()
            .max_by_key(|&(_, v)| *v)
            .unwrap()
            .0;

        // Find common correspondences
        let corr_indices: Vec<usize> = (0..n_frames)
            .filter(|&j| id_mat_work[(cam_max, j)] && id_mat_work[(cam_to_pair, j)])
            .collect();

        if corr_indices.len() < 8 {
            tracing::debug!(
                "Warning: Not enough points ({}) for epipolar geometry between cameras {} and {}",
                corr_indices.len(),
                cam_max,
                cam_to_pair
            );
            continue;
        }

        // Build 6xN matrix of point pairs
        let n_corr = corr_indices.len();
        let mut ws_pair = DMatrix::<f64>::zeros(6, n_corr);
        for (col, &j) in corr_indices.iter().enumerate() {
            for r in 0..3 {
                ws_pair[(r, col)] = ws[(cam_max * 3 + r, j)];
                ws_pair[(r + 3, col)] = ws[(cam_to_pair * 3 + r, j)];
            }
        }

        // RANSAC estimation of fundamental matrix
        let (_f, inls) = r_eg(&ws_pair, tol, 0.99, 8);

        // Mark inliers
        let n_inliers_this_pair = inls.iter().filter(|&&x| x).count();
        tracing::debug!(
            "  RANSAC: cameras {} and {}: {} inliers out of {} common correspondences",
            cam_max,
            cam_to_pair,
            n_inliers_this_pair,
            corr_indices.len()
        );

        for (local_idx, &is_inlier) in inls.iter().enumerate() {
            if is_inlier {
                let frame_idx = corr_indices[local_idx];
                id_mat_in[(cam_max, frame_idx)] = true;
            }
        }

        // Update working id_mat
        for j in 0..n_frames {
            id_mat_work[(cam_max, j)] = false;
        }
        for (local_idx, &is_inlier) in inls.iter().enumerate() {
            if is_inlier {
                let frame_idx = corr_indices[local_idx];
                id_mat_work[(cam_max, frame_idx)] = true;
            }
        }
    }

    id_mat_in
}

/// RANSAC robust estimation of epipolar geometry.
///
/// Equivalent to `RansacM/rEG.m`.
///
/// # Deviation from Octave
///
/// After the RANSAC loop finds the best inlier set using un-normalized DLT,
/// the F matrix is refined using normalized DLT on the inlier subset (same
/// as octave). However, the **inlier set is then re-evaluated** using the
/// refined F matrix. The Octave code returns the pre-refinement inlier set
/// unchanged, but because Rust and Octave use different PRNG sequences, the
/// pre-refinement inlier count can be lower in Rust. Re-evaluating with
/// the (much better) refined F recovers points that are genuine inliers but
/// were missed due to the un-normalized estimate's lower accuracy. In
/// practice this makes the Rust output match the Octave output (which
/// typically finds all true inliers in the RANSAC loop thanks to its own
/// random sequence).
fn r_eg(u: &DMatrix<f64>, th: f64, conf: f64, ss: usize) -> (Matrix3<f64>, Vec<bool>) {
    let max_sam_limit = 100_000;
    let len = u.ncols();
    // Use seeded RNG for reproducible results
    let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(42);
    let mut ptr: Vec<usize> = (0..len).collect();

    let mut max_inliers = 5;
    let mut max_sam = max_sam_limit;
    let mut no_sam = 0;

    let th_sq = 2.0 * th * th;

    let mut best_f = Matrix3::<f64>::zeros();
    let mut best_inls = vec![false; len];

    while no_sam < max_sam {
        // Fisher-Yates partial shuffle for sample selection
        // Note: idx must be > pos to ensure proper randomization (matches Octave's ceil(rand * (len-pos)) + 1)
        // Octave (1-based): idx = pos + ceil(rand * (len-pos)), giving idx in {pos+1, ..., len}
        // Rust (0-based): we want idx in {pos+1, ..., len-1}, so offset should be in {1, ..., len-pos-1}
        for pos in 0..ss {
            let idx = pos + rng.random_range(1..(len - pos));
            ptr.swap(pos, idx);
        }
        no_sam += 1;

        let sample: Vec<usize> = ptr[..ss].to_vec();
        let mut u_sample = DMatrix::<f64>::zeros(6, ss);
        for (col, &idx) in sample.iter().enumerate() {
            for r in 0..6 {
                u_sample[(r, col)] = u[(r, idx)];
            }
        }

        if let Some(sf) = u2f_dlt(&u_sample, false) {
            let errs = f_sampson(&sf, u);
            let v: Vec<bool> = errs.iter().map(|&e| e < th_sq).collect();
            let n_inliers: usize = v.iter().filter(|&&x| x).count();

            if n_inliers > max_inliers {
                best_inls = v;
                best_f = sf;
                max_inliers = n_inliers;
                let new_max = nsamples(max_inliers, len, ss, conf);
                max_sam = max_sam.min(new_max);
            }
        }
    }

    // Refine F using all inliers with normalization
    let n_inliers: usize = best_inls.iter().filter(|&&x| x).count();
    if n_inliers >= 8 {
        let mut u_inliers = DMatrix::<f64>::zeros(6, n_inliers);
        let mut col = 0;
        for (i, &is_inlier) in best_inls.iter().enumerate() {
            if is_inlier {
                for r in 0..6 {
                    u_inliers[(r, col)] = u[(r, i)];
                }
                col += 1;
            }
        }
        if let Some(f_refined) = u2f_dlt(&u_inliers, true) {
            // Re-evaluate inliers with the refined F matrix
            let errs_refined = f_sampson(&f_refined, u);
            let new_inls: Vec<bool> = errs_refined.iter().map(|&e| e < th_sq).collect();
            let new_n_inliers: usize = new_inls.iter().filter(|&&x| x).count();
            if new_n_inliers >= max_inliers {
                best_f = f_refined;
                best_inls = new_inls;
                max_inliers = new_n_inliers;
            }
        }
    }

    tracing::info!("RANSAC: {no_sam} samples, {max_inliers} inliers out of {len} points");

    (best_f, best_inls)
}

/// Linear estimation of the fundamental matrix via DLT.
///
/// Equivalent to `RansacM/u2Fdlt.m`.
///
/// # Deviation from Octave — denormalization formula
///
/// The Octave code denormalizes with `F = inv(T2) * F_norm * T1`. The
/// mathematically correct formula is `F = T2' * F_norm * T1`, derived as
/// follows:
///
/// The fundamental-matrix constraint is `u2' * F * u1 = 0`. After
/// normalization `u_n = T * u` we have `u2n' * Fn * u1n = 0`, i.e.
/// `(T2*u2)' * Fn * (T1*u1) = 0`, which expands to
/// `u2' * T2' * Fn * T1 * u1 = 0`. Therefore `F = T2' * Fn * T1`.
///
/// For the normalization matrices produced by `point_norm_iso` (see [`crate::utils`]),
/// `inv(T)` ≠ `T'`, so the Octave formula is not equivalent. In practice
/// the Octave formula produces catastrophically wrong F matrices in some
/// configurations (Sampson errors 10,000× larger), while `T2' * Fn * T1`
/// consistently yields sub-pixel Sampson errors.
///
/// # Deviation from Octave — norm used for normalization
///
/// Octave's `norm(F,2)` is the spectral norm (largest singular value).
/// This port uses the spectral norm as well, not the Frobenius norm that
/// nalgebra's `.norm()` computes.
fn u2f_dlt(u: &DMatrix<f64>, do_norm: bool) -> Option<Matrix3<f64>> {
    let n = u.ncols();
    if n < 8 {
        return None;
    }

    let u1_raw = u.rows(0, 3).clone_owned();
    let u2_raw = u.rows(3, 3).clone_owned();

    let (u1, u2, t1, t2) = if do_norm {
        let (u1n, t1n) = utils::point_norm_iso(&u1_raw);
        let (u2n, t2n) = utils::point_norm_iso(&u2_raw);
        (u1n, u2n, Some(t1n), Some(t2n))
    } else {
        (u1_raw, u2_raw, None, None)
    };

    // Build the design matrix A
    let mut a = DMatrix::<f64>::zeros(n, 9);
    for i in 0..n {
        for j in 0..3 {
            for k in 0..3 {
                a[(i, j * 3 + k)] = u2[(j, i)] * u1[(k, i)];
            }
        }
    }

    let svd = SVD::new(a, false, true);
    let v = svd.v_t?.transpose();
    let f_col = v.column(v.ncols() - 1);

    // Reshape into 3x3: octave does reshape(f,3,3)' which is column-major reshape then transpose
    // reshape(f,3,3) fills column-by-column: [f1 f4 f7; f2 f5 f8; f3 f6 f9]
    // then transpose: [f1 f2 f3; f4 f5 f6; f7 f8 f9]
    // So F(r,c) = f[r*3+c] in 0-based
    let mut f = Matrix3::<f64>::zeros();
    for r in 0..3 {
        for c in 0..3 {
            f[(r, c)] = f_col[r * 3 + c];
        }
    }

    // Enforce rank 2
    let svd_f = SVD::new(f, true, true);
    let mut s = svd_f.singular_values;
    s[2] = 0.0;
    f = svd_f.u? * nalgebra::Matrix3::from_diagonal(&s) * svd_f.v_t?;

    // Denormalize: F = T2' * F * T1
    // (u2' * F * u1 = 0) with u_n = T*u gives (T2*u2)' * Fn * (T1*u1) = 0
    // => u2' * T2' * Fn * T1 * u1 = 0, so F = T2' * Fn * T1
    if let (Some(t1), Some(t2)) = (t1, t2) {
        f = t2.transpose() * f * t1;
    }

    // Normalize F by spectral norm (2-norm), matching octave's norm(F,2)
    let svd_norm = SVD::new(f, false, false);
    let spectral_norm = svd_norm.singular_values[0];
    if spectral_norm > 0.0 {
        f /= spectral_norm;
    }

    // Final rank 2 enforcement (in case normalization introduced numerical issues)
    if f.determinant().abs() > 1e-12 {
        let svd_final = SVD::new(f, true, true);
        let mut s = svd_final.singular_values;
        s[2] = 0.0;
        f = svd_final.u? * nalgebra::Matrix3::from_diagonal(&s) * svd_final.v_t?;
    }

    Some(f)
}

/// Sampson distance (first-order geometrical error).
///
/// Equivalent to `RansacM/Fsampson.m`.
///
/// Computes `(u2'*F*u1)² / (‖F*u1‖²₁₂ + ‖F'*u2‖²₁₂)` where the
/// subscript `₁₂` means only the first two components are used in the
/// squared norm.
fn f_sampson(f: &Matrix3<f64>, u: &DMatrix<f64>) -> Vec<f64> {
    let n = u.ncols();
    let mut errs = vec![0.0; n];

    for i in 0..n {
        let u1 = nalgebra::Vector3::new(u[(0, i)], u[(1, i)], u[(2, i)]);
        let u2 = nalgebra::Vector3::new(u[(3, i)], u[(4, i)], u[(5, i)]);

        let fu1 = f * u1;
        let ftu2 = f.transpose() * u2; // Standard formula: F'*u2, not F'*u1
        let num = (u2.transpose() * f * u1)[(0, 0)];

        let denom = fu1[0].powi(2) + fu1[1].powi(2) + ftu2[0].powi(2) + ftu2[1].powi(2);
        if denom.abs() > 1e-15 {
            errs[i] = num.powi(2) / denom;
        } else {
            errs[i] = f64::INFINITY;
        }
    }

    errs
}

/// Calculate the number of RANSAC samples still needed.
///
/// Equivalent to `RansacM/nsamples.m`.
fn nsamples(n_inliers: usize, n_total: usize, ss: usize, conf: f64) -> usize {
    let outlier_ratio = 1.0 - n_inliers as f64 / n_total as f64;
    let n = (1.0 - conf).ln() / ((1.0 - (1.0 - outlier_ratio).powi(ss as i32) + f64::EPSILON).ln());
    n.ceil().max(1.0) as usize
}
