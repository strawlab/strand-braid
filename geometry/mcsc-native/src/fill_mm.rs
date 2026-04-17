//! Projective reconstruction from a measurement matrix.
//!
//! Port of the Martinec-Pajdla `fill_mm` algorithm
//! (`MartinecPajdla/fill_mm/fill_mm.m` and related files).
//!
//! # Overview
//!
//! Given a 3K×N measurement matrix M (K cameras, N points, homogeneous 2-D
//! coordinates with NaN for missing entries), this module recovers a
//! projective motion matrix P (3K×4) and shape matrix X (4×N) such that
//! `P*X ≈ M` (up to per-entry projective depths).
//!
//! The high-level flow inside [`fill_mm`] is:
//!
//! 1. **Normalization** — per-camera isotropic normalization ([`norm_m`],
//!    wrapping `normu`) centres and scales each camera’s observations.
//! 2. **Strategy selection** — the sequence strategy (F-matrices between
//!    consecutive camera pairs) is tried first; if it has fewer missing
//!    entries to recover, the central-image strategy is used instead.
//!    See the “Strategy selection” section below.
//! 3. **Epipolar geometry** — fundamental matrices and epipoles are
//!    estimated ([`m2fe`]).  For the sequence strategy, each camera k
//!    is paired with camera k−1; for central-image, each camera is
//!    paired with the central camera.
//! 4. **Depth estimation** — per-entry projective depths (λ) are computed
//!    from the epipolar geometry ([`depth_estimation`]).  For the
//!    sequence strategy, depths are chained from each point’s first
//!    visible camera (determined by [`subseq_longest`]).
//! 5. **Rescaled measurement matrix (PRMM)** — each entry of M is
//!    multiplied by its depth λ, then the matrix is balanced
//!    ([`balance_triplets`]) so rows and columns have comparable norms.
//! 6. **Null-space construction** — random 4-tuples of columns are sampled;
//!    for each, the null space of the corresponding sub-matrix is
//!    computed ([`create_nullspace`]).  The union of these null-space
//!    vectors constrains the rank-4 subspace that M must lie in.
//! 7. **Basis and depth recovery** — the rank-4 basis L is extracted from
//!    the null space ([`nullspace_to_basis`]), and any entries whose
//!    depths were not determined in step 4 are recovered by projecting
//!    into L ([`l2_depths`]).
//! 8. **Column immersion** — each column of M is projected into L to get
//!    the initial P and X ([`approx_matrix`]).
//! 9. **Factorization** — the depth-refined measurement matrix is
//!    re-factorized via SVD into the final P and X (see the detailed
//!    comment block at the factorization site below).
//!
//! # Deviation from Octave — null-space computation
//!
//! The Octave function `create_nullspace.m` calls `nulleps`, which in turn
//! calls Octave's built-in `svd(M)`. For a tall m×n matrix (m > n),
//! Octave's `svd` returns the **full** m×m U matrix, so columns beyond the
//! n-th are the null-space basis vectors.
//!
//! nalgebra's [`SVD`] only returns a thin decomposition (m×min(m,n) for U),
//! which omits the null space entirely for tall matrices. This port
//! therefore computes the null space via the eigendecomposition of `M*Mᵀ`:
//! eigenvectors with near-zero eigenvalues span the same space as the
//! missing columns of the full U. See [`compute_null_space`].
//!
//! # Strategy selection
//!
//! Like the octave `fill_mm.m` with `opt.strategy = -1`, this port
//! tries the **sequence** strategy first (fundamental matrices between
//! consecutive camera pairs k, k−1), falling back to **central-image**
//! strategies if the sequence has fewer missing entries to recover.
//! The sequence strategy typically wins and produces better-conditioned
//! depth estimates.

use eyre::Result;
use nalgebra::{DMatrix, DVector, Matrix3, SVD};
use rand::Rng;
use rand::SeedableRng;

use crate::utils;

/// Options for the fill_mm algorithm.
#[derive(Debug, Clone)]
pub struct FillMmOptions {
    pub verbose: bool,
    pub no_ba: bool,
    pub iter: usize,
    pub detection_accuracy: f64,
    pub consistent_number: usize,
    pub consistent_number_min: usize,
    pub samples: usize,
    pub create_nullspace_trial_coef: usize,
    pub create_nullspace_threshold: f64,
    pub tol: f64,
}

/// Result of fill_mm_bundle.
pub struct FillMmResult {
    /// Projective motion matrix (3*m x 4).
    pub p: DMatrix<f64>,
    /// Projective shape matrix (4 x n). Columns corresponding to
    /// `bad_cols` are all zeros (the point could not be reconstructed).
    pub x: DMatrix<f64>,
    /// Indices into the input measurement matrix M (0-based column
    /// indices) of points that could NOT be reconstructed.  Equivalent to
    /// Octave's `u2` return value of `fill_mm`.  Callers should drop
    /// these from their point index set before computing reprojection
    /// errors.
    pub bad_cols: Vec<usize>,
}

/// Projective reconstruction from measurement matrix, with optional BA.
///
/// Equivalent to `MartinecPajdla/fill_mm_test/fill_mm_bundle.m`. Bundle
/// adjustment is not implemented in this port (the Rust caller can perform
/// its own BA externally).
pub(crate) fn fill_mm_bundle(
    m: &DMatrix<f64>,
    imsize: &nalgebra::DMatrixView<f64>,
    opt: &FillMmOptions,
) -> Result<FillMmResult> {
    let mut result = fill_mm(m, opt)?;

    let n_cams = m.nrows() / 3;
    let n_pts = m.ncols();

    if !opt.no_ba {
        // Determine recovered cameras (non-zero P rows) and points (non-zero X cols)
        let r1: Vec<usize> = (0..n_cams)
            .filter(|&cam| {
                (0..4).any(|c| {
                    result.p[(cam * 3, c)] != 0.0
                        || result.p[(cam * 3 + 1, c)] != 0.0
                        || result.p[(cam * 3 + 2, c)] != 0.0
                })
            })
            .collect();
        let r2: Vec<usize> = (0..n_pts)
            .filter(|&pt| (0..4).any(|r| result.x[(r, pt)] != 0.0))
            .collect();

        if r1.len() >= 2 && r2.len() >= 4 {
            tracing::info!(
                "Bundle adjustment... ({} cameras, {} points)",
                r1.len(),
                r2.len()
            );

            // Extract sub-matrices for recovered cameras/points
            let p_sub = DMatrix::from_fn(r1.len() * 3, 4, |r, c| {
                let cam = r / 3;
                let row = r % 3;
                result.p[(r1[cam] * 3 + row, c)]
            });
            let x_sub = DMatrix::from_fn(4, r2.len(), |r, c| result.x[(r, r2[c])]);

            // Build sub measurement matrix: normalize_cut(M(k2i(r1), r2))
            // This is the 3*|r1| x |r2| sub-matrix of M
            let m_sub = DMatrix::from_fn(r1.len() * 3, r2.len(), |r, c| {
                let cam = r / 3;
                let row = r % 3;
                m[(r1[cam] * 3 + row, r2[c])]
            });

            let imsize_arr: Vec<[f64; 2]> = r1
                .iter()
                .map(|&i| [imsize[(i, 0)], imsize[(i, 1)]])
                .collect();

            let (p_ba, x_ba) =
                crate::projective_ba::bundle_px_proj(&p_sub, &x_sub, &m_sub, &imsize_arr);

            // Write back into full-size result
            for (sub_cam, &orig_cam) in r1.iter().enumerate() {
                for r in 0..3 {
                    for c in 0..4 {
                        result.p[(orig_cam * 3 + r, c)] = p_ba[(sub_cam * 3 + r, c)];
                    }
                }
            }
            for (sub_pt, &orig_pt) in r2.iter().enumerate() {
                for r in 0..4 {
                    result.x[(r, orig_pt)] = x_ba[(r, sub_pt)];
                }
            }
        }
    }

    Ok(result)
}

/// k2i: convert camera indices to row indices (0-based).
/// For camera k (0-based), returns [3*k, 3*k+1, 3*k+2].
/// Convert camera indices to row indices in a 3-rows-per-camera layout.
///
/// Equivalent to `MartinecPajdla/utils/k2i.m` with the default step of 3.
fn k2i(cameras: &[usize]) -> Vec<usize> {
    let mut result = Vec::with_capacity(cameras.len() * 3);
    for &k in cameras {
        result.push(k * 3);
        result.push(k * 3 + 1);
        result.push(k * 3 + 2);
    }
    result
}

/// Extract rows from a matrix.
fn extract_rows(m: &DMatrix<f64>, rows: &[usize]) -> DMatrix<f64> {
    let ncols = m.ncols();
    let nrows = rows.len();
    let mut result = DMatrix::zeros(nrows, ncols);
    for (new_row, &old_row) in rows.iter().enumerate() {
        for c in 0..ncols {
            result[(new_row, c)] = m[(old_row, c)];
        }
    }
    result
}

/// Extract columns from a matrix.
fn extract_cols(m: &DMatrix<f64>, cols: &[usize]) -> DMatrix<f64> {
    let nrows = m.nrows();
    let ncols = cols.len();
    let mut result = DMatrix::zeros(nrows, ncols);
    for (new_col, &old_col) in cols.iter().enumerate() {
        for r in 0..nrows {
            result[(r, new_col)] = m[(r, old_col)];
        }
    }
    result
}

/// Compute Octave `dist(M0, M, metric=1)` restricted to columns in `cols`.
///
/// That is: for every (camera, point) entry where both `m0` and `m` have a
/// non-NaN homogeneous-3 coordinate, divide each 3-vector by its third
/// coordinate (Octave `normalize_cut`), measure the Euclidean distance
/// between the resulting (x, y) pairs, and return the sum divided by the
/// number of observed entries.  Matches `MartinecPajdla/utils/dist.m`
/// with metric=1 and `MartinecPajdla/utils/eucl_dist.m`.
fn proj_repr_error_metric1(m0: &DMatrix<f64>, m: &DMatrix<f64>, cols: &[usize]) -> f64 {
    let n_cams = m0.nrows() / 3;
    let mut sum_d = 0.0;
    let mut count: usize = 0;
    for &j in cols {
        for i in 0..n_cams {
            let m0w = m0[(3 * i + 2, j)];
            let mw = m[(3 * i + 2, j)];
            if m0w.is_nan() || mw.is_nan() {
                continue;
            }
            // Skip entries where the homogeneous coord is essentially
            // zero (normalize.m would set it to 1, but that's an edge
            // case and doesn't occur for actual image points).
            if m0w.abs() < 1e-300 || mw.abs() < 1e-300 {
                continue;
            }
            let x0 = m0[(3 * i, j)] / m0w;
            let y0 = m0[(3 * i + 1, j)] / m0w;
            let x1 = m[(3 * i, j)] / mw;
            let y1 = m[(3 * i + 1, j)] / mw;
            let dx = x0 - x1;
            let dy = y0 - y1;
            sum_d += (dx * dx + dy * dy).sqrt();
            count += 1;
        }
    }
    if count == 0 {
        f64::NAN
    } else {
        sum_d / count as f64
    }
}

/// Get the visibility matrix I from measurement matrix M.
/// `I[i,j]` is true if camera i sees point j (i.e., `M[3*i, j]` is not NaN).
fn get_visibility(m: &DMatrix<f64>) -> DMatrix<bool> {
    let n_cams = m.nrows() / 3;
    let n_pts = m.ncols();
    DMatrix::from_fn(n_cams, n_pts, |i, j| !m[(3 * i, j)].is_nan())
}

/// Normalize measurement matrix using normu per camera.
/// Returns (normalized_M, T) where `T[k]` is the 3x3 normalization matrix for camera k.
/// Normalize the image coordinates by applying `normu` (see [`crate::utils`]) per camera.
///
/// Equivalent to the `normM` local function inside `fill_mm.m`.
fn norm_m(m: &DMatrix<f64>) -> (DMatrix<f64>, Vec<Matrix3<f64>>) {
    let n_cams = m.nrows() / 3;
    let mut mr = m.clone();
    let mut transforms = Vec::with_capacity(n_cams);

    for k in 0..n_cams {
        // Get non-NaN columns for this camera
        let non_nan_cols: Vec<usize> = (0..m.ncols())
            .filter(|&j| !m[(3 * k, j)].is_nan())
            .collect();

        if non_nan_cols.is_empty() {
            transforms.push(Matrix3::identity());
            continue;
        }

        let mut u_sub = DMatrix::<f64>::zeros(3, non_nan_cols.len());
        for (col, &j) in non_nan_cols.iter().enumerate() {
            for r in 0..3 {
                u_sub[(r, col)] = m[(k * 3 + r, j)];
            }
        }

        let tk = utils::normu(&u_sub).unwrap_or(Matrix3::identity());

        // Apply normalization to all columns
        for j in 0..m.ncols() {
            let v = nalgebra::Vector3::new(mr[(k * 3, j)], mr[(k * 3 + 1, j)], mr[(k * 3 + 2, j)]);
            let nv = tk * v;
            mr[(k * 3, j)] = nv[0];
            mr[(k * 3 + 1, j)] = nv[1];
            mr[(k * 3 + 2, j)] = nv[2];
        }

        transforms.push(tk);
    }

    (mr, transforms)
}

/// Undo normalization on projective motion P.
/// Undo per-camera normalization on a projective motion matrix.
///
/// Equivalent to the `normMback` local function inside `fill_mm.m`.
fn norm_m_back(p: &DMatrix<f64>, transforms: &[Matrix3<f64>]) -> DMatrix<f64> {
    let mut result = p.clone();
    let n_cams = p.nrows() / 3;

    for k in 0..n_cams {
        let tk_inv = transforms[k].try_inverse().unwrap_or(Matrix3::identity());
        for c in 0..p.ncols() {
            let v = nalgebra::Vector3::new(p[(k * 3, c)], p[(k * 3 + 1, c)], p[(k * 3 + 2, c)]);
            let nv = tk_inv * v;
            result[(k * 3, c)] = nv[0];
            result[(k * 3 + 1, c)] = nv[1];
            result[(k * 3 + 2, c)] = nv[2];
        }
    }

    result
}

/// Estimate fundamental matrix using orthogonal LS regression (u2FI.m).
/// u: 6 x N measurement matrix (two sets of homogeneous 2D points).
/// Returns F (3x3) or None if too few points.
/// Estimate the fundamental matrix using orthogonal LS regression.
///
/// Equivalent to `MartinecPajdla/fill_mm/u2FI.m`.
fn u2fi(u: &DMatrix<f64>) -> Option<(Matrix3<f64>, nalgebra::Vector3<f64>)> {
    let n = u.ncols();

    // Find columns where both views have data
    let sampcols: Vec<usize> = (0..n)
        .filter(|&j| {
            let has_view1 = !u[(0, j)].is_nan();
            let has_view2 = !u[(3, j)].is_nan();
            has_view1 && has_view2
        })
        .collect();

    if sampcols.len() < 8 {
        return None;
    }

    let pt_num = sampcols.len();

    // Normalize points
    let mut u1 = DMatrix::<f64>::zeros(3, pt_num);
    let mut u2 = DMatrix::<f64>::zeros(3, pt_num);
    for (col, &j) in sampcols.iter().enumerate() {
        for r in 0..3 {
            u1[(r, col)] = u[(r, j)];
            u2[(r, col)] = u[(r + 3, j)];
        }
    }

    let a1 = utils::normu(&u1)?;
    let a2 = utils::normu(&u2)?;

    let mut u1n = DMatrix::<f64>::zeros(3, pt_num);
    let mut u2n = DMatrix::<f64>::zeros(3, pt_num);
    for j in 0..pt_num {
        let v1 = nalgebra::Vector3::new(u1[(0, j)], u1[(1, j)], u1[(2, j)]);
        let v2 = nalgebra::Vector3::new(u2[(0, j)], u2[(1, j)], u2[(2, j)]);
        let nv1 = a1 * v1;
        let nv2 = a2 * v2;
        for r in 0..3 {
            u1n[(r, j)] = nv1[r];
            u2n[(r, j)] = nv2[r];
        }
    }

    // Build Z matrix
    let mut z = DMatrix::<f64>::zeros(pt_num, 9);
    for i in 0..pt_num {
        for j in 0..3 {
            for k in 0..3 {
                z[(i, j * 3 + k)] = u1n[(j, i)] * u2n[(k, i)];
            }
        }
    }

    let m_mat = z.transpose() * &z;

    // Sorted eigenvalue decomposition
    let eig = m_mat.symmetric_eigen();
    let eigenvalues = eig.eigenvalues;
    let eigenvectors = eig.eigenvectors;

    // Sort by eigenvalue (ascending)
    let mut indices: Vec<usize> = (0..eigenvalues.len()).collect();
    indices.sort_by(|&a, &b| eigenvalues[a].partial_cmp(&eigenvalues[b]).unwrap());

    // Smallest eigenvector
    let smallest_idx = indices[0];
    let f_vec = eigenvectors.column(smallest_idx);

    // Reshape into 3x3
    let mut f = Matrix3::<f64>::zeros();
    for r in 0..3 {
        for c in 0..3 {
            f[(r, c)] = f_vec[r * 3 + c];
        }
    }

    // Enforce rank 2
    let svd_f = SVD::new(f, true, true);
    let mut s = svd_f.singular_values;
    s[2] = 0.0;
    f = svd_f.u.unwrap() * Matrix3::from_diagonal(&s) * svd_f.v_t.unwrap();

    // Denormalize: F = A1' * F * A2
    f = a1.transpose() * f * a2;

    // Normalize
    let f_norm = f.norm();
    if f_norm > 0.0 {
        f /= f_norm;
    }

    // Final rank 2 enforcement
    let svd_final = SVD::new(f, true, true);
    let mut s = svd_final.singular_values;
    s[2] = 0.0;
    let u_mat = svd_final.u.unwrap();
    f = u_mat * Matrix3::from_diagonal(&s) * svd_final.v_t.unwrap();

    // Epipole: null space of F' (left null space of F)
    let svd_ep = SVD::new(f, true, false);
    let u_ep = svd_ep.u.unwrap();
    let epipole = nalgebra::Vector3::new(u_ep[(0, 2)], u_ep[(1, 2)], u_ep[(2, 2)]);

    Some((f, epipole))
}

/// Compute fundamental matrices and epipoles (M2Fe.m).
struct FundamentalMatrices {
    /// `F[i][j]` is the fundamental matrix from camera j to camera i (if computed).
    f: Vec<Vec<Option<Matrix3<f64>>>>,
    /// Epipoles `ep[i][j]`.
    ep: Vec<Vec<Option<nalgebra::Vector3<f64>>>>,
    /// Rows (camera indices) that were successfully used.
    rows: Vec<usize>,
    /// Rows that couldn't be used.
    #[allow(dead_code)]
    nonrows: Vec<usize>,
}

/// Estimate epipolar geometry of MM in sequence or using the central image.
///
/// Equivalent to `MartinecPajdla/fill_mm/M2Fe.m`.
fn m2fe(m: &DMatrix<f64>, central: Option<usize>) -> FundamentalMatrices {
    let n_cams = m.nrows() / 3;
    let mut f_mats = vec![vec![None; n_cams]; n_cams];
    let mut ep_mats = vec![vec![None; n_cams]; n_cams];
    let mut nonrows = Vec::new();

    let rows_to_try: Vec<usize> = if let Some(c) = central {
        (0..n_cams).filter(|&i| i != c).collect()
    } else {
        (1..n_cams).collect()
    };

    let mut good_rows = Vec::new();

    for &k in &rows_to_try {
        let j = if let Some(c) = central { c } else { k - 1 };

        // Build 6xN matrix for this pair
        let n_cols = m.ncols();
        let mut u_pair = DMatrix::<f64>::zeros(6, n_cols);
        for col in 0..n_cols {
            for r in 0..3 {
                u_pair[(r, col)] = m[(k * 3 + r, col)];
                u_pair[(r + 3, col)] = m[(j * 3 + r, col)];
            }
        }

        if let Some((g, epip)) = u2fi(&u_pair) {
            f_mats[k][j] = Some(g);
            ep_mats[k][j] = Some(epip);
            good_rows.push(k);
        } else {
            nonrows.push(k);
        }
    }

    // Add central or first camera to rows
    let mut rows = good_rows;
    if let Some(c) = central {
        if !rows.contains(&c) {
            rows.push(c);
        }
        rows.sort();
    } else {
        if !rows.contains(&0) {
            rows.insert(0, 0);
        }
    }

    FundamentalMatrices {
        f: f_mats,
        ep: ep_mats,
        rows,
        nonrows,
    }
}

/// Find the longest continuous subsequence of `true` values in each column.
///
/// Equivalent to `MartinecPajdla/fill_mm/subseq_longest.m`.
///
/// Returns `(b, len)` where `b[p]` is the starting row of the longest
/// run and `len[p]` is the length of that run, for each column `p`.
fn subseq_longest(vis: &DMatrix<bool>) -> (Vec<usize>, Vec<usize>) {
    let n_rows = vis.nrows();
    let n_cols = vis.ncols();
    let mut b = vec![0usize; n_cols];
    let mut len = vec![0usize; n_cols];

    for p in 0..n_cols {
        let mut seq = vec![0usize; n_rows];
        let mut l = 0;
        for i in 0..n_rows {
            if vis[(i, p)] {
                seq[l] += 1;
            } else {
                l = i + 1;
            }
        }
        let max_len = *seq.iter().max().unwrap_or(&0);
        len[p] = max_len;
        if max_len > 0 {
            b[p] = seq.iter().position(|&s| s == max_len).unwrap();
        }
    }

    (b, len)
}

/// Determine scale factors (projective depths) of the PRMM.
///
/// Equivalent to `MartinecPajdla/fill_mm/depth_estimation.m`.
///
/// For the **central-image** strategy (`central = Some(c)`), all depths
/// are computed relative to camera `c`, which sees all points.
///
/// For the **sequence** strategy (`central = None`), depths are chained:
/// each point's depth chain starts at the camera identified by
/// [`subseq_longest`] (the beginning of the longest run of consecutive
/// visible cameras for that point) and propagates forward through
/// consecutive pairs (k, k−1).
fn depth_estimation(
    m: &DMatrix<f64>,
    fund: &FundamentalMatrices,
    _rows: &[usize],
    central: Option<usize>,
) -> (DMatrix<f64>, DMatrix<bool>) {
    let n_cams = m.nrows() / 3;
    let n_pts = m.ncols();

    let mut lambda = DMatrix::<f64>::from_element(n_cams, n_pts, 1.0);
    let mut i_lamb = DMatrix::<bool>::from_element(n_cams, n_pts, false);

    // For central-image: reference camera is the central one.
    // For sequence: reference is camera 0, but each point's depth chain
    // starts at the beginning of its longest visible subsequence.
    let (j_cam, seq_b) = if let Some(c) = central {
        // Central-image strategy
        (c, vec![])
    } else {
        // Sequence strategy: find longest subsequence start per point
        let vis = DMatrix::from_fn(n_cams, n_pts, |i, j| !m[(3 * i, j)].is_nan());
        let (b, _len) = subseq_longest(&vis);
        (0, b)
    };

    // Initialize depths
    if central.is_some() {
        for p in 0..n_pts {
            i_lamb[(j_cam, p)] = !m[(3 * j_cam, p)].is_nan();
        }
    } else {
        // Sequence: initialize at each point's subsequence start
        for p in 0..n_pts {
            let bp = seq_b[p];
            if bp < n_cams && !m[(3 * bp, p)].is_nan() {
                i_lamb[(bp, p)] = true;
            }
        }
    }

    // Iterate over ALL cameras (not just those with valid F-matrices).
    // Octave's depth_estimation loops over setdiff(1:m, j) — all cameras.
    // The Ilamb chain propagation is independent of whether an F-matrix
    // exists; we only need F to compute the actual lambda scale.
    // If no F-matrix is available, we still propagate i_lamb and fall
    // back to lambda=1.0 (same as Octave's `else lambda(i,p) = 1`
    // branch), so the chain is never broken.
    let all_cameras: Vec<usize> = (0..n_cams).collect();
    let cameras_to_visit: Vec<usize> = if let Some(c) = central {
        all_cameras.into_iter().filter(|&i| i != c).collect()
    } else {
        all_cameras.into_iter().filter(|&i| i != 0).collect()
    };

    for i in cameras_to_visit {
        let j = if central.is_some() { j_cam } else { i - 1 };

        let f_ep = fund.f[i][j].as_ref().zip(fund.ep[i][j].as_ref());

        let ps: Vec<usize> = if central.is_some() {
            (0..n_pts).collect()
        } else {
            // Sequence: use points whose subsequence started at or before j
            (0..n_pts).filter(|&p| seq_b[p] <= j).collect()
        };

        for &p in &ps {
            i_lamb[(i, p)] = i_lamb[(j, p)] && !m[(3 * i, p)].is_nan();

            if i_lamb[(i, p)] {
                if let Some((g, epip)) = f_ep {
                    let m_ip =
                        nalgebra::Vector3::new(m[(i * 3, p)], m[(i * 3 + 1, p)], m[(i * 3 + 2, p)]);
                    let m_jp =
                        nalgebra::Vector3::new(m[(j * 3, p)], m[(j * 3 + 1, p)], m[(j * 3 + 2, p)]);
                    let u = epip.cross(&m_ip);
                    let v = g * m_jp;
                    let u_norm_sq = u.norm_squared();
                    if u_norm_sq.abs() > 1e-15 {
                        lambda[(i, p)] = u.dot(&v) / u_norm_sq * lambda[(j, p)];
                    }
                }
                // else: no F-matrix for this pair; lambda stays 1.0
            } else {
                lambda[(i, p)] = 1.0;
            }
        }
    }

    (lambda, i_lamb)
}

/// Spread depths column (spread_depths_col.m).
/// Takes a 3m x 1 column and depths indicator, returns a submatrix.
/// Spread a JIM column with known/unknown depths into a sub-matrix.
///
/// Equivalent to `MartinecPajdla/fill_mm/spread_depths_col.m`.
fn spread_depths_col(m_col: &DVector<f64>, depths_i: &[bool]) -> DMatrix<f64> {
    let n_cams = depths_i.len();
    let known_depths: Vec<usize> = (0..n_cams).filter(|&i| depths_i[i]).collect();
    let unknown_depths: Vec<usize> = (0..n_cams).filter(|&i| !depths_i[i]).collect();

    // Matches Octave `spread_depths_col.m`: exactly one column collecting all
    // known-depth entries (if any), plus one column per unknown-depth row.
    // Previously Rust always allocated `1 + |unknown|` columns, which leaves
    // a trailing all-zero column when there are no known depths — that extra
    // zero column adds a spurious null direction and causes the caller's
    // `nrows == ncols + n_null` check to reject tetrads that Octave accepts.
    let known_part = if known_depths.is_empty() { 0 } else { 1 };
    let n_cols = known_part + unknown_depths.len();
    if n_cols == 0 {
        return DMatrix::zeros(0, 0);
    }

    let n_rows = n_cams * 3;
    let mut submatrix = DMatrix::<f64>::zeros(n_rows, n_cols);

    // First column: known depth entries
    let mut col = 0;
    if !known_depths.is_empty() {
        for &k in &known_depths {
            for r in 0..3 {
                submatrix[(k * 3 + r, col)] = m_col[k * 3 + r];
            }
        }
        col += 1;
    }

    // One column per unknown depth entry
    for &k in &unknown_depths {
        for r in 0..3 {
            submatrix[(k * 3 + r, col)] = m_col[k * 3 + r];
        }
        col += 1;
    }

    submatrix
}

/// Balance triplets (balance_triplets.m).
/// Balance the PRMM by column-wise and triplet-of-rows-wise rescaling.
///
/// Equivalent to `MartinecPajdla/fill_mm/balance_triplets.m`.
fn balance_triplets(m_orig: &DMatrix<f64>) -> DMatrix<f64> {
    let n_cams = m_orig.nrows() / 3;
    let n_pts = m_orig.ncols();
    let mut b = m_orig.clone();

    let mut iteration = 0;
    loop {
        let b_old = b.clone();

        // Step 1: rescale each column
        let mut max_diff_cols: f64 = 0.0;
        for l in 0..n_pts {
            let rows: Vec<usize> = (0..n_cams)
                .filter(|&k| !m_orig[(3 * k, l)].is_nan())
                .collect();
            if !rows.is_empty() {
                let mut s = 0.0;
                for &k in &rows {
                    for r in 0..3 {
                        s += b[(k * 3 + r, l)].powi(2);
                    }
                }
                let supposed_weight = rows.len() as f64;
                max_diff_cols = max_diff_cols.max((s - supposed_weight).abs());
                let scale = (s / supposed_weight).sqrt();
                if scale > 1e-15 {
                    for &k in &rows {
                        for r in 0..3 {
                            b[(k * 3 + r, l)] /= scale;
                        }
                    }
                }
            }
        }

        // Step 2: rescale each triplet of rows
        let mut max_diff_rows: f64 = 0.0;
        for k in 0..n_cams {
            let cols: Vec<usize> = (0..n_pts)
                .filter(|&j| !m_orig[(3 * k, j)].is_nan())
                .collect();
            if !cols.is_empty() {
                let mut s = 0.0;
                for &j in &cols {
                    for r in 0..3 {
                        s += b[(k * 3 + r, j)].powi(2);
                    }
                }
                let supposed_weight = cols.len() as f64;
                max_diff_rows = max_diff_rows.max((s - supposed_weight).abs());
                let scale = (s / supposed_weight).sqrt();
                if scale > 1e-15 {
                    for &j in &cols {
                        for r in 0..3 {
                            b[(k * 3 + r, j)] /= scale;
                        }
                    }
                }
            }
        }

        // Check convergence
        let mut change = 0.0;
        for k in 0..n_cams {
            let cols: Vec<usize> = (0..n_pts)
                .filter(|&j| !m_orig[(3 * k, j)].is_nan())
                .collect();
            for &j in &cols {
                for r in 0..3 {
                    change += (b[(k * 3 + r, j)] - b_old[(k * 3 + r, j)]).powi(2);
                }
            }
        }

        iteration += 1;
        if (change <= 0.01 && max_diff_rows <= 1.0 && max_diff_cols <= 1.0) || iteration > 20 {
            break;
        }
    }

    b
}

/// Create the null space used for projective reconstruction.
///
/// Equivalent to `MartinecPajdla/fill_mm/create_nullspace.m`.
fn create_nullspace(
    m: &DMatrix<f64>,
    depths: &DMatrix<bool>,
    central: Option<usize>,
    trial_coef: usize,
    threshold: f64,
) -> (DMatrix<f64>, usize, usize) {
    let i_vis = get_visibility(m);
    let n_cams = i_vis.nrows();
    let n_pts = i_vis.ncols();
    let n_rows = m.nrows(); // 3 * n_cams

    let cols_scaled: Vec<bool> = if let Some(c) = central {
        (0..n_pts).map(|j| i_vis[(c, j)]).collect()
    } else {
        vec![]
    };

    let num_trials = trial_coef * n_pts;
    let mut nullspace_cols: Vec<DVector<f64>> = Vec::new();
    // Use seeded RNG for reproducible results
    // Use seeded RNG for reproducible results
    let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(12345);

    let mut used = 0usize;
    let mut failed = 0usize;
    let t_ns_start = std::time::Instant::now();

    for trial in 0..num_trials {
        if trial > 0 && trial % 5000 == 0 {
            tracing::debug!(
                "  [create_nullspace] trial {}/{}, used={}, failed={}, elapsed={:.2?}",
                trial,
                num_trials,
                used,
                failed,
                t_ns_start.elapsed()
            );
        }
        // Choose 4 random columns
        let mut available_cols: Vec<usize> = (0..n_pts).collect();
        let mut available_rows: Vec<usize> = (0..n_cams).collect();
        let mut cols_chosen = Vec::with_capacity(4);
        let mut trial_failed = false;
        let mut scaled_ensured = central.is_none();

        for t in 0..4 {
            if available_cols.is_empty() || available_rows.is_empty() {
                trial_failed = true;
                break;
            }

            let idx = rng.random_range(0..available_cols.len());
            let c = available_cols.remove(idx);
            cols_chosen.push(c);

            // Keep only rows that see this column
            available_rows.retain(|&r| i_vis[(r, c)]);

            if t < 3 {
                // Cut useless columns and rows (matches Octave `cut_useless`).
                let demanded = 4 - t - 1;

                // Compute demanded_rows used for the column filter.
                // Octave: if ~scaled_ensured, demanded_rows = (rows==2 ? 2 : 3);
                //         else,               demanded_rows = 2.
                let mut demanded_rows = 2;

                if !scaled_ensured && central.is_some() {
                    let demanded_scaled = if available_rows.len() == 2 { 3 } else { 2 };
                    if available_rows.len() != 2 {
                        demanded_rows = 3;
                    }
                    let cols_scaled_chosen: usize = cols_chosen
                        .iter()
                        .filter(|&&c| cols_scaled.get(c).copied().unwrap_or(false))
                        .count();
                    if cols_scaled_chosen <= demanded_scaled
                        && demanded == demanded_scaled - cols_scaled_chosen
                    {
                        available_cols.retain(|&c| cols_scaled.get(c).copied().unwrap_or(false));
                        scaled_ensured = true;
                        // Octave falls through to the `else demanded_rows = 2`
                        // path on the next iteration, but for the column
                        // filter in *this* call, demanded_rows is still the
                        // value computed above before `scaled_ensured=1`.
                    }
                }

                // Keep only columns visible from at least `demanded_rows` of
                // the remaining rows.
                available_cols.retain(|&c| {
                    let count = available_rows.iter().filter(|&&r| i_vis[(r, c)]).count();
                    count >= demanded_rows
                });

                // Keep only rows that see at least `demanded` of the
                // remaining columns.
                available_rows.retain(|&r| {
                    let count = available_cols.iter().filter(|&&c| i_vis[(r, c)]).count();
                    count >= demanded
                });
            }

            if available_rows.is_empty() {
                trial_failed = true;
                break;
            }
        }

        if trial_failed {
            failed += 1;
            continue;
        }

        // Build submatrix using spread_depths_col
        let _row_indices = k2i(&available_rows);
        let mut sub_cols: Vec<DMatrix<f64>> = Vec::new();
        for &c in &cols_chosen {
            let mut col_vec = DVector::<f64>::zeros(available_rows.len() * 3);
            for (new_r, &old_r) in available_rows.iter().enumerate() {
                for r in 0..3 {
                    col_vec[new_r * 3 + r] = m[(old_r * 3 + r, c)];
                }
            }
            let d: Vec<bool> = available_rows.iter().map(|&r| depths[(r, c)]).collect();
            let spread = spread_depths_col(&col_vec, &d);
            sub_cols.push(spread);
        }

        // Concatenate columns
        let total_cols: usize = sub_cols.iter().map(|s| s.ncols()).sum();
        let sub_rows = available_rows.len() * 3;
        if sub_rows == 0 || total_cols == 0 {
            failed += 1;
            continue;
        }

        let mut submatrix = DMatrix::<f64>::zeros(sub_rows, total_cols);
        let mut col_offset = 0;
        for sc in &sub_cols {
            for c in 0..sc.ncols() {
                for r in 0..sub_rows.min(sc.nrows()) {
                    submatrix[(r, col_offset + c)] = sc[(r, c)];
                }
            }
            col_offset += sc.ncols();
        }

        // Compute null space of submatrix using nulleps approach:
        // [u,s,v] = svd(M); sigsvs = sum(diag(s)>tol); N = u(:,sigsvs+1:end)
        // nalgebra's SVD is thin, so for m > n we need the full U.
        // Approach: compute eigendecomposition of M*M' to get full U.
        let (null_vecs, _n_sig) = compute_null_space(&submatrix, threshold);

        if !null_vecs.is_empty() {
            let n_null = null_vecs.len();
            // Check dimension condition
            if submatrix.nrows() == submatrix.ncols() + n_null {
                for nv in &null_vecs {
                    let mut full_vec = DVector::<f64>::zeros(n_rows);
                    for (new_r, &old_r) in available_rows.iter().enumerate() {
                        for r in 0..3 {
                            full_vec[old_r * 3 + r] = nv[new_r * 3 + r];
                        }
                    }
                    nullspace_cols.push(full_vec);
                }
                used += 1;
            }
        }
    }

    if nullspace_cols.is_empty() {
        return (DMatrix::zeros(n_rows, 0), used, failed);
    }

    // Assemble nullspace matrix
    let width = nullspace_cols.len();
    let mut nullspace = DMatrix::<f64>::zeros(n_rows, width);
    for (c, col) in nullspace_cols.iter().enumerate() {
        for r in 0..n_rows {
            nullspace[(r, c)] = col[r];
        }
    }

    tracing::debug!(
        "Nullspace: tried {num_trials}, used {used}, failed {failed}, size {}x{}",
        nullspace.nrows(),
        nullspace.ncols()
    );

    (nullspace, used, failed)
}

/// Compute the null space of a matrix M (m × n, m ≥ n).
///
/// Returns `(null_vectors, n_significant_svs)`. Equivalent to the inner
/// `nulleps` helper inside `create_nullspace.m`.
///
/// # Deviation from Octave
///
/// For tall matrices (m > n) Octave's `[u,s,v] = svd(M)` returns a full
/// m×m U, and the null-space vectors are taken from columns `sigsvs+1` to
/// `m`. nalgebra's SVD is always thin (U is m×min(m,n)), so those columns
/// are missing. Instead, we form `M*Mᵀ` (m×m, symmetric positive
/// semi-definite) and compute its eigendecomposition. Eigenvectors whose
/// eigenvalues have `√λ ≤ threshold` span the same null space.
fn compute_null_space(m: &DMatrix<f64>, tol: f64) -> (Vec<DVector<f64>>, usize) {
    let nrows = m.nrows();
    let ncols = m.ncols();

    if nrows <= ncols {
        // For square or wide matrices, use thin SVD
        let svd = SVD::new(m.clone(), true, false);
        let u = match svd.u {
            Some(u) => u,
            None => return (vec![], 0),
        };
        let sigsvs = svd.singular_values.iter().filter(|&&s| s > tol).count();
        let mut null_vecs = Vec::new();
        for c in sigsvs..u.ncols() {
            null_vecs.push(u.column(c).clone_owned());
        }
        (null_vecs, sigsvs)
    } else {
        // For tall matrices (m > n), compute the full left singular vectors
        // using the eigendecomposition of M * M^T
        let mmt = m * m.transpose();
        let eig = mmt.symmetric_eigen();

        // Sort eigenvalues in descending order
        let mut indices: Vec<usize> = (0..eig.eigenvalues.len()).collect();
        indices.sort_by(|&a, &b| {
            eig.eigenvalues[b]
                .partial_cmp(&eig.eigenvalues[a])
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Count significant singular values (sqrt of eigenvalues > tol)
        let sigsvs = indices
            .iter()
            .filter(|&&i| eig.eigenvalues[i].max(0.0).sqrt() > tol)
            .count();

        // Null space vectors are the eigenvectors corresponding to near-zero eigenvalues
        let mut null_vecs = Vec::new();
        for &idx in &indices[sigsvs..] {
            null_vecs.push(eig.eigenvectors.column(idx).clone_owned());
        }

        (null_vecs, sigsvs)
    }
}

/// Compute basis L from null space and extract depths.
fn nullspace_to_basis(
    nullspace: &DMatrix<f64>,
    r: usize,
    _threshold: f64,
) -> Option<(DMatrix<f64>, DVector<f64>)> {
    if nullspace.ncols() == 0 {
        return None;
    }

    let svd = if nullspace.ncols() < 10 * nullspace.nrows() {
        SVD::new(nullspace.clone(), true, false)
    } else {
        // For large matrices, use eigendecomposition approach
        let ata = nullspace * nullspace.transpose();
        let eig = ata.symmetric_eigen();
        // Sort eigenvalues ascending
        let mut indices: Vec<usize> = (0..eig.eigenvalues.len()).collect();
        indices.sort_by(|&a, &b| eig.eigenvalues[a].partial_cmp(&eig.eigenvalues[b]).unwrap());

        let len = indices.len();
        let mut u = DMatrix::<f64>::zeros(len, len);
        let mut s_diag = DVector::<f64>::zeros(len);
        for (new_i, &old_i) in indices.iter().rev().enumerate() {
            s_diag[new_i] = eig.eigenvalues[old_i].max(0.0).sqrt();
            for r in 0..len {
                u[(r, new_i)] = eig.eigenvectors[(r, old_i)];
            }
        }

        return Some((u.columns(len - r, r).clone_owned(), s_diag));
    };

    let u_mat = svd.u?;
    let len = u_mat.ncols();
    if len < r {
        return None;
    }

    // L = last r columns of U
    let l = u_mat.columns(len - r, r).clone_owned();
    Some((l, svd.singular_values))
}

/// Check if SVD has sufficient data.
fn svd_suff_data(singular_values: &DVector<f64>, r: usize, threshold: f64) -> bool {
    let n = singular_values.len();
    if n == 0 || n <= r {
        return false;
    }
    singular_values[n - r - 1] > threshold
}

/// Compute depths from basis L (L2depths.m).
/// Compute depths of the PRMM from basis L.
///
/// Equivalent to `MartinecPajdla/fill_mm/L2depths.m`.
fn l2_depths(
    l: &DMatrix<f64>,
    m: &DMatrix<f64>,
    i_depths: &DMatrix<bool>,
) -> (DMatrix<f64>, DMatrix<f64>) {
    let n_cams = m.nrows() / 3;
    let n_pts = m.ncols();

    let mut m_depths = m.clone();
    let mut lambda = DMatrix::<f64>::zeros(n_cams, n_pts);

    for j in 0..n_pts {
        let full: Vec<usize> = (0..n_cams).filter(|&i| !m[(3 * i, j)].is_nan()).collect();
        let mis_rows: Vec<usize> = full
            .iter()
            .filter(|&&i| !i_depths[(i, j)])
            .copied()
            .collect();

        if !mis_rows.is_empty() {
            // Build spread column for full rows
            let mut col_vec = DVector::<f64>::zeros(full.len() * 3);
            for (new_r, &old_r) in full.iter().enumerate() {
                for r in 0..3 {
                    col_vec[new_r * 3 + r] = m[(old_r * 3 + r, j)];
                }
            }
            let d: Vec<bool> = full.iter().map(|&i| i_depths[(i, j)]).collect();
            let submatrix = spread_depths_col(&col_vec, &d);

            let full_row_indices = k2i(&full);
            let l_sub = extract_rows(l, &full_row_indices);

            let right = submatrix.column(0).clone_owned();

            // A = [L_sub, -submatrix(:,2:end)]
            let n_extra_cols = submatrix.ncols() - 1;
            let n_a_cols = l_sub.ncols() + n_extra_cols;
            let n_a_rows = l_sub.nrows();
            let mut a_mat = DMatrix::<f64>::zeros(n_a_rows, n_a_cols);

            for r in 0..n_a_rows {
                for c in 0..l_sub.ncols() {
                    a_mat[(r, c)] = l_sub[(r, c)];
                }
                for c in 0..n_extra_cols {
                    a_mat[(r, l_sub.ncols() + c)] = -submatrix[(r, c + 1)];
                }
            }

            // Check rank
            let svd_check = SVD::new(a_mat.clone(), false, false);
            let rank = svd_check
                .singular_values
                .iter()
                .filter(|&&s| s > 1e-10)
                .count();

            if rank < a_mat.ncols() {
                // Can't compute depths - kill the data
                for &k in full.iter().filter(|&&k| !i_depths[(k, j)]) {
                    for r in 0..3 {
                        m_depths[(k * 3 + r, j)] = f64::NAN;
                    }
                    lambda[(k, j)] = f64::NAN;
                }
            } else {
                // Solve A * res = right using least squares
                let svd_a = SVD::new(a_mat, true, true);
                let res = svd_a.solve(&right, 1e-15).unwrap();

                // Assign depths.
                //
                // Match Octave `L2depths.m` exactly: identify which row of
                // the original MM each submatrix column corresponds to by
                // looking at the non-zero pattern, rather than assuming the
                // first column is the known-depths sum and the rest are
                // unknowns in `full` order. This matters because, when
                // there are no known depths, `spread_depths_col` returns a
                // matrix whose first column is an unknown-row column (not a
                // known-sum column), which would make the old index-based
                // loop access `res[4 + |unknown|]` (out of bounds) and
                // double-assign the first unknown.
                //
                // Column 0 of the submatrix corresponds to `right` and in
                // Octave receives `lambda = 1` for all rows whose 3rd entry
                // is non-zero there. Columns 1..ncols-1 each have a single
                // non-zero unknown row and receive `lambda = res(4 + ii)`.
                let w_rows_in_full = |col_idx: usize| -> Vec<usize> {
                    (0..full.len())
                        .filter(|&r| submatrix[(r * 3 + 2, col_idx)] != 0.0)
                        .map(|r| full[r])
                        .collect()
                };
                for i in w_rows_in_full(0) {
                    lambda[(i, j)] = 1.0;
                    for r in 0..3 {
                        m_depths[(i * 3 + r, j)] = m[(i * 3 + r, j)];
                    }
                }
                for ii in 1..submatrix.ncols() {
                    let depth = res[4 + ii - 1]; // res(5:end) in 1-based
                    for i in w_rows_in_full(ii) {
                        lambda[(i, j)] = depth;
                        for r in 0..3 {
                            m_depths[(i * 3 + r, j)] = m[(i * 3 + r, j)] * depth;
                        }
                    }
                }
            }
        }
    }

    (m_depths, lambda)
}

/// Immerse columns of MM into the basis (approx_matrix in approximate.m).
/// Immerse columns of MM into the basis (rank-r approximation).
///
/// Equivalent to the `approx_matrix` local function in
/// `MartinecPajdla/fill_mm/approximate.m`.
fn approx_matrix(
    m: &DMatrix<f64>,
    p: &DMatrix<f64>,
    r: usize,
    tol: f64,
) -> (DMatrix<f64>, Vec<usize>, DMatrix<f64>) {
    let n_rows = m.nrows();
    let n_pts = m.ncols();

    let mut m_app = DMatrix::<f64>::from_element(n_rows, n_pts, f64::NAN);
    let mut x = DMatrix::<f64>::from_element(r, n_pts, f64::NAN);
    let mut miss_cols = Vec::new();

    if p.ncols() == r {
        for j in 0..n_pts {
            let rows: Vec<usize> = (0..n_rows).filter(|&rr| !m[(rr, j)].is_nan()).collect();

            // Guard: nalgebra's SVD panics on empty matrices. If there are fewer
            // visible rows than the required rank `r`, the rank check would fail
            // anyway, so treat the column as missing.
            if rows.len() < r {
                miss_cols.push(j);
                continue;
            }

            let p_sub = extract_rows(p, &rows);

            // Check rank
            let svd_check = SVD::new(p_sub.clone(), false, false);
            let rank = svd_check
                .singular_values
                .iter()
                .filter(|&&s| s > tol)
                .count();

            if rank == r {
                // Solve P_sub * x_j = m_sub
                let m_col: DVector<f64> = DVector::from_fn(rows.len(), |i, _| m[(rows[i], j)]);
                let svd_solve = SVD::new(p_sub, true, true);
                let x_j = svd_solve.solve(&m_col, 1e-15).unwrap();

                // x(:,j) = x_j
                for rr in 0..r {
                    x[(rr, j)] = x_j[rr];
                }

                // m_app(:,j) = P * x_j
                let p_times_x = p * &x_j;
                for rr in 0..n_rows {
                    m_app[(rr, j)] = p_times_x[rr];
                }
            } else {
                miss_cols.push(j);
            }
        }
    }

    (m_app, miss_cols, x)
}

/// Extend matrix for unrecovered rows (extend_matrix in approximate.m).
#[allow(dead_code)]
fn extend_matrix(
    m: &DMatrix<f64>,
    sub_m: &DMatrix<f64>,
    x: &DMatrix<f64>,
    rows: &[usize],
    nonrows: &[usize],
    r: usize,
    tol: f64,
) -> (DMatrix<f64>, Vec<usize>, DMatrix<f64>) {
    let total_rows = m.nrows();
    let n_cols = m.ncols();

    let mut e = DMatrix::<f64>::from_element(total_rows, n_cols, f64::NAN);
    // Copy sub_m into rows
    for (ri, &row) in rows.iter().enumerate() {
        for c in 0..n_cols {
            e[(row, c)] = sub_m[(ri, c)];
        }
    }

    let mut unrecovered = Vec::new();
    let mut p_nonrows = Vec::new();

    for &i in nonrows {
        let cols: Vec<usize> = (0..n_cols).filter(|&c| !m[(i, c)].is_nan()).collect();
        let x_sub = extract_cols(x, &cols);

        let svd_check = SVD::new(x_sub.clone(), false, false);
        let rank = svd_check
            .singular_values
            .iter()
            .filter(|&&s| s > 1e-10)
            .count();

        if rank == r {
            // p_row = m(i,cols) / X(:,cols)
            let m_row: DVector<f64> = DVector::from_fn(cols.len(), |j, _| m[(i, cols[j])]);

            // Solve X_sub' * p_row = m_row (since m_row = p_row * X_sub, so X_sub' * p_row' = m_row')
            let svd_solve = SVD::new(x_sub.transpose(), true, true);
            let p_row = svd_solve.solve(&m_row, 1e-15).unwrap();

            // Check for large values
            let max_val = (p_row.transpose() * x).abs().max();
            if max_val > 1.0 / tol {
                unrecovered.push(i);
            } else {
                // E(i,:) = p_row * X
                let e_row = p_row.transpose() * x;
                for c in 0..n_cols {
                    e[(i, c)] = e_row[(0, c)];
                }
                p_nonrows.push(p_row);
            }
        } else {
            unrecovered.push(i);
        }
    }

    let p_nr = if !p_nonrows.is_empty() {
        let nr = p_nonrows.len();
        let nc = p_nonrows[0].len();
        DMatrix::from_fn(nr, nc, |i, j| p_nonrows[i][j])
    } else {
        DMatrix::zeros(0, r)
    };

    (e, unrecovered, p_nr)
}

/// Main fill_mm function: Projective reconstruction from measurement matrix.
/// Equivalent to `fill_mm.m`.
/// Main projective reconstruction from a measurement matrix.
///
/// Equivalent to `MartinecPajdla/fill_mm/fill_mm.m`. See the
/// [module-level documentation](self) for the full algorithm overview.
fn fill_mm(m_input: &DMatrix<f64>, opt: &FillMmOptions) -> Result<FillMmResult> {
    use std::time::Instant;
    let t0 = Instant::now();
    let n_cams = m_input.nrows() / 3;
    let n_pts_orig = m_input.ncols();
    tracing::debug!(
        "[fill_mm] entry: {} cams, {} pts ({:.2?})",
        n_cams,
        n_pts_orig,
        t0.elapsed()
    );

    // Remove points visible in fewer than 2 cameras
    let pt_beg: Vec<usize> = (0..n_pts_orig)
        .filter(|&j| {
            let count = (0..n_cams)
                .filter(|&i| !m_input[(3 * i, j)].is_nan())
                .count();
            count >= 2
        })
        .collect();

    if pt_beg.len() < n_pts_orig {
        tracing::debug!(
            "Removed correspondences in < 2 images ({})",
            n_pts_orig - pt_beg.len()
        );
    }

    let m = extract_cols(m_input, &pt_beg);
    let n_pts = m.ncols();

    // Strategy -1: try sequence first, then all central images.
    //
    // The sequence strategy uses all cameras and all points, computing
    // fundamental matrices between consecutive pairs. It tends to give
    // better-conditioned depth estimates when cameras form a sequence.
    let i_vis = get_visibility(&m);

    // Compute predictions for both strategies (matching octave's
    // compute_predictions)
    let seq_f = i_vis.iter().filter(|&&v| !v).count(); // missing entries

    let mut best_cent_idx = None;
    let mut best_cent_f = 0usize;
    let mut best_cent_s = 0usize;
    let mut cent_strengths: Vec<Option<(Vec<usize>, Vec<usize>)>> = vec![None; n_cams];

    for c in 0..n_cams {
        // Compute central-image strength (matching octave's strength())
        let mut good_rows = Vec::new();
        let mut i_local = i_vis.clone();
        for i in 0..n_cams {
            if i == c {
                continue;
            }
            let common: Vec<usize> = (0..n_pts)
                .filter(|&j| i_local[(i, j)] && i_local[(c, j)])
                .collect();
            if common.len() >= 8 {
                good_rows.push(i);
                // general=1: zero out non-common columns (NOT done for general)
                // The octave code does NOT zero out for general=1; it keeps all.
            } else {
                // Zero out this camera's row
                for j in 0..n_pts {
                    i_local[(i, j)] = false;
                }
            }
        }
        if !good_rows.contains(&c) {
            good_rows.push(c);
        }
        good_rows.sort();
        let good_cols: Vec<usize> = (0..n_pts)
            .filter(|&j| good_rows.iter().filter(|&&r| i_vis[(r, j)]).count() >= 2)
            .collect();

        let n_visible: usize = good_rows
            .iter()
            .map(|&r| good_cols.iter().filter(|&&j| i_vis[(r, j)]).count())
            .sum();
        let n_missing = good_rows.len() * good_cols.len() - n_visible;

        // Compute num_scaled (points visible in central and at least one other)
        let central_row_in_good = good_rows.iter().position(|&r| r == c);
        let num_scaled = if central_row_in_good.is_some() {
            let scaled_cols: Vec<usize> = good_cols
                .iter()
                .enumerate()
                .filter(|&(_, &j)| i_vis[(c, j)])
                .map(|(idx, _)| idx)
                .collect();
            let mut count = 0;
            for &r in &good_rows {
                for &idx in &scaled_cols {
                    if i_vis[(r, good_cols[idx])] {
                        count += 1;
                    }
                }
            }
            count
        } else {
            0
        };

        if n_missing > best_cent_f || (n_missing == best_cent_f && num_scaled > best_cent_s) {
            best_cent_f = n_missing;
            best_cent_s = num_scaled;
            best_cent_idx = Some(c);
        }
        cent_strengths[c] = Some((good_rows, good_cols));
    }

    // Compute S for sequence: sum of longest consecutive run lengths per point.
    // Octave uses this as the tiebreaker when F values are equal.
    let (_, seq_len) = subseq_longest(&i_vis);
    let seq_s: usize = seq_len.iter().sum();

    tracing::debug!("[fill_mm] seq: F={seq_f} S={seq_s}");
    tracing::debug!("[fill_mm] best_cent: F={best_cent_f} S={best_cent_s} cam={best_cent_idx:?}");

    // Choose best strategy: sequence wins if it has more missing entries
    // to fill (F) or equal F but more scaled points (S) — matching Octave's tiebreaker.
    let (rows, cols, central) =
        if seq_f > best_cent_f || (seq_f == best_cent_f && seq_s >= best_cent_s) {
            // Sequence strategy
            let rows: Vec<usize> = (0..n_cams).collect();
            let cols: Vec<usize> = (0..n_pts).collect();
            tracing::debug!(
                "Using sequence strategy ({} images, {} points)",
                rows.len(),
                cols.len()
            );
            (rows, cols, None)
        } else if let Some(c) = best_cent_idx {
            let (good_rows, good_cols) = cent_strengths[c].take().unwrap();
            tracing::debug!(
                "Using central image: {c} ({} images, {} points)",
                good_rows.len(),
                good_cols.len()
            );
            (good_rows, good_cols, Some(c))
        } else {
            ((0..n_cams).collect(), (0..n_pts).collect(), None)
        };

    tracing::debug!("Using {} images, {} points", rows.len(), cols.len());

    // Normalize measurement matrix
    tracing::debug!("[fill_mm] starting norm_m ({:.2?} elapsed)", t0.elapsed());
    let (mn, transforms) = norm_m(&m);
    tracing::debug!("[fill_mm] norm_m done ({:.2?} elapsed)", t0.elapsed());

    // Extract sub-matrices for rows and cols
    let row_indices = k2i(&rows);
    let mn_rows = extract_rows(&mn, &row_indices);
    let _mn_rows_cols = extract_cols(&mn_rows, &cols);

    // Compute fundamental matrices and epipoles
    tracing::debug!("[fill_mm] starting m2fe ({:.2?} elapsed)", t0.elapsed());
    let central_in_rows = central.map(|c| rows.iter().position(|&r| r == c).unwrap());
    let fund = m2fe(&mn_rows, central_in_rows);
    tracing::debug!(
        "[fill_mm] m2fe done, {} fund rows ({:.2?} elapsed)",
        fund.rows.len(),
        t0.elapsed()
    );

    if fund.rows.len() < 2 {
        eyre::bail!("Not enough cameras for reconstruction");
    }

    // Estimate depths
    tracing::debug!(
        "[fill_mm] starting depth_estimation ({:.2?} elapsed)",
        t0.elapsed()
    );
    let _mn_fund_rows = extract_rows(&mn_rows, &k2i(&fund.rows));
    let (lambda, i_lamb) = depth_estimation(&mn_rows, &fund, &fund.rows, central_in_rows);
    tracing::debug!(
        "[fill_mm] depth_estimation done ({:.2?} elapsed)",
        t0.elapsed()
    );

    // Build rescaled measurement matrix B
    let n_fund_cams = fund.rows.len();
    let mut b = DMatrix::<f64>::zeros(n_fund_cams * 3, n_pts);
    for (ri, &cam) in fund.rows.iter().enumerate() {
        for j in 0..n_pts {
            for r in 0..3 {
                b[(ri * 3 + r, j)] = mn_rows[(cam * 3 + r, j)] * lambda[(cam, j)];
            }
        }
    }

    // Brief depth-chain diagnostic
    let i_lamb_true = i_lamb.iter().filter(|&&v| v).count();
    let i_lamb_total = i_lamb.nrows() * i_lamb.ncols();
    tracing::debug!(
        "[fill_mm] i_lamb known={}/{} ({:.1}%)",
        i_lamb_true,
        i_lamb_total,
        100.0 * i_lamb_true as f64 / i_lamb_total as f64
    );

    // Balance triplets
    tracing::debug!(
        "[fill_mm] starting balance_triplets on {}x{} matrix ({:.2?} elapsed)",
        b.nrows(),
        b.ncols(),
        t0.elapsed()
    );
    let b_balanced = balance_triplets(&b);
    tracing::debug!(
        "[fill_mm] balance_triplets done ({:.2?} elapsed)",
        t0.elapsed()
    );

    // Create null space
    tracing::debug!(
        "[fill_mm] starting create_nullspace (trial_coef={}, n_pts={}, => {} trials) ({:.2?} elapsed)",
        opt.create_nullspace_trial_coef,
        n_pts,
        opt.create_nullspace_trial_coef * n_pts,
        t0.elapsed()
    );
    let i_lamb_fund = DMatrix::from_fn(n_fund_cams, n_pts, |i, j| i_lamb[(fund.rows[i], j)]);
    let (nullspace, _used, _failed) = create_nullspace(
        &b_balanced,
        &i_lamb_fund,
        central_in_rows.map(|c| fund.rows.iter().position(|&r| r == c).unwrap()),
        opt.create_nullspace_trial_coef,
        opt.create_nullspace_threshold,
    );

    tracing::debug!(
        "[fill_mm] create_nullspace done, nullspace {}x{} ({:.2?} elapsed)",
        nullspace.nrows(),
        nullspace.ncols(),
        t0.elapsed()
    );
    if nullspace.ncols() == 0 {
        eyre::bail!("Empty nullspace - cannot reconstruct");
    }

    // Compute basis L
    tracing::debug!(
        "[fill_mm] starting nullspace_to_basis ({:.2?} elapsed)",
        t0.elapsed()
    );
    let r = 4;
    let (l_basis, singular_values) =
        nullspace_to_basis(&nullspace, r, opt.create_nullspace_threshold)
            .ok_or_else(|| eyre::eyre!("Failed to compute basis from nullspace"))?;

    tracing::debug!(
        "[fill_mm] nullspace_to_basis done, l_basis {}x{} ({:.2?} elapsed)",
        l_basis.nrows(),
        l_basis.ncols(),
        t0.elapsed()
    );
    let suff = svd_suff_data(&singular_values, r, opt.create_nullspace_threshold);
    tracing::debug!(
        "[fill_mm] svd_suff_data: n={} r={} suff={} sv[n-r-1]={:.6e} (threshold {:.3e})",
        singular_values.len(),
        r,
        suff,
        if singular_values.len() > r {
            singular_values[singular_values.len() - r - 1]
        } else {
            f64::NAN
        },
        opt.create_nullspace_threshold
    );
    if !suff {
        eyre::bail!(
            "Insufficient data in nullspace SVD (sv[{}]={:.3e} < threshold {:.3e}) — \
            nullspace rank-deficient; Octave would try another strategy here",
            singular_values.len().saturating_sub(r + 1),
            if singular_values.len() > r {
                singular_values[singular_values.len() - r - 1]
            } else {
                0.0
            },
            opt.create_nullspace_threshold
        );
    }

    // Compute depths from basis
    tracing::debug!(
        "[fill_mm] starting l2_depths ({:.2?} elapsed)",
        t0.elapsed()
    );
    let (m_depths, _lambda_depths) = l2_depths(&l_basis, &b_balanced, &i_lamb_fund);
    tracing::debug!("[fill_mm] l2_depths done ({:.2?} elapsed)", t0.elapsed());

    // Approximate: immerse columns into basis
    tracing::debug!(
        "[fill_mm] starting approx_matrix ({:.2?} elapsed)",
        t0.elapsed()
    );
    let (m_app, miss_cols, _x_local) = approx_matrix(&m_depths, &l_basis, r, opt.tol);
    tracing::debug!(
        "[fill_mm] approx_matrix done, {} miss_cols ({:.2?} elapsed)",
        miss_cols.len(),
        t0.elapsed()
    );

    // Handle unrecovered rows
    let u1_rows: Vec<usize> = {
        let mean_abs: Vec<f64> = (0..l_basis.nrows())
            .map(|r| {
                let row_vals: Vec<f64> = (0..l_basis.ncols())
                    .map(|c| l_basis[(r, c)].abs())
                    .collect();
                row_vals.iter().sum::<f64>() / row_vals.len() as f64
            })
            .collect();
        // Group by camera (triplets)
        let n_rows_cams = l_basis.nrows() / 3;
        let mut bad_cams = Vec::new();
        for k in 0..n_rows_cams {
            let avg = (mean_abs[k * 3] + mean_abs[k * 3 + 1] + mean_abs[k * 3 + 2]) / 3.0;
            if avg <= opt.tol {
                bad_cams.push(k);
            }
        }
        bad_cams
    };

    let good_row_cams: Vec<usize> = (0..n_fund_cams).filter(|k| !u1_rows.contains(k)).collect();

    let good_cols: Vec<usize> = (0..n_pts).filter(|j| !miss_cols.contains(j)).collect();

    if good_row_cams.len() < 2 || good_cols.len() < r {
        eyre::bail!("Not enough recovered cameras or points for factorization");
    }

    // ----------------------------------------------------------------
    // Factorization into structure and motion
    // ----------------------------------------------------------------
    //
    // Matches octave `fill_mm.m` lines 194-207.
    //
    // At this point we have an initial projective reconstruction
    // R = P_approx * X_approx (from the null-space fitting in steps 6-8
    // above).  The factorization refines this by going back to the
    // *original* un-normalized measurement data M0 and recomputing
    // per-entry projective depths, which gives a better-conditioned
    // matrix for the final SVD.
    //
    // Concretely:
    //
    // (a) **Un-normalize Mdepths.**  During step 7, `l2_depths` computed
    //     depth-scaled measurements `Mdepths` in the *normalized*
    //     coordinate frame (after `normM`).  We undo that normalization
    //     with `normMback(Mdepths, T)` to get `Mdepths_un` in the
    //     original (centred, but un-scaled) pixel frame.
    //
    // (b) **Recompute per-entry depths from M0.**  For every (camera,
    //     point) entry that is observed, we solve the scalar
    //     least-squares problem
    //
    //         lambda_{i,j} = argmin_s || s * M0_{i,j} - Mdepths_un_{i,j} ||^2
    //
    //     where `M0_{i,j}` is the 3-vector from the *original*
    //     un-normalized input and `Mdepths_un_{i,j}` is the
    //     corresponding depth-estimated 3-vector.  This is more accurate
    //     than the depths from step 4 because Mdepths incorporates the
    //     null-space constraint.
    //
    // (c) **Build B = M_filled * lambda.**  `M_filled` is the original
    //     M0 with any NaN entries patched from the approximation R.
    //     For entries where `lambda` is NaN (depth could not be
    //     computed), B is filled directly from R.
    //
    // (d) **Normalize, balance, SVD.**  B is normalized per-camera
    //     (`normM`), balanced (`balance_triplets`), and SVD-factored.
    //     The rank-4 truncation gives the final P and X.  One
    //     `normMback` undoes the B normalization on P.
    //
    // Note: B is built from un-normalized data, so only one round of
    // normalization/un-normalization (via `normM`/`normMback` of B) is
    // needed.  There is no second un-normalization through the original
    // M-normalization transforms.
    tracing::debug!(
        "[fill_mm] starting factorization ({:.2?} elapsed)",
        t0.elapsed()
    );

    let good_row_indices = k2i(&good_row_cams);
    let n_good_cams = good_row_cams.len();

    // Step (a): un-normalize Mdepths.
    let good_transforms: Vec<Matrix3<f64>> = good_row_cams
        .iter()
        .map(|&k| transforms[fund.rows[k]])
        .collect();
    let m_depths_good = extract_rows(&m_depths, &good_row_indices);
    let m_depths_un = norm_m_back(&m_depths_good, &good_transforms);

    // M0: the ORIGINAL un-normalized input M (with NaN for missing).
    // In octave this is `M0 = M` saved before `normM(M)`.  In Rust,
    // `m` is the input to fill_mm before normalization.
    let m0_rows = k2i(&good_row_cams
        .iter()
        .map(|&k| fund.rows[k])
        .collect::<Vec<_>>());
    let m0_good = extract_rows(&m, &m0_rows);

    // R (the approximate reconstruction, un-normalized).  `m_app` was
    // computed in the normalized domain; undo that.
    let r_good = extract_rows(&m_app, &good_row_indices);
    let r_un = norm_m_back(&r_good, &good_transforms);

    // M_filled: M0 with NaN entries patched from R.
    let mut m_filled = m0_good.clone();
    for i in 0..m_filled.nrows() {
        for j in 0..m_filled.ncols() {
            if m_filled[(i, j)].is_nan() {
                m_filled[(i, j)] = r_un[(i, j)];
            }
        }
    }

    // Step (b): recompute per-entry depths from M0.
    //
    // Octave: `lambda(i) = M0(k2i(i)) \ Mdepths_un(k2i(i))`
    //
    // For each observed (camera, point) entry, find the scalar lambda
    // that best maps the original 3-vector to the depth-estimated one:
    //   lambda = (M0' * Mdepths_un) / (M0' * M0)
    let mut lambda_fact = DMatrix::<f64>::from_element(n_good_cams, n_pts, f64::NAN);
    for i in 0..n_good_cams {
        for j in 0..n_pts {
            if !m0_good[(i * 3 + 2, j)].is_nan() {
                let mut num = 0.0;
                let mut den = 0.0;
                for r in 0..3 {
                    let m0v = m0_good[(i * 3 + r, j)];
                    let mdv = m_depths_un[(i * 3 + r, j)];
                    num += m0v * mdv;
                    den += m0v * m0v;
                }
                if den.abs() > 1e-15 {
                    lambda_fact[(i, j)] = num / den;
                }
            }
        }
    }

    // --- Diagnostic: proj-space reprojection error (no factorization). ---
    // Matches Octave's `info.err.no_fact = dist(M0(k2i(r1),r2), R, metric=1)`.
    // Computed over (good_cams × good_cols) with observed entries only.
    {
        let err_no_fact = proj_repr_error_metric1(&m0_good, &r_un, &good_cols);
        tracing::info!(
            "[fill_mm] Repr. error in proj. space (no fact.): {:.6}",
            err_no_fact
        );
    }

    // Step (c): build B = M_filled * lambda.
    let mut b_matrix = DMatrix::<f64>::zeros(n_good_cams * 3, n_pts);
    for i in 0..n_good_cams {
        for j in 0..n_pts {
            let lam = lambda_fact[(i, j)];
            if !lam.is_nan() {
                for r in 0..3 {
                    b_matrix[(i * 3 + r, j)] = m_filled[(i * 3 + r, j)] * lam;
                }
            } else {
                // Missing depth: fill directly from the reconstruction R.
                for r in 0..3 {
                    b_matrix[(i * 3 + r, j)] = r_un[(i * 3 + r, j)];
                }
            }
        }
    }

    // Normalize, balance, SVD.
    //
    // Octave factorizes B restricted to recovered columns (see
    // `r2 = cols(setdiff(1:length(cols),u2))` and subsequent use of
    // `M(k2i(r1),r2)`).  `good_cols` is the Rust equivalent of `r2`:
    // columns where `approx_matrix` successfully immersed the point into
    // the rank-r basis.  miss_cols (seen in <r cameras) have NaN entries
    // in `b_matrix` and cannot be SVDed directly.  The caller is told
    // about these via `bad_cols` in FillMmResult so they can be dropped
    // from the inlier set used for downstream reprojection error.
    let b_cols = extract_cols(&b_matrix, &good_cols);
    let (bn, t_fact) = norm_m(&b_cols);
    let bn = balance_triplets(&bn);

    let svd_fact = SVD::new(bn, true, true);
    let u_fact = svd_fact
        .u
        .ok_or_else(|| eyre::eyre!("SVD failed in factorization"))?;
    let s_fact = svd_fact.singular_values;
    let vt_fact = svd_fact
        .v_t
        .ok_or_else(|| eyre::eyre!("SVD failed in factorization"))?;

    // P = U(:,1:4) * sqrt(S(1:4))
    // X = sqrt(S(1:4)) * V(:,1:4)'
    let mut s_sqrt = nalgebra::Matrix4::<f64>::zeros();
    for i in 0..4.min(s_fact.len()) {
        s_sqrt[(i, i)] = s_fact[i].sqrt();
    }

    let p_local = u_fact.columns(0, 4) * s_sqrt;
    let x_vt = vt_fact.rows(0, 4);
    let x_factored = s_sqrt * DMatrix::from_fn(4, x_vt.ncols(), |r, c| x_vt[(r, c)]);

    // Un-normalize P
    let p_fact = DMatrix::from_fn(p_local.nrows(), p_local.ncols(), |r, c| p_local[(r, c)]);
    let p_unnorm = norm_m_back(&p_fact, &t_fact);

    // --- Diagnostic: proj-space reprojection error (after factorization). ---
    // Matches Octave's `info.err.fact = dist(M0(k2i(r1),r2), P*X, metric=1)`.
    {
        // Build P*X restricted to good_cams × good_cols.
        let px = &p_unnorm * &x_factored;
        // px currently has columns indexed 0..good_cols.len(); splat into
        // a full n_pts-wide matrix so we can reuse the same helper.
        let mut px_full = DMatrix::<f64>::zeros(px.nrows(), n_pts);
        for (xi, &col) in good_cols.iter().enumerate() {
            for r in 0..px.nrows() {
                px_full[(r, col)] = px[(r, xi)];
            }
        }
        let err_fact = proj_repr_error_metric1(&m0_good, &px_full, &good_cols);
        tracing::info!(
            "[fill_mm] Repr. error in proj. space (fact.):    {:.6}",
            err_fact
        );
    }

    // Map back to original camera indices
    // The P matrix corresponds to good_row_cams within fund.rows
    // We need to map back to the full set of cameras
    let final_cam_indices: Vec<usize> = good_row_cams.iter().map(|&k| fund.rows[k]).collect();

    // Build full P matrix for all rows
    let mut p_full = DMatrix::<f64>::zeros(rows.len() * 3, 4);
    for (pi, &cam_in_rows) in final_cam_indices.iter().enumerate() {
        for r in 0..3 {
            for c in 0..4 {
                p_full[(cam_in_rows * 3 + r, c)] = p_unnorm[(pi * 3 + r, c)];
            }
        }
    }

    // Step (d) note: P is now in the centred (pp-subtracted) coordinate
    // frame, which is the frame of the input to fill_mm.
    // Only the B-matrix normalization (`t_fact`) needed undoing, which
    // `norm_m_back` already handled.  No second un-normalization through
    // the original M-normalization transforms is needed, because B was
    // built from un-normalized (but still centred) M0 * lambda.
    let p_final = p_full;

    // Build X for the good columns, mapped back into n_pts width so that
    // column j in the returned X corresponds to column j in the input M.
    // miss_cols remain zero-filled.  We return the miss_cols list so the
    // caller can drop those points from its inlier set (mirroring
    // Octave's `X = X(:, union(cols, setdiff(noncols, u2)))`).
    let mut x_final = DMatrix::<f64>::zeros(4, n_pts);
    for (xi, &col) in good_cols.iter().enumerate() {
        for r in 0..4 {
            x_final[(r, col)] = x_factored[(r, xi)];
        }
    }
    let bad_cols_local: Vec<usize> = miss_cols.clone();

    // Map back to original point indices
    let mut p_out = DMatrix::<f64>::zeros(n_cams * 3, 4);
    for (ri, &cam) in rows.iter().enumerate() {
        for r in 0..3 {
            for c in 0..4 {
                p_out[(cam * 3 + r, c)] = p_final[(ri * 3 + r, c)];
            }
        }
    }

    // Extend for unrecovered cameras if needed
    // (This is the extend_matrix step from approximate.m for nonrows)

    // Map X back through pt_beg so that the output X has one column per
    // column of the original input measurement matrix.  Columns dropped
    // by ptbeg (points seen in <2 cameras) and miss_cols columns remain
    // zero; their original indices appear in `bad_cols_out` below.
    let mut x_out = DMatrix::<f64>::zeros(4, n_pts_orig);
    for (j, &orig_j) in pt_beg.iter().enumerate() {
        if j < n_pts {
            for r in 0..4 {
                x_out[(r, orig_j)] = x_final[(r, j)];
            }
        }
    }

    // Map bad_cols (local indices into post-ptbeg M) back to original M
    // column indices.  Also include points dropped by ptbeg itself
    // (points seen in fewer than 2 cameras).
    let pt_beg_set: std::collections::HashSet<usize> = pt_beg.iter().copied().collect();
    let mut bad_cols_out: Vec<usize> = bad_cols_local
        .iter()
        .filter_map(|&j| pt_beg.get(j).copied())
        .collect();
    for j in 0..n_pts_orig {
        if !pt_beg_set.contains(&j) {
            bad_cols_out.push(j);
        }
    }
    bad_cols_out.sort_unstable();
    bad_cols_out.dedup();

    tracing::debug!(
        "[fill_mm] done ({:.2?} elapsed, {} bad_cols)",
        t0.elapsed(),
        bad_cols_out.len()
    );
    Ok(FillMmResult {
        p: p_out,
        x: x_out,
        bad_cols: bad_cols_out,
    })
}
