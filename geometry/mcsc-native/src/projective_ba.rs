//! Projective bundle adjustment for the MCSC pipeline.
//!
//! Equivalent to `MartinecPajdla/fill_mm_test/bundle_PX_proj.m` and
//! `eval_y_and_dy.m`. Refines projective motion (P) and shape (X) matrices
//! by minimizing reprojection error in preconditioned image coordinates,
//! using a tangent-space parameterization of P and X.
//!
//! This operates on the raw 3K×4 / 4×N projective matrices, **before**
//! euclidization. It does not decompose cameras into intrinsics/extrinsics.

use nalgebra::{DMatrix, DVector, Matrix3};

/// Per-visible-entry Jacobian blocks.
///
/// For visible entry `l = (cam, pt)`, residuals `2l, 2l+1` depend only on
/// camera `cam`'s 11 parameters and point `pt`'s 3 parameters.
/// `jp` is the 2×11 block dq/diP, `jx` is the 2×3 block dq/diX.
#[derive(Clone)]
struct VisBlocks {
    /// Row-major 2×11.
    jp: [f64; 22],
    /// Row-major 2×3.
    jx: [f64; 6],
}

/// Run projective bundle adjustment on `(P, X)`.
///
/// * `p0` — 3K × 4 joint camera matrix
/// * `x0` — 4 × N scene points
/// * `m`  — 3K × N measurement matrix (homogeneous, NaN for missing)
/// * `imsize` — K × 2 principal points `[pp_x, pp_y]` (i.e. half-resolution),
///   matching octave's `config.cal.pp(:,1:2)'`.
///
/// Returns refined `(P, X)`.
pub(crate) fn bundle_px_proj(
    p0: &DMatrix<f64>,
    x0: &DMatrix<f64>,
    m: &DMatrix<f64>,
    imsize: &[[f64; 2]],
) -> (DMatrix<f64>, DMatrix<f64>) {
    let k = p0.nrows() / 3; // number of cameras
    let n = x0.ncols(); // number of points

    // Build q: 2K × N non-homogeneous observation matrix (normalize_cut of M)
    let mut q = DMatrix::<f64>::from_element(2 * k, n, f64::NAN);
    for cam in 0..k {
        for pt in 0..n {
            let w = m[(cam * 3 + 2, pt)];
            if !w.is_nan() && w.abs() > f64::EPSILON {
                q[(cam * 2, pt)] = m[(cam * 3, pt)] / w;
                q[(cam * 2 + 1, pt)] = m[(cam * 3 + 1, pt)] / w;
            }
        }
    }

    // Precondition: apply image conditioners H_k
    // H_k = vgg_conditioner_from_image(imsize)
    let mut h_mats: Vec<Matrix3<f64>> = Vec::with_capacity(k);
    let mut h_inv_mats: Vec<Matrix3<f64>> = Vec::with_capacity(k);
    let mut p_cond = p0.clone();
    for cam in 0..k {
        let (hk, hk_inv) = vgg_conditioner_from_image(imsize[cam]);
        // P0(k2i(k),:) = H{k} * P0(k2i(k),:)
        let p_block = p_cond.rows(cam * 3, 3).clone_owned();
        let conditioned = hk * &p_block;
        for r in 0..3 {
            for c in 0..4 {
                p_cond[(cam * 3 + r, c)] = conditioned[(r, c)];
            }
        }
        // q(k2i(k,2),:) = nhom(H{k} * hom(q(k2i(k,2),:)))
        for pt in 0..n {
            if !q[(cam * 2, pt)].is_nan() {
                let u = q[(cam * 2, pt)];
                let v = q[(cam * 2 + 1, pt)];
                // hom: [u; v; 1], then H*hom, then nhom
                let hu = hk[(0, 0)] * u + hk[(0, 1)] * v + hk[(0, 2)];
                let hv = hk[(1, 0)] * u + hk[(1, 1)] * v + hk[(1, 2)];
                let hw = hk[(2, 0)] * u + hk[(2, 1)] * v + hk[(2, 2)];
                q[(cam * 2, pt)] = hu / hw;
                q[(cam * 2 + 1, pt)] = hv / hw;
            }
        }
        h_mats.push(hk);
        h_inv_mats.push(hk_inv);
    }

    // Normalize P (each camera block has unit Frobenius norm)
    for cam in 0..k {
        let mut frob_sq = 0.0;
        for r in 0..3 {
            for c in 0..4 {
                frob_sq += p_cond[(cam * 3 + r, c)].powi(2);
            }
        }
        let frob = frob_sq.sqrt();
        if frob > 0.0 {
            for r in 0..3 {
                for c in 0..4 {
                    p_cond[(cam * 3 + r, c)] /= frob;
                }
            }
        }
    }

    // Normalize X (each column has unit norm)
    let mut x_cond = x0.clone();
    for pt in 0..n {
        let mut col_norm = 0.0;
        for r in 0..4 {
            col_norm += x_cond[(r, pt)].powi(2);
        }
        let col_norm = col_norm.sqrt();
        if col_norm > 0.0 {
            for r in 0..4 {
                x_cond[(r, pt)] /= col_norm;
            }
        }
    }

    // Build visibility matrix and observation vector y
    let mut qivis = DMatrix::<bool>::from_element(k, n, false);
    for cam in 0..k {
        for pt in 0..n {
            let u = q[(cam * 2, pt)];
            let v = q[(cam * 2 + 1, pt)];
            qivis[(cam, pt)] = !u.is_nan() && !v.is_nan();
        }
    }
    let nvis: usize = qivis.iter().filter(|&&v| v).count();
    let mut y = DVector::<f64>::zeros(2 * nvis);
    let mut vis_entries: Vec<(usize, usize)> = Vec::with_capacity(nvis); // (cam, pt)
    let mut idx = 0;
    for pt in 0..n {
        for cam in 0..k {
            if qivis[(cam, pt)] {
                y[idx * 2] = q[(cam * 2, pt)];
                y[idx * 2 + 1] = q[(cam * 2 + 1, pt)];
                vis_entries.push((cam, pt));
                idx += 1;
            }
        }
    }

    // Compute tangent-space bases using the orthogonal complement.
    //
    // For a unit vector v (dim×1), the tangent space is the (dim-1)
    // dimensional subspace orthogonal to v.  Octave's `qr(v)` returns
    // the full Q; columns 2..dim span the tangent space.  nalgebra's
    // `qr().q()` is thin, so we compute the full Q via Householder
    // reflection instead.

    // TX{n}: 4×3 matrix, X(:,n) = X0(:,n) + TX{n} * iX
    let tx: Vec<DMatrix<f64>> = (0..n)
        .map(|pt| {
            let col = x_cond.column(pt).clone_owned();
            tangent_basis(&DVector::from_column_slice(col.as_slice()))
        })
        .collect();

    // TP{k}: 12×11 matrix, P_k(:) = P0_k(:) + TP{k} * iP
    let tp: Vec<DMatrix<f64>> = (0..k)
        .map(|cam| {
            let mut pvec = DVector::<f64>::zeros(12);
            for r in 0..3 {
                for c in 0..4 {
                    pvec[c * 3 + r] = p_cond[(cam * 3 + r, c)];
                }
            }
            tangent_basis(&pvec)
        })
        .collect();

    // Number of parameters: 11*K + 3*N
    let n_params = 11 * k + 3 * n;
    let _n_resid = 2 * nvis;

    // Run Levenberg-Marquardt
    let mut params = DVector::<f64>::zeros(n_params);
    let mut lam = 1e-4_f64;
    let mut n_fail = 0;
    let mut n_iter = 0;
    let max_iter = 10000;
    let max_stepy = 100.0 * f64::EPSILON;

    // Index of visible entries per camera and per point (for assembling
    // the block-sparse normal equations without scanning all entries).
    let mut vis_by_cam: Vec<Vec<usize>> = vec![Vec::new(); k];
    let mut vis_by_pt: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (l, &(cam, pt)) in vis_entries.iter().enumerate() {
        vis_by_cam[cam].push(l);
        vis_by_pt[pt].push(l);
    }

    // Closure: evaluate residuals only (no Jacobian).
    let eval_resid = |params: &DVector<f64>| -> DVector<f64> {
        eval_residuals_only(params, &p_cond, &tp, &x_cond, &tx, &y, &vis_entries, k, n)
    };

    // Closure: evaluate residuals + per-entry Jacobian blocks.
    let eval_full = |params: &DVector<f64>| -> (DVector<f64>, Vec<VisBlocks>) {
        eval_residuals_and_blocks(params, &p_cond, &tp, &x_cond, &tx, &y, &vis_entries, k, n)
    };

    let (mut fp, mut blocks) = eval_full(&params);
    let mut _last_fp_norm = f64::INFINITY;
    let mut stepy = f64::INFINITY;

    while n_fail < 20 && stepy > max_stepy && n_iter < max_iter {
        // Solve (J'J + λI) D = -J'F via Schur complement over the
        // point-parameter block.
        //
        // With params ordered [iP; iX]:
        //   H = [ U  W ]     g = [ gp ]
        //       [ W' V ]         [ gx ]
        // where U is 11K×11K block-diagonal (over cameras),
        //       V is 3N×3N block-diagonal (over points),
        //       W is 11K×3N with at most one nonzero 11×3 block per
        //       visible (cam, pt) pair.
        //
        // Solve  S Dp = -gp + W (V+λI)^{-1} gx
        //        Dx   = (V+λI)^{-1} ( -gx - W' Dp )
        // with   S = (U+λI) - W (V+λI)^{-1} W'.
        let d = match solve_schur(
            &blocks,
            &fp,
            &vis_entries,
            &vis_by_cam,
            &vis_by_pt,
            k,
            n,
            lam,
        ) {
            Some(d) => d,
            None => break,
        };
        if d.iter().any(|v| v.is_nan() || v.is_infinite()) {
            break;
        }

        let new_params = &params + &d;
        let fp_new = eval_resid(&new_params);
        let old_cost: f64 = fp.iter().map(|v| v * v).sum();
        let new_cost: f64 = fp_new.iter().map(|v| v * v).sum();

        if new_cost < old_cost {
            // Success: track max absolute change in residuals
            let old_fp = std::mem::replace(&mut fp, DVector::zeros(0));
            params = new_params;
            lam = (lam / 10.0).max(1e-15);
            let (new_fp, new_blocks) = eval_full(&params);
            stepy = new_fp
                .iter()
                .zip(old_fp.iter())
                .map(|(a, b)| (a - b).abs())
                .fold(0.0_f64, f64::max);
            _last_fp_norm = old_cost;
            fp = new_fp;
            blocks = new_blocks;
            n_fail = 0;
            n_iter += 1;
        } else {
            // Failure
            lam = (lam * 10.0).min(1e5);
            n_fail += 1;
        }
    }
    let _ = n_params; // silence unused-variable lint; kept for parity w/ octave

    let final_rms = (fp.iter().map(|v| v * v).sum::<f64>() / fp.len() as f64).sqrt();
    let final_max = fp.iter().map(|v| v.abs()).fold(0.0_f64, f64::max);
    tracing::debug!(
        "Projective BA: {n_iter} iters, n_fail={n_fail}, stepy={stepy:.6e}, rms={final_rms:.6}, max={final_max:.6}",
    );

    // Rearrange p back to P, X and undo preconditioning
    let mut p_out = DMatrix::<f64>::zeros(3 * k, 4);
    for cam in 0..k {
        let ip = params.rows(cam * 11, 11);
        // P_k = P0_k + reshape(TP{k} * iP, 3, 4)
        let delta = &tp[cam] * ip;
        let mut p_block = DMatrix::<f64>::zeros(3, 4);
        for r in 0..3 {
            for c in 0..4 {
                p_block[(r, c)] = p_cond[(cam * 3 + r, c)] + delta[c * 3 + r];
            }
        }
        // Undo conditioning: P = inv(H) * P_conditioned
        let p_uncond = h_inv_mats[cam] * &p_block;
        for r in 0..3 {
            for c in 0..4 {
                p_out[(cam * 3 + r, c)] = p_uncond[(r, c)];
            }
        }
    }

    let mut x_out = DMatrix::<f64>::zeros(4, n);
    let pt_offset = 11 * k;
    for pt in 0..n {
        let ix = params.rows(pt_offset + pt * 3, 3);
        let delta = &tx[pt] * ix;
        for r in 0..4 {
            x_out[(r, pt)] = x_cond[(r, pt)] + delta[r];
        }
    }

    (p_out, x_out)
}

/// Rebuild the current conditioned `P` (3K×4) and `X` (4×N) from the
/// tangent-space parameter vector.
fn apply_params(
    params: &DVector<f64>,
    p0: &DMatrix<f64>,
    tp: &[DMatrix<f64>],
    x0: &DMatrix<f64>,
    tx: &[DMatrix<f64>],
    k: usize,
    n: usize,
) -> (DMatrix<f64>, DMatrix<f64>) {
    let mut p_cur = p0.clone();
    for cam in 0..k {
        let ip = params.rows(cam * 11, 11);
        let delta = &tp[cam] * ip;
        for r in 0..3 {
            for c in 0..4 {
                p_cur[(cam * 3 + r, c)] += delta[c * 3 + r];
            }
        }
    }
    let mut x_cur = x0.clone();
    let pt_offset = 11 * k;
    for pt in 0..n {
        let ix = params.rows(pt_offset + pt * 3, 3);
        let delta = &tx[pt] * ix;
        for r in 0..4 {
            x_cur[(r, pt)] += delta[r];
        }
    }
    (p_cur, x_cur)
}

/// Compute residuals only (no Jacobian).
#[allow(clippy::too_many_arguments)]
fn eval_residuals_only(
    params: &DVector<f64>,
    p0: &DMatrix<f64>,
    tp: &[DMatrix<f64>],
    x0: &DMatrix<f64>,
    tx: &[DMatrix<f64>],
    y_obs: &DVector<f64>,
    vis_entries: &[(usize, usize)],
    k: usize,
    n: usize,
) -> DVector<f64> {
    let (p_cur, x_cur) = apply_params(params, p0, tp, x0, tx, k, n);
    let n_resid = 2 * vis_entries.len();
    let mut residuals = DVector::<f64>::zeros(n_resid);
    for (l, &(cam, pt)) in vis_entries.iter().enumerate() {
        // Compute P_cam * X_pt (3-vector) explicitly.
        let mut px = [0.0f64; 3];
        for r in 0..3 {
            let mut s = 0.0;
            for c in 0..4 {
                s += p_cur[(cam * 3 + r, c)] * x_cur[(c, pt)];
            }
            px[r] = s;
        }
        let pred_u = px[0] / px[2];
        let pred_v = px[1] / px[2];
        residuals[l * 2] = pred_u - y_obs[l * 2];
        residuals[l * 2 + 1] = pred_v - y_obs[l * 2 + 1];
    }
    residuals
}

/// Compute residuals and per-visible-entry 2×11 / 2×3 Jacobian blocks.
///
/// Equivalent to `eval_y_and_dy.m`, but returns the per-entry blocks
/// (jp = dq/diP, jx = dq/diX) instead of assembling the full sparse
/// Jacobian.  The full Jacobian has at most 2·(11+3) = 28 nonzeros per
/// row, and we never need to materialize it.
#[allow(clippy::too_many_arguments)]
fn eval_residuals_and_blocks(
    params: &DVector<f64>,
    p0: &DMatrix<f64>,
    tp: &[DMatrix<f64>],
    x0: &DMatrix<f64>,
    tx: &[DMatrix<f64>],
    y_obs: &DVector<f64>,
    vis_entries: &[(usize, usize)],
    k: usize,
    n: usize,
) -> (DVector<f64>, Vec<VisBlocks>) {
    let (p_cur, x_cur) = apply_params(params, p0, tp, x0, tx, k, n);
    let n_vis = vis_entries.len();
    let n_resid = 2 * n_vis;

    let mut residuals = DVector::<f64>::zeros(n_resid);
    let mut blocks = Vec::<VisBlocks>::with_capacity(n_vis);

    for (l, &(cam, pt)) in vis_entries.iter().enumerate() {
        // x = P_cam * X_pt
        let mut xv = [0.0f64; 3];
        for r in 0..3 {
            let mut s = 0.0;
            for c in 0..4 {
                s += p_cur[(cam * 3 + r, c)] * x_cur[(c, pt)];
            }
            xv[r] = s;
        }
        let inv_x3 = 1.0 / xv[2];
        let u = xv[0] * inv_x3;
        let v = xv[1] * inv_x3;
        residuals[l * 2] = u - y_obs[l * 2];
        residuals[l * 2 + 1] = v - y_obs[l * 2 + 1];

        // dudx = [[1/x3, 0, -u/x3], [0, 1/x3, -v/x3]]  (2×3)
        let dudx: [[f64; 3]; 2] = [[inv_x3, 0.0, -u * inv_x3], [0.0, inv_x3, -v * inv_x3]];

        // dxdP[row, col] = sum_{c2=0..4} X_pt[c2] * TP[cam][c2*3+row, col]
        // where TP[cam] is 12×11. Result is 3×11.
        let x_pt = [
            x_cur[(0, pt)],
            x_cur[(1, pt)],
            x_cur[(2, pt)],
            x_cur[(3, pt)],
        ];
        let tp_cam = &tp[cam];
        let mut dxdp = [[0.0f64; 11]; 3];
        for (c2, &xc) in x_pt.iter().enumerate() {
            for (row, dxdp_row) in dxdp.iter_mut().enumerate() {
                let tp_row = c2 * 3 + row;
                for col in 0..11 {
                    dxdp_row[col] += xc * tp_cam[(tp_row, col)];
                }
            }
        }

        // dxdX = P_cam * TX{pt}  (3×4 * 4×3 = 3×3)
        let tx_pt = &tx[pt];
        let mut dxdx = [[0.0f64; 3]; 3];
        for row in 0..3 {
            for col in 0..3 {
                let mut s = 0.0;
                for c2 in 0..4 {
                    s += p_cur[(cam * 3 + row, c2)] * tx_pt[(c2, col)];
                }
                dxdx[row][col] = s;
            }
        }

        // jp (2×11) = dudx (2×3) * dxdp (3×11)
        let mut jp = [0.0f64; 22];
        for res_r in 0..2 {
            for col in 0..11 {
                let mut s = 0.0;
                for mid in 0..3 {
                    s += dudx[res_r][mid] * dxdp[mid][col];
                }
                jp[res_r * 11 + col] = s;
            }
        }
        // jx (2×3) = dudx (2×3) * dxdx (3×3)
        let mut jx = [0.0f64; 6];
        for res_r in 0..2 {
            for col in 0..3 {
                let mut s = 0.0;
                for mid in 0..3 {
                    s += dudx[res_r][mid] * dxdx[mid][col];
                }
                jx[res_r * 3 + col] = s;
            }
        }

        blocks.push(VisBlocks { jp, jx });
    }

    (residuals, blocks)
}

/// Solve `(J'J + λI) D = -J'F` using the block Schur complement on the
/// point-parameter block.
///
/// Given the block structure of J,
/// ```text
///     H = [ U  W ]     g = [ gp ]      U ∈ R^{11K x 11K} block-diag cam
///         [ W' V ]         [ gx ]      V ∈ R^{3N x 3N}   block-diag pt
/// ```
/// we solve
///     (U + λI - W (V+λI)^{-1} W')  Dp = -gp + W (V+λI)^{-1} gx
///     Dx = (V+λI)^{-1} ( -gx - W' Dp )
///
/// The reduced system is dense 11K × 11K (small) and V is block-diagonal
/// in 3×3 blocks, so its inverse is trivial.
#[allow(clippy::too_many_arguments)]
fn solve_schur(
    blocks: &[VisBlocks],
    fp: &DVector<f64>,
    vis_entries: &[(usize, usize)],
    vis_by_cam: &[Vec<usize>],
    vis_by_pt: &[Vec<usize>],
    k: usize,
    n: usize,
    lam: f64,
) -> Option<DVector<f64>> {
    // ---- Build gp (11K), gx (3N): g = J' F. ----
    let mut gp = vec![0.0f64; 11 * k];
    let mut gx = vec![0.0f64; 3 * n];
    for (l, &(cam, pt)) in vis_entries.iter().enumerate() {
        let b = &blocks[l];
        let f0 = fp[2 * l];
        let f1 = fp[2 * l + 1];
        let cam_off = cam * 11;
        for col in 0..11 {
            gp[cam_off + col] += b.jp[col] * f0 + b.jp[11 + col] * f1;
        }
        let pt_off = pt * 3;
        for col in 0..3 {
            gx[pt_off + col] += b.jx[col] * f0 + b.jx[3 + col] * f1;
        }
    }

    // ---- Build V+λI per point (3×3), and invert it. ----
    // Also stash W-blocks per (cam, pt) -- but since each vis entry has
    // exactly one (cam, pt), we just use `blocks[l]` directly and index
    // via vis_by_cam / vis_by_pt.
    let mut v_inv: Vec<[f64; 9]> = vec![[0.0; 9]; n]; // row-major 3×3
    for pt in 0..n {
        if vis_by_pt[pt].is_empty() {
            // No observations of this point: its normal block is λI, so
            // the inverse is (1/λ)I.  (In practice such points have zero
            // gx, and the solution is trivially zero.)
            let inv_l = 1.0 / lam;
            v_inv[pt] = [inv_l, 0.0, 0.0, 0.0, inv_l, 0.0, 0.0, 0.0, inv_l];
            continue;
        }
        // V_pt = sum_{l : vis_by_pt[pt]} jx[l]' jx[l]   (3×3)
        let mut v = [0.0f64; 9];
        for &l in &vis_by_pt[pt] {
            let jx = &blocks[l].jx;
            // jx is 2×3 row-major; jx' * jx:
            for r in 0..3 {
                for c in 0..3 {
                    v[r * 3 + c] += jx[r] * jx[c] + jx[3 + r] * jx[3 + c];
                }
            }
        }
        for d in 0..3 {
            v[d * 3 + d] += lam;
        }
        v_inv[pt] = invert3x3(&v)?;
    }

    // ---- Precompute per-visible-entry "reduced" blocks used below. ----
    //
    // For each visible entry l = (cam, pt):
    //   Z[l] = W_block[cam,pt] * Vinv[pt]       (11×3)
    //        = jp[l]' * jx[l] * Vinv[pt]
    // (since W_block[cam,pt] = sum over that single vis entry of jp'·jx = 11×3.)
    //
    // Then the Schur block between camera c1 and c2 is:
    //   S[c1,c2] = U[c1,c2] + λI δ_{c1,c2}
    //            - sum_{pt vis (c1,c2)} Z[l(c1,pt)] * (W_block[c2,pt])'
    // and the RHS camera block is:
    //   rhs_p[c1] = -gp[c1] + sum_{pt vis c1} Z[l(c1,pt)] * gx[pt]
    //
    // U[c1,c1] = sum over cam=c1 vis entries of jp'·jp  (11×11).
    let mut w_vinv: Vec<[f64; 33]> = Vec::with_capacity(vis_entries.len()); // 11×3 row-major
    for (l, &(_cam, pt)) in vis_entries.iter().enumerate() {
        let jp = &blocks[l].jp;
        let jx = &blocks[l].jx;
        let vinv = &v_inv[pt];
        // wblk = jp' * jx  (11×2 * 2×3 = 11×3)
        let mut wblk = [0.0f64; 33];
        for r in 0..11 {
            for c in 0..3 {
                wblk[r * 3 + c] = jp[r] * jx[c] + jp[11 + r] * jx[3 + c];
            }
        }
        // z = wblk * vinv  (11×3)
        let mut z = [0.0f64; 33];
        for r in 0..11 {
            for c in 0..3 {
                let mut s = 0.0;
                for mid in 0..3 {
                    s += wblk[r * 3 + mid] * vinv[mid * 3 + c];
                }
                z[r * 3 + c] = s;
            }
        }
        w_vinv.push(z);
    }

    // ---- Assemble reduced (Schur) system S (11K × 11K) and rhs_p (11K). ----
    let dim = 11 * k;
    let mut s = DMatrix::<f64>::zeros(dim, dim);
    let mut rhs_p = DVector::<f64>::zeros(dim);

    // Diagonal: S[c,c] += U[c,c] + λI - sum_{pt vis c} Z[l(c,pt)] * W[c,pt]'
    // Off-diag: S[c1,c2] -= sum_{pt vis both} Z[l(c1,pt)] * W[c2,pt]'
    //
    // We proceed point-by-point: for each point pt, for each pair of
    // cameras (c1, c2) that see it, accumulate Z[l1] * W[l2]' into
    // S[c1,c2]. This is O(sum_pt n_cams_per_pt^2), typically K²·n_pts
    // worst case — a few million flops for K=20.
    for pt in 0..n {
        let ls = &vis_by_pt[pt];
        for &l1 in ls {
            let c1 = vis_entries[l1].0;
            let z1 = &w_vinv[l1]; // 11×3
            // rhs contribution: rhs_p[c1] += z1 * gx[pt]
            let gx_pt = [gx[3 * pt], gx[3 * pt + 1], gx[3 * pt + 2]];
            let c1_off = c1 * 11;
            for r in 0..11 {
                rhs_p[c1_off + r] +=
                    z1[r * 3] * gx_pt[0] + z1[r * 3 + 1] * gx_pt[1] + z1[r * 3 + 2] * gx_pt[2];
            }
            for &l2 in ls {
                let c2 = vis_entries[l2].0;
                let jp2 = &blocks[l2].jp;
                let jx2 = &blocks[l2].jx;
                // W[c2,pt]' = (jp2' * jx2)' = jx2' * jp2    (3 × 11)
                // Need Z1 (11×3) * W[c2,pt]' (3×11) = 11×11 subtracted from S.
                // Compute W[c2,pt]' directly: wt[i,j] = sum_{t=0..1} jx2[t*3+i] * jp2[t*11+j]
                let mut wt = [0.0f64; 33]; // 3×11
                for i in 0..3 {
                    for j in 0..11 {
                        wt[i * 11 + j] = jx2[i] * jp2[j] + jx2[3 + i] * jp2[11 + j];
                    }
                }
                // contrib[r,c] = sum_{m=0..3} z1[r*3+m] * wt[m*11+c]
                let c2_off = c2 * 11;
                for r in 0..11 {
                    let z10 = z1[r * 3];
                    let z11 = z1[r * 3 + 1];
                    let z12 = z1[r * 3 + 2];
                    for c in 0..11 {
                        let v = z10 * wt[c] + z11 * wt[11 + c] + z12 * wt[22 + c];
                        s[(c1_off + r, c2_off + c)] -= v;
                    }
                }
            }
        }
    }

    // Add U[c,c] (= sum over vis entries in camera c of jp' jp) + λI to diagonal blocks.
    let _ = k; // k is implied by vis_by_cam.len()
    for (cam, cam_vis) in vis_by_cam.iter().enumerate() {
        let off = cam * 11;
        for &l in cam_vis {
            let jp = &blocks[l].jp;
            for r in 0..11 {
                for c in 0..11 {
                    s[(off + r, off + c)] += jp[r] * jp[c] + jp[11 + r] * jp[11 + c];
                }
            }
        }
        for d in 0..11 {
            s[(off + d, off + d)] += lam;
        }
    }

    // rhs_p = -gp + rhs_p_so_far
    for i in 0..dim {
        rhs_p[i] -= gp[i];
    }

    // Solve S * Dp = rhs_p via Cholesky.  S is SPD for λ > 0.
    let dp = match s.cholesky() {
        Some(chol) => chol.solve(&rhs_p),
        None => return None,
    };

    // Back-substitute: Dx[pt] = Vinv[pt] * ( -gx[pt] - sum_{cam vis pt} W[cam,pt]' * Dp[cam] )
    let mut dx = DVector::<f64>::zeros(3 * n);
    for pt in 0..n {
        let mut tmp = [-gx[3 * pt], -gx[3 * pt + 1], -gx[3 * pt + 2]];
        for &l in &vis_by_pt[pt] {
            let cam = vis_entries[l].0;
            let cam_off = cam * 11;
            let jp = &blocks[l].jp;
            let jx = &blocks[l].jx;
            // W[cam,pt]' * dp_cam (3-vector):
            // W = jp'*jx (11×3) → W' = jx' * jp (3×11)
            // W'[i,j] = jx[0*3+i]*jp[0*11+j] + jx[1*3+i]*jp[1*11+j]
            for i in 0..3 {
                let mut s2 = 0.0;
                for j in 0..11 {
                    let wt_ij = jx[i] * jp[j] + jx[3 + i] * jp[11 + j];
                    s2 += wt_ij * dp[cam_off + j];
                }
                tmp[i] -= s2;
            }
        }
        let vinv = &v_inv[pt];
        for r in 0..3 {
            dx[3 * pt + r] =
                vinv[r * 3] * tmp[0] + vinv[r * 3 + 1] * tmp[1] + vinv[r * 3 + 2] * tmp[2];
        }
    }

    // Stack [dp; dx].
    let mut d = DVector::<f64>::zeros(11 * k + 3 * n);
    for i in 0..dim {
        d[i] = dp[i];
    }
    for i in 0..3 * n {
        d[dim + i] = dx[i];
    }
    Some(d)
}

/// Invert a 3×3 row-major matrix. Returns None on near-singular matrix.
fn invert3x3(m: &[f64; 9]) -> Option<[f64; 9]> {
    let a = m[0];
    let b = m[1];
    let c = m[2];
    let d = m[3];
    let e = m[4];
    let f = m[5];
    let g = m[6];
    let h = m[7];
    let i = m[8];
    let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    if !det.is_finite() || det.abs() < 1e-300 {
        return None;
    }
    let inv_det = 1.0 / det;
    Some([
        (e * i - f * h) * inv_det,
        (c * h - b * i) * inv_det,
        (b * f - c * e) * inv_det,
        (f * g - d * i) * inv_det,
        (a * i - c * g) * inv_det,
        (c * d - a * f) * inv_det,
        (d * h - e * g) * inv_det,
        (b * g - a * h) * inv_det,
        (a * e - b * d) * inv_det,
    ])
}

/// Image conditioner from image size.
///
/// Equivalent to `vgg_conditioner_from_image` in `bundle_PX_proj.m`.
/// Compute an orthonormal basis for the tangent space (orthogonal complement)
/// of a vector `v`.
///
/// For a dim-dimensional vector, returns a dim × (dim−1) matrix whose
/// columns form an orthonormal basis for the subspace orthogonal to `v`.
///
/// Equivalent to the octave idiom `[Q,~] = qr(v); T = Q(:,2:end)'` used
/// in `bundle_PX_proj.m` to build the tangent-space bases `TP{k}` and
/// `TX{n}`.  Octave's `qr` of a dim×1 vector returns the full dim×dim Q;
/// nalgebra's `qr().q()` is thin (dim×1), so we use a Householder
/// reflector instead.
fn tangent_basis(v: &DVector<f64>) -> DMatrix<f64> {
    let dim = v.len();
    // Build the Householder reflector that maps v to e1 * ||v||
    // Q = I - 2 * u * u' / (u' * u),  where u = v - ||v|| * e1
    // Then Q * v = ||v|| * e1, and Q(:,2:end) spans the orthogonal complement.
    let v_norm = v.norm();
    if v_norm < 1e-15 {
        // Degenerate: return identity columns 1..dim
        return DMatrix::identity(dim, dim - 1);
    }

    // Use nalgebra's full QR on a dim×1 matrix by constructing it manually
    // via Householder reflection
    let mut u = v.clone();
    if u[0] >= 0.0 {
        u[0] += v_norm;
    } else {
        u[0] -= v_norm;
    }
    let u_norm_sq = u.dot(&u);

    // Q = I - 2*u*u'/||u||^2
    // We only need columns 1..dim-1 of Q
    let mut result = DMatrix::<f64>::zeros(dim, dim - 1);
    for col in 0..(dim - 1) {
        // Q * e_{col+1}
        // = e_{col+1} - 2 * u * u[col+1] / ||u||^2
        let factor = 2.0 * u[col + 1] / u_norm_sq;
        for row in 0..dim {
            let e_val = if row == col + 1 { 1.0 } else { 0.0 };
            result[(row, col)] = e_val - factor * u[row];
        }
    }
    result
}

/// Image conditioner from image size (or principal point = half-resolution).
///
/// Equivalent to `vgg_conditioner_from_image` in `bundle_PX_proj.m`.
/// Accepts `[pp_x, pp_y]` which are half the image width and height.
fn vgg_conditioner_from_image(imsize: [f64; 2]) -> (Matrix3<f64>, Matrix3<f64>) {
    let c = imsize[0];
    let r = imsize[1];
    let f = (c + r) / 2.0;

    let h = Matrix3::new(
        1.0 / f,
        0.0,
        -c / (2.0 * f),
        0.0,
        1.0 / f,
        -r / (2.0 * f),
        0.0,
        0.0,
        1.0,
    );
    let h_inv = Matrix3::new(f, 0.0, c / 2.0, 0.0, f, r / 2.0, 0.0, 0.0, 1.0);

    (h, h_inv)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nalgebra::DVector;

    /// Verify the Schur-complement solver produces the same step as a
    /// full dense `(J'J + λI) d = -J'F` solve on a small synthetic BA
    /// problem.  This is the regression test for the LM-loop rewrite
    /// that replaced a dense Jacobian with block Schur-complement
    /// normal-equations assembly.
    #[test]
    fn test_schur_matches_dense() {
        use rand::Rng;
        use rand::SeedableRng;
        use rand::rngs::StdRng;

        // Small problem: 3 cams, 5 points, partial visibility.
        let k = 3;
        let n = 5;
        let mut rng = StdRng::seed_from_u64(42);

        // Random P (3K×4) and X (4×N), roughly in a sensible scale.
        let mut p0 = DMatrix::<f64>::zeros(3 * k, 4);
        for i in 0..3 * k {
            for j in 0..4 {
                p0[(i, j)] = rng.random::<f64>() - 0.5;
            }
        }
        let mut x0 = DMatrix::<f64>::zeros(4, n);
        for i in 0..4 {
            for j in 0..n {
                x0[(i, j)] = rng.random::<f64>() - 0.5;
            }
        }
        // Make X[3,:] positive so x3 != 0.
        for j in 0..n {
            x0[(3, j)] = 1.0 + 0.1 * rng.random::<f64>();
        }

        // Normalize P (Frobenius) and X (unit column) like bundle_px_proj.
        for cam in 0..k {
            let mut f = 0.0;
            for r in 0..3 {
                for c in 0..4 {
                    f += p0[(cam * 3 + r, c)].powi(2);
                }
            }
            let f = f.sqrt();
            for r in 0..3 {
                for c in 0..4 {
                    p0[(cam * 3 + r, c)] /= f;
                }
            }
        }
        for pt in 0..n {
            let mut s = 0.0;
            for r in 0..4 {
                s += x0[(r, pt)].powi(2);
            }
            let s = s.sqrt();
            for r in 0..4 {
                x0[(r, pt)] /= s;
            }
        }

        // Tangent bases.
        let tx: Vec<DMatrix<f64>> = (0..n)
            .map(|pt| tangent_basis(&DVector::from_column_slice(x0.column(pt).as_slice())))
            .collect();
        let tp: Vec<DMatrix<f64>> = (0..k)
            .map(|cam| {
                let mut v = DVector::<f64>::zeros(12);
                for r in 0..3 {
                    for c in 0..4 {
                        v[c * 3 + r] = p0[(cam * 3 + r, c)];
                    }
                }
                tangent_basis(&v)
            })
            .collect();

        // Visibility: leave out (cam=1, pt=3) and (cam=2, pt=0).
        let mut vis_entries = Vec::new();
        for pt in 0..n {
            for cam in 0..k {
                if (cam, pt) == (1, 3) || (cam, pt) == (2, 0) {
                    continue;
                }
                vis_entries.push((cam, pt));
            }
        }

        // Synthetic observations: project X by P, then perturb slightly.
        let mut y = DVector::<f64>::zeros(2 * vis_entries.len());
        for (l, &(cam, pt)) in vis_entries.iter().enumerate() {
            let mut px = [0.0; 3];
            for r in 0..3 {
                for c in 0..4 {
                    px[r] += p0[(cam * 3 + r, c)] * x0[(c, pt)];
                }
            }
            y[2 * l] = px[0] / px[2] + 0.01 * (rng.random::<f64>() - 0.5);
            y[2 * l + 1] = px[1] / px[2] + 0.01 * (rng.random::<f64>() - 0.5);
        }

        // Perturb params away from zero to get a non-trivial Jacobian.
        let n_params = 11 * k + 3 * n;
        let mut params = DVector::<f64>::zeros(n_params);
        for i in 0..n_params {
            params[i] = 0.05 * (rng.random::<f64>() - 0.5);
        }

        let (fp, blocks) =
            eval_residuals_and_blocks(&params, &p0, &tp, &x0, &tx, &y, &vis_entries, k, n);

        // Build a dense Jacobian from the per-entry blocks so we can
        // form J'J + λI and solve densely for the reference step.
        let nvis = vis_entries.len();
        let mut jac = DMatrix::<f64>::zeros(2 * nvis, n_params);
        for (l, &(cam, pt)) in vis_entries.iter().enumerate() {
            let b = &blocks[l];
            for r in 0..2 {
                for c in 0..11 {
                    jac[(2 * l + r, cam * 11 + c)] = b.jp[r * 11 + c];
                }
                for c in 0..3 {
                    jac[(2 * l + r, 11 * k + pt * 3 + c)] = b.jx[r * 3 + c];
                }
            }
        }
        let lam = 1.0_f64;
        let jtj = jac.transpose() * &jac;
        let jtf = jac.transpose() * &fp;
        let mut lhs = jtj;
        for i in 0..n_params {
            lhs[(i, i)] += lam;
        }
        let d_dense = lhs.cholesky().unwrap().solve(&-jtf);

        let mut vis_by_cam: Vec<Vec<usize>> = vec![Vec::new(); k];
        let mut vis_by_pt: Vec<Vec<usize>> = vec![Vec::new(); n];
        for (l, &(cam, pt)) in vis_entries.iter().enumerate() {
            vis_by_cam[cam].push(l);
            vis_by_pt[pt].push(l);
        }
        let d_schur = solve_schur(
            &blocks,
            &fp,
            &vis_entries,
            &vis_by_cam,
            &vis_by_pt,
            k,
            n,
            lam,
        )
        .unwrap();

        // The two step vectors must agree.  Tolerance reflects that
        // the Schur path involves a hand-rolled 3×3 inverse and
        // different accumulation order, so bit-identity is not expected
        // even though both solve the same linear system.
        let max_dense = d_dense.iter().fold(0.0_f64, |a, &b| a.max(b.abs()));
        for i in 0..n_params {
            let diff = (d_dense[i] - d_schur[i]).abs();
            assert!(
                diff < 1e-6 * max_dense.max(1.0),
                "step[{i}] mismatch: dense={}, schur={}, diff={}",
                d_dense[i],
                d_schur[i],
                diff
            );
        }
    }

    #[test]
    fn test_tangent_basis_4d() {
        // Compare with octave: v = [0.3; -0.5; 0.7; 0.1]
        // [Q,~] = qr(v); TX = Q(:,2:end)' (= Q(2:end,:)' = 4x3)
        let v = DVector::from_column_slice(&[0.3, -0.5, 0.7, 0.1]);
        let t = tangent_basis(&v);
        assert_eq!(t.nrows(), 4);
        assert_eq!(t.ncols(), 3);

        // Columns must be orthogonal to v
        for c in 0..3 {
            let dot: f64 = (0..4).map(|r| t[(r, c)] * v[r]).sum();
            assert!(
                dot.abs() < 1e-12,
                "column {c} not orthogonal to v: dot={dot}"
            );
        }

        // Columns must be orthonormal
        for c1 in 0..3 {
            for c2 in 0..3 {
                let dot: f64 = (0..4).map(|r| t[(r, c1)] * t[(r, c2)]).sum();
                let expected = if c1 == c2 { 1.0 } else { 0.0 };
                assert!(
                    (dot - expected).abs() < 1e-12,
                    "T'T[{c1},{c2}]={dot}, expected {expected}"
                );
            }
        }
    }

    #[test]
    fn test_tangent_basis_12d() {
        // Simulate a P-matrix tangent basis (12-vector)
        let mut v = DVector::from_column_slice(&[
            1.0, -2.0, 3.0, 0.5, -0.1, 0.8, -1.5, 0.3, 0.2, -0.7, 1.1, -0.4,
        ]);
        let v_norm = v.norm();
        v /= v_norm;
        let t = tangent_basis(&v);
        assert_eq!(t.nrows(), 12);
        assert_eq!(t.ncols(), 11);

        // Orthogonal to v
        for c in 0..11 {
            let dot: f64 = (0..12).map(|r| t[(r, c)] * v[r]).sum();
            assert!(dot.abs() < 1e-12, "column {c} not orthogonal: dot={dot}");
        }

        // Orthonormal
        for c1 in 0..11 {
            for c2 in c1..11 {
                let dot: f64 = (0..12).map(|r| t[(r, c1)] * t[(r, c2)]).sum();
                let expected = if c1 == c2 { 1.0 } else { 0.0 };
                assert!(
                    (dot - expected).abs() < 1e-12,
                    "T'T[{c1},{c2}]={dot}, expected {expected}"
                );
            }
        }
    }

    #[test]
    fn test_tangent_basis_matches_octave() {
        // Exact comparison with octave output for v=[0.3; -0.5; 0.7; 0.1]
        // Octave: [Q,~]=qr(v); TX=Q(2:end,:)'
        let v = DVector::from_column_slice(&[0.3, -0.5, 0.7, 0.1]);
        let t = tangent_basis(&v);

        // Expected values from octave (12 decimal places)
        let expected = [
            [5.455447255900e-01, -7.637626158260e-01, -1.091089451180e-01],
            [7.757756117847e-01, 3.139141435015e-01, 4.484487764307e-02],
            [3.139141435015e-01, 5.605201990979e-01, -6.278282870029e-02],
            [4.484487764307e-02, -6.278282870029e-02, 9.910310244714e-01],
        ];
        for r in 0..4 {
            for c in 0..3 {
                assert!(
                    (t[(r, c)] - expected[r][c]).abs() < 1e-10,
                    "T[{r},{c}]={}, expected {}",
                    t[(r, c)],
                    expected[r][c]
                );
            }
        }
    }
}
