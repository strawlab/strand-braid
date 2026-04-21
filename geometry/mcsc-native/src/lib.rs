//! Native Rust port of MultiCamSelfCal (MCSC).
//!
//! This crate implements the core multi-camera self-calibration algorithm
//! from Svoboda et al., ported from the original Octave/Matlab code.
//!
//! The entry point is [`run_mcsc`], which corresponds to the Octave script
//! `gocal.m`. The main sub-algorithms live in:
//!
//! - `ransac` — RANSAC inlier validation (`findinl.m`, `rEG.m`)
//! - `fill_mm` — Martinec-Pajdla projective reconstruction (`fill_mm.m`,
//!   `fill_mm_bundle.m`)
//! - `projective_ba` — projective bundle adjustment
//!   (`bundle_PX_proj.m`, `eval_y_and_dy.m`)
//! - `utils` — shared helpers (`rq.m`, `undoradial.m`, `normu.m`,
//!   `pointnormiso.m`)
//!
//! # Pipeline overview
//!
//! The pipeline has three main phases, each of which preserves or improves
//! calibration quality:
//!
//! 1. **Projective reconstruction** (`fill_mm`): recovers P (3K×4) and
//!    X (4×N) such that `P*X ≈ M` up to per-entry projective depths.
//!
//! 2. **Euclidean upgrade** (`euclidize`): recovers camera intrinsics
//!    (K), rotations (R), translations (t), and metric 3-D points from
//!    the projective P and X.  This involves estimating the plane at
//!    infinity (B) and the absolute quadric (Q), then decomposing each
//!    camera's projection matrix via RQ factorization. The output Pe
//!    projects to **full** (non-centred) pixel coordinates, because the
//!    principal-point subtraction applied to K inside `euclidize`
//!    reverses the centring done in `gocal.m` / [`run_mcsc`].
//!
//! 3. **Outlier removal loop** (`gocal.m`): iterates steps 1-2, removing
//!    points whose reprojection error exceeds a threshold, until
//!    convergence.
//!
//! # Deviations from the Octave code
//!
//! Several deliberate deviations from a literal port exist:
//!
//! * **F-matrix denormalization** (`ransac::u2f_dlt`): uses the
//!   mathematically correct `T2' * F * T1` instead of Octave's
//!   `inv(T2) * F * T1`. See the function-level docs for the derivation.
//! * **Inlier re-evaluation after refinement** (`ransac::r_eg`): the
//!   refined F matrix is used to re-evaluate which points are inliers,
//!   rather than returning only the pre-refinement inlier set.
//! * **F-matrix normalization** (`ransac::u2f_dlt`): the spectral (2-)
//!   norm is used to match Octave's `norm(F,2)`, rather than the Frobenius
//!   norm.
//! * **Null-space computation** (`fill_mm::create_nullspace`): uses
//!   eigendecomposition of `M*M'` to obtain the full left-singular vectors
//!   of tall matrices, because nalgebra's SVD only returns a thin
//!   decomposition.
//! * **Deterministic RANSAC**: uses a seeded ChaCha8 PRNG instead of the
//!   runtime-seeded RNG that Octave uses, so results are reproducible
//!   across runs.
//!
//! This crate was primarily written by Opus 4.6 and Opus 4.7. This was done
//! comparing calibration results from sample datasets with the original Octave
//! code, and iterating until reprojection distances matched closely. The code
//! is not a line-by-line port of the Octave code, but it implements the same
//! underlying algorithms and produces similar outputs given the same inputs.
//! Some bugs in the original Octave code were identified and fixed in the
//! native rust code, resulting in improved accuracy and robustness in some
//! cases. The Octave code may contain some code paths which were not ported.
use eyre::Result;
use nalgebra::{DMatrix, DVector, Matrix3, Matrix3x4, SVD};

mod fill_mm;
mod io;
mod projective_ba;
mod ransac;
mod utils;

pub use fill_mm::FillMmOptions;
pub use io::{
    ini_to_mcsc_config, load_mcsc_data, parse_camera_order, parse_id_mat, parse_mcsc_config,
    parse_points_dat, parse_rad_file, parse_res_dat,
};

/// Configuration values parsed from multicamselfcal.cfg.
#[derive(Debug, Clone)]
pub struct McscIniConfig {
    pub config_dir: camino::Utf8PathBuf,
    pub num_cameras: usize,
    pub num_cams_fill_raw: Option<usize>,
    pub do_ba: bool,
    pub use_nth_frame: u16,
    pub inl_tol: f64,
}

/// Configuration for the MCSC algorithm.
#[derive(Debug, Clone)]
pub struct McscCfg {
    /// Number of cameras that can be missing for a point to still be used.
    /// If 0, only points visible in ALL cameras are used.
    pub num_cams_fill: usize,
    /// Inlier tolerance in pixels for RANSAC validation.
    pub inl_tol: f64,
    /// Whether to perform bundle adjustment at the end.
    pub do_bundle_adjustment: bool,
    /// Whether cameras have square pixels (aspect ratio = 1).
    ///
    /// Only applies to cameras whose intrinsics are **not** provided in
    /// `McscInput::intrinsics`: known-intrinsics cameras use the full
    /// supplied `K` as a hard constraint (see `use_known_intrinsics`).
    pub square_pix: bool,
    /// When true (the default) and a camera has an entry in
    /// `McscInput::intrinsics`, the Euclidean upgrade in `euclidize`
    /// treats that camera's intrinsic matrix `K` as a hard constraint
    /// ("extrinsics-only" mode for that camera). When false, MCSC
    /// self-calibrates all cameras as in the original algorithm, even
    /// when intrinsics are provided.
    ///
    /// Setting this to false is mainly useful for regression testing
    /// against the behaviour of the Octave MCSC pipeline.
    pub use_known_intrinsics: bool,
}

impl Default for McscCfg {
    fn default() -> Self {
        Self {
            num_cams_fill: 12,
            inl_tol: 5.0,
            do_bundle_adjustment: false,
            square_pix: true,
            use_known_intrinsics: true,
        }
    }
}

/// Input data for MCSC calibration.
pub struct McscInput {
    /// Visibility matrix (n_cams x n_points), true if point visible from camera.
    pub id_mat: DMatrix<bool>,
    /// Observations matrix (3*n_cams x n_points), homogeneous image coordinates.
    /// For invisible points, the values are irrelevant.
    pub points: DMatrix<f64>,
    /// Camera resolutions (n_cams x 2): [width, height] per camera.
    pub res: Vec<[usize; 2]>,
    /// Intrinsics parameters per camera, if present.
    /// Each entry is (K_3x3, kc_4x1).
    pub intrinsics: Vec<Option<(Matrix3<f64>, [f64; 4])>>,
    /// Camera names/identifiers.
    pub camera_names: Vec<String>,
}

/// Result of the MCSC calibration.
pub struct McscResult {
    /// Mean reprojection distance across all inlier points and cameras, in pixels.
    pub mean_reproj_distance: f64,
    /// Standard deviation of reprojection distance across all inlier points and cameras, in pixels.
    pub std_reproj_distance: f64,
    /// Projection matrices P_i (3x4) for each camera.
    pub projection_matrices: Vec<Matrix3x4<f64>>,
    /// Camera centers in world coordinates (3x1) for each camera.
    pub camera_centers: Vec<nalgebra::Vector3<f64>>,
    /// Rotation matrices (3x3) for each camera.
    pub rotations: Vec<Matrix3<f64>>,
    /// Calibration/intrinsic matrices K (3x3) for each camera.
    pub intrinsics: Vec<Matrix3<f64>>,
    /// Reconstructed 3D points (4 x n_inlier_points), homogeneous.
    pub points_3d: DMatrix<f64>,
    /// Indices of inlier points (into the original point set).
    pub inlier_indices: Vec<usize>,
    /// Per-camera 2D-3D correspondences for further calibration:
    /// (4D world point, 2D distorted pixel) pairs.
    pub points4cal: Vec<DMatrix<f64>>,
}

/// Run the multi-camera self-calibration algorithm.
///
/// This is the main entry point, equivalent to the Octave script `gocal.m`.
/// The high-level flow is:
///
/// 1. Optionally undo radial distortion (`undoradial.m`).
/// 2. Subtract principal points to centre coordinates. All subsequent
///    projective computations operate in this centred frame.
/// 3. RANSAC pairwise validation (`findinl.m`).
/// 4. Iterative outlier-removal loop:
///    a. Projective reconstruction (`fill_mm_bundle.m` → `fill_mm.m`).
///    b. Euclidean upgrade (`euclidize.m`). The resulting Pe matrices
///    project back to **full** (non-centred) pixel coordinates
///    because `euclidize` subtracts pp from K, which reverses the
///    centring from step 2.
///    c. Reprojection error computation (`reprerror.m`) against the
///    original (non-centred) pixel observations.
///    d. Outlier detection and removal (`findoutl.m`).
/// 5. Optionally refine with projective bundle adjustment.
/// 6. Decompose projection matrices into K, R, t per camera.
pub fn run_mcsc(input: McscInput, config: McscCfg) -> Result<McscResult> {
    let n_cams = input.id_mat.nrows();
    let n_frames = input.id_mat.ncols();

    if n_cams < 3 || n_frames < 20 {
        eyre::bail!(
            "Not enough cameras ({n_cams}) or frames ({n_frames}). Need >= 3 cameras and >= 20 frames."
        );
    }

    tracing::debug!("MCSC: {n_cams} cameras, {n_frames} frames");

    let mut num_cams_fill = config.num_cams_fill;
    if n_cams as isize - num_cams_fill as isize <= 2 {
        num_cams_fill = n_cams.saturating_sub(3);
    }
    let inl_tol = config.inl_tol;

    // Principal points: half of resolution, with 0 for third coordinate
    // pp is n_cams x 3: [w/2, h/2, 0]
    let mut pp = DMatrix::<f64>::zeros(n_cams, 3);
    for i in 0..n_cams {
        pp[(i, 0)] = input.res[i][0] as f64 / 2.0;
        pp[(i, 1)] = input.res[i][1] as f64 / 2.0;
        // pp[(i, 2)] = 0.0; // already zero
    }

    // Loaded Ws = input.points (3*n_cams x n_frames)
    let loaded_ws = &input.points;
    let id_mat = &input.id_mat;

    // linear.Ws = linearized coordinates.  For each camera with known
    // intrinsics we apply `undo_radial` (which removes only the
    // non-linear distortion part; the linear `K` is preserved on both
    // sides).  Cameras without intrinsics are passed through unchanged.
    // Then every camera has its image-centre principal point
    // `(w/2, h/2)` subtracted so that all subsequent projective
    // computations operate in a centred frame.
    let mut linear_ws = loaded_ws.clone();
    for i in 0..n_cams {
        if let Some((k, kc)) = &input.intrinsics[i] {
            for j in 0..n_frames {
                if id_mat[(i, j)] {
                    let pt = nalgebra::Vector3::new(
                        loaded_ws[(i * 3, j)],
                        loaded_ws[(i * 3 + 1, j)],
                        loaded_ws[(i * 3 + 2, j)],
                    );
                    let undistorted = utils::undo_radial(&pt, k, kc);
                    for r in 0..3 {
                        linear_ws[(i * 3 + r, j)] = undistorted[r];
                    }
                }
            }
        }
    }
    subtract_pp(&mut linear_ws, &pp, n_cams, n_frames);

    // Build per-camera intrinsic matrices for the known-intrinsics
    // Euclidean upgrade.  `k_full_per_cam[i]` is the provided pinhole
    // matrix (non-centred, as the user supplied it); `k_centred_per_cam[i]`
    // is the same matrix in MCSC's centred pixel frame (i.e. with
    // `(pp_x, pp_y)` subtracted from `(cx, cy)`).  Both are `None` when
    // the user did not provide intrinsics for that camera, or when
    // `use_known_intrinsics` is disabled.
    let mut k_full_per_cam: Vec<Option<Matrix3<f64>>> = vec![None; n_cams];
    let mut k_centred_per_cam: Vec<Option<Matrix3<f64>>> = vec![None; n_cams];
    if config.use_known_intrinsics {
        for i in 0..n_cams {
            if let Some((k, _kc)) = &input.intrinsics[i] {
                let mut k_c = *k;
                k_c[(0, 2)] -= pp[(i, 0)];
                k_c[(1, 2)] -= pp[(i, 1)];
                k_full_per_cam[i] = Some(*k);
                k_centred_per_cam[i] = Some(k_c);
            }
        }
    }

    // Set invisible entries to NaN (fill_mm expects NaN for missing data)
    for i in 0..n_cams {
        for j in 0..n_frames {
            if !id_mat[(i, j)] {
                linear_ws[(i * 3, j)] = f64::NAN;
                linear_ws[(i * 3 + 1, j)] = f64::NAN;
                linear_ws[(i * 3 + 2, j)] = f64::NAN;
            }
        }
    }

    // Build IdMat as f64 for easier manipulation
    let id_mat_work = DMatrix::<bool>::from_fn(n_cams, n_frames, |i, j| id_mat[(i, j)]);

    // RANSAC validation: find inliers
    tracing::info!("RANSAC validation step running with tolerance threshold: {inl_tol:.2} ...");
    let inliers_id_mat = ransac::find_inliers(&linear_ws, &id_mat_work, inl_tol);

    // Count inliers before packing
    let n_inliers_before_packing: usize = (0..n_frames)
        .filter(|&j| (0..n_cams).any(|i| inliers_id_mat[(i, j)]))
        .count();
    tracing::debug!("Before packing: {n_inliers_before_packing} points had at least 1 inlier");

    // Pack: keep only columns with enough cameras
    let packed_idx: Vec<usize> = (0..n_frames)
        .filter(|&j| {
            let vis_count: usize = (0..n_cams).filter(|&i| inliers_id_mat[(i, j)]).count();
            vis_count >= n_cams - num_cams_fill
        })
        .collect();

    if packed_idx.len() < 20 {
        eyre::bail!(
            "Only {} points survived RANSAC validation and packing: probably not enough for reliable selfcalibration",
            packed_idx.len()
        );
    }

    tracing::debug!(
        "{} points survived RANSAC validation and packing",
        packed_idx.len()
    );

    // Build camera structures
    // cam(i).idlin = indices where camera i has data in id_mat (original)
    // cam(i).idin = indices where camera i has data in inliers_id_mat
    let mut cam_idlin: Vec<Vec<usize>> = Vec::new();
    let mut cam_xgt: Vec<DMatrix<f64>> = Vec::new(); // linearized coords for visible points
    let mut cam_xdist: Vec<DMatrix<f64>> = Vec::new(); // original distorted coords

    for i in 0..n_cams {
        let idlin: Vec<usize> = (0..n_frames).filter(|&j| id_mat[(i, j)]).collect();

        // xgt: linearized coords + add back pp
        let n_vis = idlin.len();
        let mut xgt = DMatrix::<f64>::zeros(3, n_vis);
        let mut xdist = DMatrix::<f64>::zeros(3, n_vis);
        for (col, &j) in idlin.iter().enumerate() {
            for r in 0..3 {
                xgt[(r, col)] = linear_ws[(i * 3 + r, j)];
                xdist[(r, col)] = loaded_ws[(i * 3 + r, j)];
            }
            // Add back principal point to xgt
            xgt[(0, col)] += pp[(i, 0)];
            xgt[(1, col)] += pp[(i, 1)];
        }

        cam_idlin.push(idlin);
        cam_xgt.push(xgt);
        cam_xdist.push(xdist);
    }

    // Iterative outlier removal loop
    let mut inlier_id_mat = inliers_id_mat.clone();
    let mut inlier_idx = packed_idx.clone();
    let mut last_good_inlier_idx = packed_idx.clone();
    let mut _prev_mean_err = 9e9_f64;
    let mut pe: DMatrix<f64> = DMatrix::zeros(n_cams * 3, 4);
    let mut xe: DMatrix<f64> = DMatrix::zeros(4, n_frames);
    let mut cam_std2d_err = vec![0.0_f64; n_cams];
    let mut cam_mean2d_err = vec![0.0_f64; n_cams];
    let mut cam_err2d: Vec<Vec<f64>> = vec![vec![]; n_cams];
    let mut cam_visandrec: Vec<Vec<usize>> = vec![vec![]; n_cams];
    let mut cam_recandvis: Vec<Vec<usize>> = vec![vec![]; n_cams];

    let options = FillMmOptions {
        verbose: true,
        no_ba: true,
        iter: 5,
        detection_accuracy: 2.0,
        consistent_number: 9,
        consistent_number_min: 6,
        samples: 1000,
        create_nullspace_trial_coef: 10,
        create_nullspace_threshold: 0.01,
        tol: 1e-6,
    };

    loop {
        tracing::info!(
            "{} points/frames have survived validations so far",
            inlier_idx.len()
        );
        tracing::info!("Filling of missing points is running ...");

        // Extract sub-matrices for inlier columns
        let ws_inlier = extract_columns(&linear_ws, &inlier_idx);

        // Run fill_mm_bundle
        let fill_result = match fill_mm::fill_mm_bundle(
            &ws_inlier,
            &pp.columns(0, 2), // n_cams x 2
            &options,
        ) {
            Ok(r) => r,
            Err(e) => {
                tracing::debug!(
                    "[run_mcsc] fill_mm_bundle failed: {e} - breaking outlier loop, using last good inlier set ({} pts)",
                    last_good_inlier_idx.len()
                );
                inlier_idx = last_good_inlier_idx.clone();
                break;
            }
        };

        // Note: `fill_result.bad_cols` lists local indices in ws_inlier
        // whose 3-D reconstruction failed (`approx_matrix` rank check).
        // The corresponding columns of `fill_result.x` are zero.  We do
        // NOT remove these from `inlier_idx` here because `findoutl` uses
        // the full `inlier_id_mat` for bookkeeping; instead
        // `compute_reproj_error` skips rec indices with all-zero Xe
        // columns so the NaN from `0 / 0` does not poison per-camera
        // statistics.
        if !fill_result.bad_cols.is_empty() {
            tracing::debug!(
                "[run_mcsc] fill_mm produced {} unreconstructed points (kept in inlier set, skipped in reprerror)",
                fill_result.bad_cols.len()
            );
        }

        let p_mat = &fill_result.p;
        let x_mat = &fill_result.x;

        let (p_signed, rmat, lambda) = compute_rmat_lambda_signed(p_mat, x_mat, n_cams);

        // Euclidize
        let euc = match euclidize(
            &rmat,
            &lambda,
            &p_signed,
            x_mat,
            &pp,
            config.square_pix,
            &k_full_per_cam,
            &k_centred_per_cam,
        ) {
            Ok(e) => e,
            Err(err) => {
                tracing::debug!("[run_mcsc] euclidize failed: {err} - breaking outlier loop");
                inlier_idx = last_good_inlier_idx.clone();
                break;
            }
        };
        pe = euc.pe;
        xe = euc.xe;
        let _ce = euc.c;

        tracing::info!("************************************************************");

        // Compute reprojection errors
        compute_reproj_error(
            &cam_idlin,
            &cam_xgt,
            &pe,
            &xe,
            n_cams,
            n_frames,
            &inlier_idx,
            &mut cam_err2d,
            &mut cam_mean2d_err,
            &mut cam_std2d_err,
            &mut cam_visandrec,
            &mut cam_recandvis,
        );

        // Find outliers
        let (n_outliers, new_inlier_id_mat, new_inlier_idx) = find_outliers(
            &cam_err2d,
            &cam_mean2d_err,
            &cam_std2d_err,
            &cam_idlin,
            &cam_visandrec,
            &inlier_id_mat,
            inl_tol,
            num_cams_fill,
            n_cams,
        );

        tracing::info!("Number of detected outliers: {n_outliers}");

        inlier_id_mat = new_inlier_id_mat;
        inlier_idx = new_inlier_idx;
        tracing::info!("About cameras (Id, 2D reprojection error, #inliers):");
        print_cam_stats(&cam_std2d_err, &cam_mean2d_err, &inlier_id_mat, n_cams);

        let current_mean_err: f64 = cam_mean2d_err.iter().sum::<f64>() / n_cams as f64;

        last_good_inlier_idx = inlier_idx.clone();
        if n_outliers == 0 {
            tracing::debug!("[run_mcsc] converged after outlier removal");
            break;
        }

        _prev_mean_err = current_mean_err;
    }

    // Final bundle adjustment if requested.
    //
    // NOTE on known-K mode: `euclidize` with `use_known_intrinsics`
    // produces a Euclidean reconstruction whose reprojection error
    // is typically several pixels on real data — higher than
    // self-calibration, which can absorb residual modelling error
    // into K.  Closing that gap requires a non-linear refinement of
    // (R, t, X) with K held fixed (bundle adjustment / iterated PnP).
    // See `scratch/MCSC-vs-PnP.md` for the rationale.  The downstream
    // `braidz-mcsc` crate has a BA pass for this purpose (currently
    // gated behind a TODO for unrelated reasons); when that is
    // unblocked, using it with `BAIntrinsicsSource::CheckerboardCal`
    // and `use_known_intrinsics = true` gives the full pipeline.

    if config.do_bundle_adjustment {
        tracing::debug!("Refinement by using Bundle Adjustment");
        let ws_inlier = extract_columns(&linear_ws, &inlier_idx);
        match fill_mm::fill_mm_bundle(
            &ws_inlier,
            &pp.columns(0, 2),
            &FillMmOptions {
                no_ba: false,
                ..options
            },
        ) {
            Err(e) => tracing::debug!("[run_mcsc] BA fill_mm_bundle failed: {e} - skipping BA"),
            Ok(fill_result) => {
                let (p_signed, rmat, lambda) =
                    compute_rmat_lambda_signed(&fill_result.p, &fill_result.x, n_cams);
                match euclidize(
                    &rmat,
                    &lambda,
                    &p_signed,
                    &fill_result.x,
                    &pp,
                    config.square_pix,
                    &k_full_per_cam,
                    &k_centred_per_cam,
                ) {
                    Err(e) => tracing::debug!("[run_mcsc] BA euclidize failed: {e} - skipping BA"),
                    Ok(euc) => {
                        pe = euc.pe;
                        xe = euc.xe;
                        compute_reproj_error(
                            &cam_idlin,
                            &cam_xgt,
                            &pe,
                            &xe,
                            n_cams,
                            n_frames,
                            &inlier_idx,
                            &mut cam_err2d,
                            &mut cam_mean2d_err,
                            &mut cam_std2d_err,
                            &mut cam_visandrec,
                            &mut cam_recandvis,
                        );
                    }
                }
            }
        }
    }

    // Build results
    let mut projection_matrices = Vec::new();
    let mut camera_centers = Vec::new();
    let mut rotations = Vec::new();
    let mut intrinsics_vec = Vec::new();
    let mut points4cal = Vec::new();

    for i in 0..n_cams {
        let p_i = pe.rows(i * 3, 3);
        let pmat = Matrix3x4::from_fn(|r, c| p_i[(r, c)]);
        projection_matrices.push(pmat);

        // Decompose.  For cameras with known intrinsics the Euclidean
        // upgrade already built `Pe = K_full * [R | -R C]` directly
        // (see `euclidize`), so we skip RQ entirely and report the
        // supplied `K` verbatim, recovering `(R, C)` by solving
        // `K^{-1} * Pe = [R | -R C]`.  For self-calibrated cameras we
        // fall back to the canonical RQ decomposition (K with strictly
        // positive diagonal, R a proper rotation).
        let (k, r, c) = if let Some(k_full) = &k_full_per_cam[i] {
            let k_inv = k_full.try_inverse().ok_or_else(|| {
                eyre::eyre!("Known intrinsics matrix for camera {i} is singular")
            })?;
            let m = Matrix3::from_fn(|r, c| p_i[(r, c)]);
            let t_col = nalgebra::Vector3::new(p_i[(0, 3)], p_i[(1, 3)], p_i[(2, 3)]);
            let r_mat = k_inv * m;
            let t_vec = k_inv * t_col;
            let c_vec = -r_mat.transpose() * t_vec;
            (*k_full, r_mat, c_vec)
        } else {
            let m = Matrix3::from_fn(|r, c| p_i[(r, c)]);
            let (k, r) = utils::rq_decomposition(&m);
            let tvec = k.try_inverse().unwrap()
                * nalgebra::Vector3::new(p_i[(0, 3)], p_i[(1, 3)], p_i[(2, 3)]);
            let c = -r.transpose() * tvec;
            (k, r, c)
        };

        camera_centers.push(c);
        rotations.push(r);
        intrinsics_vec.push(k);

        // Build points4cal: [Xe', xe'] for visible and reconstructed points
        let n_corr = cam_visandrec[i].len();
        if n_corr > 0 {
            let mut corresp = DMatrix::<f64>::zeros(n_corr, 7);
            for (idx, (&vis_idx, &rec_idx)) in cam_visandrec[i]
                .iter()
                .zip(cam_recandvis[i].iter())
                .enumerate()
            {
                // World point (homogeneous)
                for r in 0..4 {
                    corresp[(idx, r)] = xe[(r, rec_idx)];
                }
                // Distorted pixel coordinates
                for r in 0..3 {
                    corresp[(idx, 4 + r)] = cam_xdist[i][(r, vis_idx)];
                }
            }
            points4cal.push(corresp);
        } else {
            points4cal.push(DMatrix::<f64>::zeros(0, 7));
        }
    }

    // Print final reprojection error
    let all_err: Vec<f64> = cam_err2d.iter().flat_map(|v| v.iter().copied()).collect();
    if all_err.is_empty() {
        eyre::bail!("No inliers. Cannot compute reprojection distance.");
    }
    let mean_reproj_distance = all_err.iter().sum::<f64>() / all_err.len() as f64;
    let std_reproj_distance = {
        let var = all_err
            .iter()
            .map(|e| (e - mean_reproj_distance).powi(2))
            .sum::<f64>()
            / (all_err.len() - 1) as f64;
        var.sqrt()
    };
    tracing::info!(
        "2D reprojection error: mean {mean_reproj_distance:.2} pixels, std {std_reproj_distance:.2}"
    );

    Ok(McscResult {
        mean_reproj_distance,
        std_reproj_distance,
        projection_matrices,
        camera_centers,
        rotations,
        intrinsics: intrinsics_vec,
        points_3d: xe,
        inlier_indices: inlier_idx,
        points4cal,
    })
}

/// Compute Rmat, Lambda from P*X and normalize per-camera sign ambiguity.
///
/// The projective reconstruction from `fill_mm` has a per-camera sign
/// ambiguity: each camera's 3-row block of P can be independently negated
/// without changing the projective reconstruction. The `euclidize` function
/// is sensitive to these signs (via Lambda sums in the `a`, `b`, `c`
/// computation), so we normalize them here: for each camera, if more than
/// half of its Lambda (projective depth) values are negative, the camera's
/// P rows, Rmat rows, and Lambda row are all negated to ensure consistent
/// positive depths.
fn compute_rmat_lambda_signed(
    p: &DMatrix<f64>,
    x: &DMatrix<f64>,
    n_cams: usize,
) -> (DMatrix<f64>, DMatrix<f64>, DMatrix<f64>) {
    let mut rmat = p * x;
    let n_pts = x.ncols();

    let mut lambda = DMatrix::<f64>::zeros(n_cams, n_pts);
    for i in 0..n_cams {
        for j in 0..n_pts {
            lambda[(i, j)] = rmat[(i * 3 + 2, j)];
        }
    }

    let mut p_signed = p.clone();
    for i in 0..n_cams {
        // Ignore columns whose projective depth is exactly zero; those
        // correspond to miss_col points (no 3-D reconstruction) and
        // would otherwise bias the sign decision toward "mostly zero"
        // cameras.
        let nonzero: Vec<f64> = (0..n_pts)
            .map(|j| lambda[(i, j)])
            .filter(|&v| v != 0.0)
            .collect();
        let total = nonzero.len().max(1);
        let neg_count = nonzero.iter().filter(|&&v| v < 0.0).count();
        if neg_count > total / 2 {
            for r in 0..3 {
                for c in 0..4 {
                    p_signed[(i * 3 + r, c)] = -p_signed[(i * 3 + r, c)];
                }
                for j in 0..n_pts {
                    rmat[(i * 3 + r, j)] = -rmat[(i * 3 + r, j)];
                }
            }
            for j in 0..n_pts {
                lambda[(i, j)] = -lambda[(i, j)];
            }
        }
    }

    (p_signed, rmat, lambda)
}

/// Subtract principal points from the measurement matrix.
fn subtract_pp(ws: &mut DMatrix<f64>, pp: &DMatrix<f64>, n_cams: usize, n_frames: usize) {
    // pp_vec is the n_cams*3 x 1 vector formed by reshaping pp' (transpose)
    for j in 0..n_frames {
        for i in 0..n_cams {
            ws[(i * 3, j)] -= pp[(i, 0)];
            ws[(i * 3 + 1, j)] -= pp[(i, 1)];
            ws[(i * 3 + 2, j)] -= pp[(i, 2)];
        }
    }
}

/// Extract columns from a matrix by index.
fn extract_columns(m: &DMatrix<f64>, indices: &[usize]) -> DMatrix<f64> {
    let nrows = m.nrows();
    let ncols = indices.len();
    let mut result = DMatrix::zeros(nrows, ncols);
    for (new_col, &old_col) in indices.iter().enumerate() {
        for r in 0..nrows {
            result[(r, new_col)] = m[(r, old_col)];
        }
    }
    result
}

struct EuclidizeResult {
    pe: DMatrix<f64>,
    xe: DMatrix<f64>,
    c: DMatrix<f64>,
    #[allow(dead_code)]
    rot: DMatrix<f64>,
}

/// Perform Euclidean reconstruction from a projective reconstruction.
///
/// Equivalent to `CoreFunctions/euclidize.m`, augmented with an
/// optional known-intrinsics path.
///
/// # Algorithm
///
/// 1. **Plane at infinity (B)**: estimated from the constraint that the
///    principal-point-centred image coordinates have zero mean under the
///    projective depths Λ.
/// 2. **Absolute quadric (Q)**: estimated from linear constraints on the
///    dual image of the absolute conic,
///    `ω*_j = P_j · Q · P_j^T = s_j · K_c_j · K_c_j^T`.
///     - For cameras **without** supplied intrinsics (self-calibration
///       mode), `K_c_j` is unknown and only the structural constraints
///       skew=0, and optionally square pixels, plus `ω*(0,2)=ω*(1,2)=0`
///       (principal point at the image centre in the centred frame) are
///       imposed — 3 or 4 rows per camera.
///     - For cameras **with** supplied intrinsics (known-K mode), `K_c_j`
///       is the caller-provided centred intrinsic matrix; we pre-multiply
///       `P_j` by `K_c_j^{-1}` and impose the 5 independent linear
///       constraints that `K_c_j^{-1} · ω*_j · K_c_j^{-T} ∝ I_3` (three
///       off-diagonals vanish, and two diagonal equalities). This
///       removes all per-camera intrinsic freedom, which is the fix for
///       the "MCSC produces skew" issue observed when checkerboard
///       intrinsics are available.
/// 3. **Euclidean upgrade**: `H = [A, B]` where `A = U√S` from the SVD of
///    Q. Then `Pe_dyn = P*H` and `Xe = H^{-1}*X` are in a Euclidean frame
///    tied to the *centred* pixel convention.
/// 4. **Per-camera decomposition**: for each camera we need to produce a
///    `Pe_rt` block such that `nhom(Pe_rt · Xe)` matches **full**
///    (non-centred) pixel coordinates, because downstream reprojection
///    comparisons use the non-centred observations `cam_xgt`.
///     - **Self-calibrated cameras** use the canonical RQ path: the
///       3×4 Pe_dyn block is first sign-normalised so the 3×3
///       sub-block has positive determinant, then
///       `rq_decomposition` returns `K` with strictly positive
///       diagonal and `R` a proper rotation.  Pe_rt is then
///       `(K + pp_shift) * [R | -R·C]` — the `+= pp` on `K`'s
///       principal-point entries exactly cancels the centring that
///       was applied to the input measurement matrix.
///     - **Known-K cameras** bypass RQ entirely. We solve
///       `M = K_c^{-1} · Pe_dyn` for `[s·R | s·t]`, normalise `s` so
///       that the 3×3 block is a rotation, project to SO(3), and then
///       build `Pe_rt = K_full · [R | -R·C]` directly with the
///       caller-supplied non-centred `K_full`. This guarantees exact
///       reprojection to full pixels and zero residual intrinsic drift.
///
/// # Parameters
///
/// * `k_full_per_cam[i]` — non-centred `K` for camera `i`, or `None` for
///   self-calibration. When provided, it is the value the caller wants
///   to appear in the final `Pe_rt`.
/// * `k_centred_per_cam[i]` — the same `K` with `(pp_x, pp_y)` subtracted
///   from `(cx, cy)`, to match the centred frame that P and X live in.
///   Must be `Some` iff `k_full_per_cam[i]` is.
#[allow(clippy::too_many_arguments)]
fn euclidize(
    _rmat: &DMatrix<f64>,
    lambda: &DMatrix<f64>,
    p: &DMatrix<f64>,
    x: &DMatrix<f64>,
    pp: &DMatrix<f64>,
    square_pix: bool,
    k_full_per_cam: &[Option<Matrix3<f64>>],
    k_centred_per_cam: &[Option<Matrix3<f64>>],
) -> Result<EuclidizeResult> {
    let n = lambda.nrows(); // number of cameras
    let m = lambda.ncols(); // number of points

    // The Ws matrix needed here is the linearized measurement matrix
    // We reconstruct it from P*X: Ws = Rmat / Lambda (row-by-row normalization)
    // Actually, we need the original centered Ws. We can get it from Rmat:
    // Rmat = Lambda .* Ws, so Ws(i,:) = Rmat(i,:) / Lambda(cam,:)
    // But the euclidize function uses Ws in a specific way to compute B.
    // Let's look more carefully: it uses sum(Ws(3i-2,:).*Lambda(i,:)) etc.
    // which equals sum(Rmat(3i-2,:)).
    // So we can compute a,b,c directly from Rmat.
    let rmat = p * x;

    // Compute B (the plane at infinity)
    let mut a = DVector::<f64>::zeros(n);
    let mut b = DVector::<f64>::zeros(n);
    let mut c = DVector::<f64>::zeros(n);

    for i in 0..n {
        let mut sum_a = 0.0;
        let mut sum_b = 0.0;
        let mut sum_c = 0.0;
        for j in 0..m {
            // a(i) = sum(Ws(3i-2,:) .* Lambda(i,:)) where Ws = Rmat
            sum_a += rmat[(i * 3, j)] * lambda[(i, j)];
            sum_b += rmat[(i * 3 + 1, j)] * lambda[(i, j)];
            sum_c += lambda[(i, j)];
        }
        a[i] = sum_a;
        b[i] = sum_b;
        c[i] = sum_c;
    }

    // Build Temp matrix for B
    let mut temp_a = DMatrix::<f64>::zeros(n, 4);
    let mut temp_b = DMatrix::<f64>::zeros(n, 4);
    for i in 0..n {
        for col in 0..4 {
            temp_a[(i, col)] = -p[(i * 3 + 2, col)] * a[i] / c[i] + p[(i * 3, col)];
            temp_b[(i, col)] = -p[(i * 3 + 2, col)] * b[i] / c[i] + p[(i * 3 + 1, col)];
        }
    }

    let mut temp = DMatrix::<f64>::zeros(2 * n, 4);
    for i in 0..n {
        for col in 0..4 {
            temp[(i, col)] = temp_a[(i, col)];
            temp[(n + i, col)] = temp_b[(i, col)];
        }
    }

    let svd_b = SVD::new(temp, true, true);
    let v_b = svd_b.v_t.unwrap().transpose();
    let b_vec = v_b.column(3).clone_owned(); // last column

    // Compute A (the absolute quadric Q).
    //
    // For each camera we add linear rows in the 10 unknowns of the
    // symmetric 4×4 Q.  Self-calibrated cameras contribute 3–4
    // structural rows; known-K cameras contribute 5 rows by imposing
    // K_c^{-1} ω* K_c^{-T} ∝ I_3 on rows of `P' = K_c^{-1} P`.
    let mut rows_temp = Vec::new();

    for i in 0..n {
        if let Some(k_c) = &k_centred_per_cam[i] {
            // Known-intrinsics camera: pre-normalise P by K_c^{-1} so
            // that the constraints become orthonormality on the rows
            // of the resulting P'.
            let k_inv = k_c
                .try_inverse()
                .ok_or_else(|| eyre::eyre!("K_centred for camera {i} is singular"))?;
            let p_block = Matrix3x4::from_fn(|r, c| p[(i * 3 + r, c)]);
            let p_prime = k_inv * p_block; // 3x4

            // Rows of P' as [f64; 4] slices.
            let r0 = [p_prime[(0, 0)], p_prime[(0, 1)], p_prime[(0, 2)], p_prime[(0, 3)]];
            let r1 = [p_prime[(1, 0)], p_prime[(1, 1)], p_prime[(1, 2)], p_prime[(1, 3)]];
            let r2 = [p_prime[(2, 0)], p_prime[(2, 1)], p_prime[(2, 2)], p_prime[(2, 3)]];

            // Off-diagonal: (ω*)_{01}=0, (ω*)_{02}=0, (ω*)_{12}=0
            rows_temp.push(q_row(&r0, &r1));
            rows_temp.push(q_row(&r0, &r2));
            rows_temp.push(q_row(&r1, &r2));

            // Diagonal equalities: (ω*)_{00}=(ω*)_{11}, (ω*)_{00}=(ω*)_{22}
            let d00 = q_row(&r0, &r0);
            let d11 = q_row(&r1, &r1);
            let d22 = q_row(&r2, &r2);
            let mut row_01 = [0.0_f64; 10];
            let mut row_02 = [0.0_f64; 10];
            for k in 0..10 {
                row_01[k] = d00[k] - d11[k];
                row_02[k] = d00[k] - d22[k];
            }
            rows_temp.push(row_01);
            rows_temp.push(row_02);
        } else {
            // Self-calibration path (original MCSC formulation).
            let p1 = p.row(i * 3).clone_owned();
            let p2 = p.row(i * 3 + 1).clone_owned();
            let p3 = p.row(i * 3 + 2).clone_owned();

            // P1^T * Q * P2 = 0 (skew = 0)
            let (u, v) = (&p1, &p2);
            rows_temp.push(q_row(u.as_slice(), v.as_slice()));

            // |P1|^2 = |P2|^2 (square pixels)
            if square_pix {
                let mut row = [0.0_f64; 10];
                let q1 = q_row(u.as_slice(), u.as_slice());
                let q2 = q_row(v.as_slice(), v.as_slice());
                for k in 0..10 {
                    row[k] = q1[k] - q2[k];
                }
                rows_temp.push(row);
            }

            // P1^T * Q * P3 = 0   (implies centred cx ≈ 0)
            let (u, v) = (&p1, &p3);
            rows_temp.push(q_row(u.as_slice(), v.as_slice()));

            // P2^T * Q * P3 = 0   (implies centred cy ≈ 0)
            let (u, v) = (&p2, &p3);
            rows_temp.push(q_row(u.as_slice(), v.as_slice()));
        }
    }

    let n_rows = rows_temp.len();
    let mut temp_q = DMatrix::<f64>::zeros(n_rows, 10);
    for (i, row) in rows_temp.iter().enumerate() {
        for (j, &val) in row.iter().enumerate() {
            temp_q[(i, j)] = val;
        }
    }

    let svd_q = SVD::new(temp_q, false, true);
    let v_q = svd_q.v_t.unwrap().transpose();
    let q_vec = v_q.column(v_q.ncols() - 1).clone_owned();

    // Reconstruct symmetric 4x4 matrix Q
    let mut q_mat = nalgebra::Matrix4::<f64>::zeros();
    q_mat[(0, 0)] = q_vec[0];
    q_mat[(0, 1)] = q_vec[1];
    q_mat[(1, 0)] = q_vec[1];
    q_mat[(0, 2)] = q_vec[2];
    q_mat[(2, 0)] = q_vec[2];
    q_mat[(0, 3)] = q_vec[3];
    q_mat[(3, 0)] = q_vec[3];
    q_mat[(1, 1)] = q_vec[4];
    q_mat[(1, 2)] = q_vec[5];
    q_mat[(2, 1)] = q_vec[5];
    q_mat[(1, 3)] = q_vec[6];
    q_mat[(3, 1)] = q_vec[6];
    q_mat[(2, 2)] = q_vec[7];
    q_mat[(2, 3)] = q_vec[8];
    q_mat[(3, 2)] = q_vec[8];
    q_mat[(3, 3)] = q_vec[9];

    // Check sign: M*M^T should have positive diagonal
    // P_block is 3x4 (first camera)
    let p_block = nalgebra::Matrix3x4::from_fn(|r, c| p[(r, c)]);
    let m_mt = p_block * q_mat * p_block.transpose();
    if m_mt[(0, 0)] <= 0.0 {
        // q_vec = -q_vec; // not needed since we only use q_mat below
        q_mat = -q_mat;
    }

    // SVD of Q to get A
    let svd_qq = SVD::new(q_mat, true, true);
    let u_qq = svd_qq.u.unwrap();
    let s_qq = svd_qq.singular_values;
    // A = U(:,1:3) * sqrt(S(1:3,1:3))
    let mut a_mat = nalgebra::Matrix4x3::<f64>::zeros();
    for r in 0..4 {
        for c in 0..3 {
            a_mat[(r, c)] = u_qq[(r, c)] * s_qq[c].sqrt();
        }
    }

    // H = [A, B]
    let mut h_mat = nalgebra::Matrix4::<f64>::zeros();
    for r in 0..4 {
        for c in 0..3 {
            h_mat[(r, c)] = a_mat[(r, c)];
        }
        h_mat[(r, 3)] = b_vec[r];
    }

    let h_inv = h_mat
        .try_inverse()
        .ok_or_else(|| eyre::eyre!("H matrix is singular in euclidize"))?;

    // Euclidean motion and shape
    let mut pe_dyn = p * &DMatrix::from_fn(4, 4, |r, c| h_mat[(r, c)]);
    let mut xe_dyn = DMatrix::from_fn(4, 4, |r, c| h_inv[(r, c)]) * x;

    // Normalize Xe
    for j in 0..xe_dyn.ncols() {
        let w = xe_dyn[(3, j)];
        if w.abs() > 1e-15 {
            for r in 0..4 {
                xe_dyn[(r, j)] /= w;
            }
        }
    }

    // Decompose each camera's projection matrix
    let mut pe_rt = DMatrix::<f64>::zeros(n * 3, 4);
    let mut rot_all = DMatrix::<f64>::zeros(n * 3, 3);
    let mut c_st = DMatrix::<f64>::zeros(n, 3);

    for i in 0..n {
        // Normalize by scale
        let p3_row = nalgebra::RowVector3::new(
            pe_dyn[(i * 3 + 2, 0)],
            pe_dyn[(i * 3 + 2, 1)],
            pe_dyn[(i * 3 + 2, 2)],
        );
        let sc = p3_row.norm();

        for r in 0..3 {
            for c in 0..4 {
                pe_dyn[(i * 3 + r, c)] /= sc;
            }
        }

        // Check for points behind the camera
        let mut behind = 0;
        for j in 0..xe_dyn.ncols() {
            let val = pe_dyn[(i * 3 + 2, 0)] * xe_dyn[(0, j)]
                + pe_dyn[(i * 3 + 2, 1)] * xe_dyn[(1, j)]
                + pe_dyn[(i * 3 + 2, 2)] * xe_dyn[(2, j)]
                + pe_dyn[(i * 3 + 2, 3)] * xe_dyn[(3, j)];
            if val < 0.0 {
                behind += 1;
            }
        }
        if behind > 0 {
            for r in 0..3 {
                for c in 0..4 {
                    pe_dyn[(i * 3 + r, c)] = -pe_dyn[(i * 3 + r, c)];
                }
            }
        }

        // Canonicalise the sign of the whole 3×4 Pe_dyn block so that
        // its 3×3 left sub-block has positive determinant.  Since
        // `rq_decomposition` assumes det > 0 to return a proper
        // rotation `R` together with `K * R = input`, we must make
        // the input consistent with that contract; otherwise
        // `t_vec = K^{-1} * Pe_dyn[:,3]` would silently disagree with
        // the `(K, R)` factor pair.  `Pe` and `-Pe` represent the
        // same projective camera, so flipping the whole block is
        // harmless; we just need to also re-check the "points behind
        // camera" condition that may now have flipped (it will not,
        // since negating the block flips both the row-3 sign and the
        // depth-sign together — see the analysis in the sign-
        // convention tests).
        let det_left = {
            let a = pe_dyn[(i * 3, 0)];
            let b = pe_dyn[(i * 3, 1)];
            let c = pe_dyn[(i * 3, 2)];
            let d = pe_dyn[(i * 3 + 1, 0)];
            let e = pe_dyn[(i * 3 + 1, 1)];
            let f = pe_dyn[(i * 3 + 1, 2)];
            let g = pe_dyn[(i * 3 + 2, 0)];
            let h = pe_dyn[(i * 3 + 2, 1)];
            let k = pe_dyn[(i * 3 + 2, 2)];
            a * (e * k - f * h) - b * (d * k - f * g) + c * (d * h - e * g)
        };
        if det_left < 0.0 {
            for r in 0..3 {
                for c in 0..4 {
                    pe_dyn[(i * 3 + r, c)] = -pe_dyn[(i * 3 + r, c)];
                }
            }
        }

        let (r_rot, cc) = if let (Some(k_full), Some(k_c)) =
            (&k_full_per_cam[i], &k_centred_per_cam[i])
        {
            // Known-intrinsics path.  Pe_dyn_block should satisfy
            //   Pe_dyn_block ≈ s * K_c * [R | t],   t = -R·C,
            // so M = K_c^{-1} * Pe_dyn_block ≈ s * [R | t].
            // Extract R by SVD-projecting the 3×3 left block to SO(3),
            // extract s as its average singular value, then recover t.
            let k_c_inv = k_c.try_inverse().ok_or_else(|| {
                eyre::eyre!("K_centred for camera {i} is singular in decomposition")
            })?;
            let m33 = Matrix3::from_fn(|r, c| pe_dyn[(i * 3 + r, c)]);
            let t_col = nalgebra::Vector3::new(
                pe_dyn[(i * 3, 3)],
                pe_dyn[(i * 3 + 1, 3)],
                pe_dyn[(i * 3 + 2, 3)],
            );
            let m_left = k_c_inv * m33;
            let m_t = k_c_inv * t_col;

            // SVD-project m_left to SO(3).
            let svd = nalgebra::SVD::new(m_left, true, true);
            let u = svd.u.unwrap();
            let v_t = svd.v_t.unwrap();
            let mut r_mat = u * v_t;
            if r_mat.determinant() < 0.0 {
                // Flip sign of last column of U to force det(R) = +1.
                let mut u_fixed = u;
                for r in 0..3 {
                    u_fixed[(r, 2)] = -u_fixed[(r, 2)];
                }
                r_mat = u_fixed * v_t;
            }
            let s_sum: f64 = svd.singular_values.iter().sum();
            let s_avg = s_sum / 3.0;
            if s_avg.abs() < 1e-12 {
                eyre::bail!(
                    "Known-K decomposition produced near-zero scale for camera {i}"
                );
            }
            let t_vec = m_t / s_avg;
            let c_vec = -r_mat.transpose() * t_vec;

            // Pe_rt = K_full * [R | -R·C].  Writing directly to pe_rt.
            let r_cc = r_mat * c_vec;
            for r in 0..3 {
                for c in 0..3 {
                    let mut val = 0.0;
                    for kk in 0..3 {
                        val += k_full[(r, kk)] * r_mat[(kk, c)];
                    }
                    pe_rt[(i * 3 + r, c)] = val;
                }
                let mut val = 0.0;
                for kk in 0..3 {
                    val += k_full[(r, kk)] * (-r_cc[kk]);
                }
                pe_rt[(i * 3 + r, 3)] = val;
            }

            (r_mat, c_vec)
        } else {
            // Self-calibration path: unchanged RQ decomposition plus the
            // `k + pp_shift` step converts the centred-frame K that
            // RQ recovers into the full-pixel K needed by `Pe_rt`.
            let m33 = Matrix3::from_fn(|r, c| pe_dyn[(i * 3 + r, c)]);
            let (k, r_rot) = utils::rq_decomposition(&m33);

            let k_inv = k
                .try_inverse()
                .ok_or_else(|| eyre::eyre!("K matrix is singular for camera {i}"))?;

            let t_vec = k_inv
                * nalgebra::Vector3::new(
                    pe_dyn[(i * 3, 3)],
                    pe_dyn[(i * 3 + 1, 3)],
                    pe_dyn[(i * 3 + 2, 3)],
                );
            let cc = -r_rot.transpose() * t_vec;

            // With canonical RQ (K positive diagonal, R proper
            // rotation), Pe_dyn = K_c * [R | -R·C] in the centred
            // pixel frame, where K_c has `cx ≈ 0, cy ≈ 0`. To project
            // to non-centred (full) pixels we need
            // `Pe_rt = (K_c + pp_shift) * [R | -R·C]`.
            let mut k_mod = k;
            k_mod[(0, 2)] += pp[(i, 0)];
            k_mod[(1, 2)] += pp[(i, 1)];

            let r_cc = r_rot * cc;
            for r in 0..3 {
                for c in 0..3 {
                    let mut val = 0.0;
                    for kk in 0..3 {
                        val += k_mod[(r, kk)] * r_rot[(kk, c)];
                    }
                    pe_rt[(i * 3 + r, c)] = val;
                }
                let mut val = 0.0;
                for kk in 0..3 {
                    val += k_mod[(r, kk)] * (-r_cc[kk]);
                }
                pe_rt[(i * 3 + r, 3)] = val;
            }

            (r_rot, cc)
        };

        for r in 0..3 {
            for c in 0..3 {
                rot_all[(i * 3 + r, c)] = r_rot[(r, c)];
            }
        }

        for c in 0..3 {
            c_st[(i, c)] = cc[c];
        }
    }

    Ok(EuclidizeResult {
        pe: pe_rt,
        xe: xe_dyn,
        c: c_st,
        rot: rot_all,
    })
}


/// Build a row for the quadric constraint: u^T Q v expanded into 10 unknowns.
fn q_row(u: &[f64], v: &[f64]) -> [f64; 10] {
    [
        u[0] * v[0],
        u[0] * v[1] + u[1] * v[0],
        u[2] * v[0] + u[0] * v[2],
        u[0] * v[3] + u[3] * v[0],
        u[1] * v[1],
        u[1] * v[2] + u[2] * v[1],
        u[1] * v[3] + u[3] * v[1],
        u[2] * v[2],
        u[3] * v[2] + u[2] * v[3],
        u[3] * v[3],
    ]
}

/// Compute per-camera 2-D reprojection errors.
///
/// Equivalent to `CoreFunctions/reprerror.m`. Compares `nhom(Pe * Xe)`
/// (full pixel coordinates) against `cam_xgt` (original non-centred
/// pixel observations).
#[allow(clippy::too_many_arguments)]
fn compute_reproj_error(
    cam_idlin: &[Vec<usize>],
    cam_xgt: &[DMatrix<f64>],
    pe: &DMatrix<f64>,
    xe: &DMatrix<f64>,
    n_cams: usize,
    n_frames: usize,
    inlier_idx: &[usize],
    cam_err2d: &mut [Vec<f64>],
    cam_mean2d_err: &mut [f64],
    cam_std2d_err: &mut [f64],
    cam_visandrec: &mut [Vec<usize>],
    cam_recandvis: &mut [Vec<usize>],
) {
    for i in 0..n_cams {
        // Project all points through camera i
        // xe_proj = Pe(3i-2:3i, :) * Xe
        let p_i = pe.rows(i * 3, 3);
        let projected = p_i * xe;

        // Build masks
        // mask.rec: which original frames survived as inliers
        let mut mask_rec = vec![false; n_frames];
        for &idx in inlier_idx {
            mask_rec[idx] = true;
        }
        // mask.vis: which original frames are visible for camera i
        let mut mask_vis = vec![false; n_frames];
        for &idx in &cam_idlin[i] {
            mask_vis[idx] = true;
        }
        // mask.both = vis & rec
        let mask_both: Vec<bool> = mask_rec
            .iter()
            .zip(mask_vis.iter())
            .map(|(&r, &v)| r && v)
            .collect();

        // Compute cumulative sums for unmask
        let mut unmask_rec = vec![0usize; n_frames];
        let mut unmask_vis = vec![0usize; n_frames];
        let mut cum_rec = 0usize;
        let mut cum_vis = 0usize;
        for j in 0..n_frames {
            if mask_rec[j] {
                cum_rec += 1;
            }
            if mask_vis[j] {
                cum_vis += 1;
            }
            unmask_rec[j] = cum_rec;
            unmask_vis[j] = cum_vis;
        }

        // recandvis and visandrec: indices of points that are both visible AND reconstructed
        let mut recandvis_i = Vec::new();
        let mut visandrec_i = Vec::new();

        for j in 0..n_frames {
            let both_match = !(mask_rec[j] ^ mask_both[j]) && mask_rec[j];
            if both_match {
                let rec_idx = unmask_rec[j] - 1;
                let vis_idx = unmask_vis[j] - 1;
                // Skip points that fill_mm could not reconstruct (Xe
                // column is all zero).  These correspond to Octave's
                // `u2` output of fill_mm - points seen in too few
                // cameras or failing the rank check in approx_matrix.
                // Without this guard the projection `P*Xe[:,j]` has
                // w=0 and divide-by-zero NaNs poison the per-camera
                // mean/std reprojection errors.
                if rec_idx < xe.ncols() {
                    let all_zero = (0..xe.nrows()).all(|r| xe[(r, rec_idx)] == 0.0);
                    if all_zero {
                        continue;
                    }
                }
                recandvis_i.push(rec_idx);
                visandrec_i.push(vis_idx);
            }
        }

        // Compute 2D errors
        let mut errors = Vec::new();
        let mut n_bad_w = 0usize;
        let mut n_bad_err = 0usize;
        for (&rec_idx, &vis_idx) in recandvis_i.iter().zip(visandrec_i.iter()) {
            if rec_idx < projected.ncols() && vis_idx < cam_xgt[i].ncols() {
                // Normalize projected point
                let w = projected[(2, rec_idx)];
                let px = projected[(0, rec_idx)] / w;
                let py = projected[(1, rec_idx)] / w;

                let gx = cam_xgt[i][(0, vis_idx)];
                let gy = cam_xgt[i][(1, vis_idx)];

                let dx = px - gx;
                let dy = py - gy;
                let err = (dx * dx + dy * dy).sqrt();
                if !w.is_finite() || w == 0.0 {
                    n_bad_w += 1;
                }
                if !err.is_finite() {
                    n_bad_err += 1;
                }
                errors.push(err);
            }
        }
        if n_bad_w > 0 || n_bad_err > 0 {
            // Find the offending point(s)
            let mut offenders: Vec<(usize, f64, [f64; 4])> = Vec::new();
            for (&rec_idx, _) in recandvis_i.iter().zip(visandrec_i.iter()) {
                let w = projected[(2, rec_idx)];
                if !w.is_finite() || w == 0.0 {
                    offenders.push((
                        rec_idx,
                        w,
                        [
                            xe[(0, rec_idx)],
                            xe[(1, rec_idx)],
                            xe[(2, rec_idx)],
                            xe[(3, rec_idx)],
                        ],
                    ));
                }
            }
            tracing::debug!(
                "  [reprerror cam {}] bad_w={} bad_err={} (n_pairs={}) offenders={:?}",
                i + 1,
                n_bad_w,
                n_bad_err,
                errors.len(),
                offenders
            );
        }

        let mean_err = if errors.is_empty() {
            0.0
        } else {
            errors.iter().sum::<f64>() / errors.len() as f64
        };
        let std_err = if errors.len() <= 1 {
            0.0
        } else {
            let var = errors.iter().map(|e| (e - mean_err).powi(2)).sum::<f64>()
                / (errors.len() - 1) as f64;
            var.sqrt()
        };

        cam_err2d[i] = errors;
        cam_mean2d_err[i] = mean_err;
        cam_std2d_err[i] = std_err;
        cam_visandrec[i] = visandrec_i;
        cam_recandvis[i] = recandvis_i;
    }
}

/// Detect outlier observations and update the inlier set.
///
/// Equivalent to `CoreFunctions/findoutl.m`.
#[allow(clippy::too_many_arguments)]
fn find_outliers(
    cam_err2d: &[Vec<f64>],
    cam_mean2d_err: &[f64],
    cam_std2d_err: &[f64],
    cam_idlin: &[Vec<usize>],
    cam_visandrec: &[Vec<usize>],
    inlier_id_mat: &DMatrix<bool>,
    inl_tol: f64,
    num_cams_fill: usize,
    n_cams: usize,
) -> (usize, DMatrix<bool>, Vec<usize>) {
    let n_frames = inlier_id_mat.ncols();
    let mut idx_out_mat = DMatrix::<bool>::from_element(n_cams, n_frames, false);

    for i in 0..n_cams {
        if cam_std2d_err[i] > cam_mean2d_err[i] || cam_mean2d_err[i] > inl_tol {
            for (local_idx, &err) in cam_err2d[i].iter().enumerate() {
                let reprr = err - cam_mean2d_err[i];
                if reprr > 3.0 * cam_std2d_err[i] && reprr > inl_tol {
                    // Map local_idx back to original frame index
                    if local_idx < cam_visandrec[i].len() {
                        let vis_idx = cam_visandrec[i][local_idx];
                        if vis_idx < cam_idlin[i].len() {
                            let frame_idx = cam_idlin[i][vis_idx];
                            if frame_idx < n_frames {
                                idx_out_mat[(i, frame_idx)] = true;
                            }
                        }
                    }
                }
            }
        }
    }

    // Zero all columns with at least one outlier
    let mut new_id_mat = inlier_id_mat.clone();
    let mut n_outlier_cols = 0;
    for j in 0..n_frames {
        let has_outlier = (0..n_cams).any(|i| idx_out_mat[(i, j)]);
        if has_outlier {
            for i in 0..n_cams {
                new_id_mat[(i, j)] = false;
            }
            n_outlier_cols += 1;
        }
    }

    let new_inlier_idx: Vec<usize> = (0..n_frames)
        .filter(|&j| {
            let vis_count: usize = (0..n_cams).filter(|&i| new_id_mat[(i, j)]).count();
            vis_count >= n_cams - num_cams_fill
        })
        .collect();

    (n_outlier_cols, new_id_mat, new_inlier_idx)
}

fn print_cam_stats(
    cam_std: &[f64],
    cam_mean: &[f64],
    inlier_id_mat: &DMatrix<bool>,
    n_cams: usize,
) {
    tracing::info!("CamId    std       mean  #inliers");
    for i in 0..n_cams {
        let n_inliers: usize = (0..inlier_id_mat.ncols())
            .filter(|&j| inlier_id_mat[(i, j)])
            .count();
        tracing::info!(
            "{:>3}  {:>8.2}  {:>8.2} {:>6}",
            i + 1,
            cam_std[i],
            cam_mean[i],
            n_inliers
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic projective reconstruction from known cameras and
    /// 3-D points, then verify that `euclidize` recovers a valid Euclidean
    /// reconstruction.
    /// Load a space-separated ASCII matrix file (like octave's `save -ascii`).
    fn load_ascii_matrix(path: &str) -> DMatrix<f64> {
        let text =
            std::fs::read_to_string(path).unwrap_or_else(|e| panic!("Cannot read {path}: {e}"));
        let rows: Vec<Vec<f64>> = text
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
            .map(|line| {
                line.split_whitespace()
                    .map(|s| s.parse::<f64>().unwrap())
                    .collect()
            })
            .collect();
        let nrows = rows.len();
        let ncols = rows[0].len();
        DMatrix::from_fn(nrows, ncols, |r, c| rows[r][c])
    }

    /// Feed octave's P and X through the Rust euclidize and compare
    /// the output Pe*Xe reprojection against octave's Pe*Xe.
    ///
    /// This test requires data files produced by a debug run of the
    /// octave code. Skip if files don't exist.
    #[test]
    fn test_euclidize_matches_octave() {
        let pfm_path = "/tmp/mcsc_debug_Pfm.dat";
        if !std::path::Path::new(pfm_path).exists() {
            tracing::debug!("Skipping test_euclidize_matches_octave: {pfm_path} not found");
            return;
        }

        let p_fm = load_ascii_matrix(pfm_path);
        let x_fm = load_ascii_matrix("/tmp/mcsc_debug_Xfm.dat");
        let pp_mat = load_ascii_matrix("/tmp/mcsc_debug_pp.dat");
        let pe_oct = load_ascii_matrix("/tmp/mcsc_debug_Pe.dat");
        let xe_oct = load_ascii_matrix("/tmp/mcsc_debug_Xe.dat");

        let n_cams = p_fm.nrows() / 3;
        let n_pts = x_fm.ncols();
        tracing::debug!(
            "Loaded octave data: P {}x{}, X {}x{}, pp {}x{}",
            p_fm.nrows(),
            p_fm.ncols(),
            x_fm.nrows(),
            x_fm.ncols(),
            pp_mat.nrows(),
            pp_mat.ncols()
        );

        // Sign-normalize (same as run_mcsc does before euclidize)
        let (p_signed, rmat, lambda) = compute_rmat_lambda_signed(&p_fm, &x_fm, n_cams);

        // Run Rust euclidize
        let no_intrinsics: Vec<Option<Matrix3<f64>>> = vec![None; n_cams];
        let result = euclidize(
            &rmat,
            &lambda,
            &p_signed,
            &x_fm,
            &pp_mat,
            true,
            &no_intrinsics,
            &no_intrinsics,
        )
        .unwrap();

        // Compare nhom(Pe*Xe) between Rust and Octave
        let pe_xe_rs = &result.pe * &result.xe;
        let pe_xe_oct = &pe_oct * &xe_oct;

        let mut max_err_rs = 0.0_f64;
        let mut sum_err_rs = 0.0_f64;
        let mut count = 0;

        // Both should reproject to the same full pixel coordinates.
        // Compare each against the other's reprojection.
        for i in 0..n_cams {
            for j in 0..n_pts.min(pe_xe_rs.ncols()).min(pe_xe_oct.ncols()) {
                let w_rs = pe_xe_rs[(i * 3 + 2, j)];
                let w_oct = pe_xe_oct[(i * 3 + 2, j)];
                if w_rs.abs() > 1e-10 && w_oct.abs() > 1e-10 {
                    let u_rs = pe_xe_rs[(i * 3, j)] / w_rs;
                    let v_rs = pe_xe_rs[(i * 3 + 1, j)] / w_rs;
                    let u_oct = pe_xe_oct[(i * 3, j)] / w_oct;
                    let v_oct = pe_xe_oct[(i * 3 + 1, j)] / w_oct;
                    let err = ((u_rs - u_oct).powi(2) + (v_rs - v_oct).powi(2)).sqrt();
                    max_err_rs = max_err_rs.max(err);
                    sum_err_rs += err;
                    count += 1;
                }
            }
        }
        if count > 0 {
            tracing::debug!(
                "Rust vs Octave Pe*Xe: mean={:.4} max={:.4} ({count} entries)",
                sum_err_rs / count as f64,
                max_err_rs
            );
        }

        // The key test: given identical P,X input, do the two euclidize
        // implementations produce similar reprojections?
        assert!(
            max_err_rs < 5.0,
            "Rust euclidize diverges from Octave: max err {max_err_rs:.4} pixels"
        );
    }

    #[test]
    fn test_euclidize_invariants() {
        // 3 cameras, 10 random 3-D points
        let n_cams = 3;
        let n_pts = 10;

        // Known intrinsics (with pp at the centre of the image)
        // pp values that will be subtracted in euclidize
        let pp1 = [320.0, 240.0];
        let pp2 = [300.0, 250.0];
        let pp3 = [310.0, 230.0];
        // K matrices with cx,cy = pp (as euclidize expects)
        let k1 = Matrix3::new(500.0, 0.0, pp1[0], 0.0, 500.0, pp1[1], 0.0, 0.0, 1.0);
        let k2 = Matrix3::new(600.0, 0.0, pp2[0], 0.0, 600.0, pp2[1], 0.0, 0.0, 1.0);
        let k3 = Matrix3::new(550.0, 0.0, pp3[0], 0.0, 550.0, pp3[1], 0.0, 0.0, 1.0);

        // Known rotations (varied orientations, not just z-axis)
        let r1 = Matrix3::identity();
        // Camera 2: rotated ~30 deg around y
        let r2 = Matrix3::new(
            0.866, 0.0, 0.5, //
            0.0, 1.0, 0.0, //
            -0.5, 0.0, 0.866,
        );
        // Camera 3: rotated ~20 deg around x
        let r3 = Matrix3::new(
            1.0, 0.0, 0.0, //
            0.0, 0.9397, -0.342, //
            0.0, 0.342, 0.9397,
        );

        // Known camera centres (well-separated)
        let c1 = nalgebra::Vector3::new(0.0, 0.0, 0.0);
        let c2 = nalgebra::Vector3::new(2.0, 0.0, 0.5);
        let c3 = nalgebra::Vector3::new(-1.0, 2.0, 0.3);

        // Build 3x4 projection matrices: P = K * [R | -R*C]
        let build_p =
            |k: &Matrix3<f64>, r: &Matrix3<f64>, c: &nalgebra::Vector3<f64>| -> Matrix3x4<f64> {
                let t = -(r * c);
                let mut p = Matrix3x4::zeros();
                for i in 0..3 {
                    for j in 0..3 {
                        let mut val = 0.0;
                        for kk in 0..3 {
                            val += k[(i, kk)] * r[(kk, j)];
                        }
                        p[(i, j)] = val;
                    }
                    let mut val = 0.0;
                    for kk in 0..3 {
                        val += k[(i, kk)] * t[kk];
                    }
                    p[(i, 3)] = val;
                }
                p
            };

        let p1 = build_p(&k1, &r1, &c1);
        let p2 = build_p(&k2, &r2, &c2);
        let p3 = build_p(&k3, &r3, &c3);

        // Joint P matrix (9 x 4)
        let mut p_joint = DMatrix::<f64>::zeros(9, 4);
        for (idx, p) in [&p1, &p2, &p3].iter().enumerate() {
            for r in 0..3 {
                for c in 0..4 {
                    p_joint[(idx * 3 + r, c)] = p[(r, c)];
                }
            }
        }

        // Random 3-D points in front of all cameras
        let mut x_world = DMatrix::<f64>::zeros(4, n_pts);
        for j in 0..n_pts {
            x_world[(0, j)] = (j as f64 * 0.37).sin() * 2.0;
            x_world[(1, j)] = (j as f64 * 0.53).cos() * 2.0;
            x_world[(2, j)] = 5.0 + (j as f64 * 0.71).sin();
            x_world[(3, j)] = 1.0;
        }

        // Principal points (must match the cx,cy in K above)
        let mut pp = DMatrix::<f64>::zeros(n_cams, 3);
        pp[(0, 0)] = pp1[0];
        pp[(0, 1)] = pp1[1];
        pp[(1, 0)] = pp2[0];
        pp[(1, 1)] = pp2[1];
        pp[(2, 0)] = pp3[0];
        pp[(2, 1)] = pp3[1];

        // Measurement matrix M = P * X (full pixel coordinates)
        let m_mat_full = &p_joint * &x_world;

        // Centre the measurement matrix (subtract pp, matching gocal.m)
        let mut m_mat = m_mat_full.clone();
        for i in 0..n_cams {
            for j in 0..n_pts {
                m_mat[(i * 3, j)] -= pp[(i, 0)];
                m_mat[(i * 3 + 1, j)] -= pp[(i, 1)];
            }
        }

        // Build centred P
        let build_centred_p = |k: &Matrix3<f64>,
                               r: &Matrix3<f64>,
                               c: &nalgebra::Vector3<f64>,
                               ppx: f64,
                               ppy: f64|
         -> Matrix3x4<f64> {
            let mut kc = *k;
            kc[(0, 2)] -= ppx;
            kc[(1, 2)] -= ppy;
            let t = -(r * c);
            let mut p = Matrix3x4::zeros();
            for ii in 0..3 {
                for jj in 0..3 {
                    let mut val = 0.0;
                    for kk in 0..3 {
                        val += kc[(ii, kk)] * r[(kk, jj)];
                    }
                    p[(ii, jj)] = val;
                }
                let mut val = 0.0;
                for kk in 0..3 {
                    val += kc[(ii, kk)] * t[kk];
                }
                p[(ii, 3)] = val;
            }
            p
        };

        let p1c = build_centred_p(&k1, &r1, &c1, pp1[0], pp1[1]);
        let p2c = build_centred_p(&k2, &r2, &c2, pp2[0], pp2[1]);
        let p3c = build_centred_p(&k3, &r3, &c3, pp3[0], pp3[1]);

        let mut p_joint_c = DMatrix::<f64>::zeros(9, 4);
        for (idx, p) in [&p1c, &p2c, &p3c].iter().enumerate() {
            for r in 0..3 {
                for c in 0..4 {
                    p_joint_c[(idx * 3 + r, c)] = p[(r, c)];
                }
            }
        }

        // Lambda = third row of each camera block
        let m_centred = &p_joint_c * &x_world;
        let mut lambda = DMatrix::<f64>::zeros(n_cams, n_pts);
        for i in 0..n_cams {
            for j in 0..n_pts {
                lambda[(i, j)] = m_centred[(i * 3 + 2, j)];
            }
        }

        // Apply a projective transformation H to move away from Euclidean.
        // (euclidize is designed to recover Euclidean structure from a
        // projective reconstruction - it fails on already-Euclidean input
        // because Q ≈ I makes the plane-at-infinity degenerate.)
        let h = nalgebra::Matrix4::new(
            1.2, 0.1, -0.05, 0.3, //
            -0.08, 0.9, 0.03, -0.15, //
            0.02, -0.04, 1.1, 0.1, //
            0.15, -0.1, 0.08, 0.8,
        );
        let h_inv = h.try_inverse().unwrap();
        let p_proj = &p_joint_c * &DMatrix::from_fn(4, 4, |r, c| h[(r, c)]);
        let x_proj = DMatrix::from_fn(4, 4, |r, c| h_inv[(r, c)]) * &x_world;

        let rmat = &p_proj * &x_proj;
        let mut lambda_proj = DMatrix::<f64>::zeros(n_cams, n_pts);
        for i in 0..n_cams {
            for j in 0..n_pts {
                lambda_proj[(i, j)] = rmat[(i * 3 + 2, j)];
            }
        }

        // Run euclidize (self-calibration mode - existing invariant test)
        let no_intrinsics: Vec<Option<Matrix3<f64>>> = vec![None; n_cams];
        let result = euclidize(
            &rmat,
            &lambda_proj,
            &p_proj,
            &x_proj,
            &pp,
            true,
            &no_intrinsics,
            &no_intrinsics,
        )
        .unwrap();

        // Check invariant 1: Pe * Xe should reproduce the FULL pixel
        // measurement matrix.  `euclidize` adds pp back to K
        // internally, so nhom(Pe*Xe) gives full (non-centred) pixel
        // coordinates despite the centring of the input data.
        let pe_xe = &result.pe * &result.xe;
        let mut max_reproj = 0.0_f64;
        for i in 0..n_cams {
            for j in 0..n_pts {
                let w_orig = m_mat_full[(i * 3 + 2, j)];
                let w_rec = pe_xe[(i * 3 + 2, j)];
                if w_orig.abs() > 1e-10 && w_rec.abs() > 1e-10 {
                    let u_orig = m_mat_full[(i * 3, j)] / w_orig;
                    let v_orig = m_mat_full[(i * 3 + 1, j)] / w_orig;
                    let u_rec = pe_xe[(i * 3, j)] / w_rec;
                    let v_rec = pe_xe[(i * 3 + 1, j)] / w_rec;
                    let err = ((u_orig - u_rec).powi(2) + (v_orig - v_rec).powi(2)).sqrt();
                    max_reproj = max_reproj.max(err);
                }
            }
        }
        assert!(
            max_reproj < 2.0,
            "Max reprojection error too high: {max_reproj:.4} pixels"
        );

        // Check invariant 2: Xe(:,j) should be close to the original
        // world points (up to a similarity transform)
        // We check that the relative distances are preserved.
        let dist_orig = {
            let dx = x_world[(0, 0)] - x_world[(0, 1)];
            let dy = x_world[(1, 0)] - x_world[(1, 1)];
            let dz = x_world[(2, 0)] - x_world[(2, 1)];
            (dx * dx + dy * dy + dz * dz).sqrt()
        };
        let dist_rec = {
            let dx = result.xe[(0, 0)] - result.xe[(0, 1)];
            let dy = result.xe[(1, 0)] - result.xe[(1, 1)];
            let dz = result.xe[(2, 0)] - result.xe[(2, 1)];
            (dx * dx + dy * dy + dz * dz).sqrt()
        };
        // Distances should be proportional (same scale factor for all pairs)
        let dist_orig2 = {
            let dx = x_world[(0, 0)] - x_world[(0, 5)];
            let dy = x_world[(1, 0)] - x_world[(1, 5)];
            let dz = x_world[(2, 0)] - x_world[(2, 5)];
            (dx * dx + dy * dy + dz * dz).sqrt()
        };
        let dist_rec2 = {
            let dx = result.xe[(0, 0)] - result.xe[(0, 5)];
            let dy = result.xe[(1, 0)] - result.xe[(1, 5)];
            let dz = result.xe[(2, 0)] - result.xe[(2, 5)];
            (dx * dx + dy * dy + dz * dz).sqrt()
        };
        let ratio1 = dist_rec / dist_orig;
        let ratio2 = dist_rec2 / dist_orig2;
        assert!(
            (ratio1 - ratio2).abs() / ratio1.max(ratio2) < 0.01,
            "Distance ratios not preserved: {ratio1:.6} vs {ratio2:.6}"
        );
    }

    /// Known-intrinsics path of `euclidize`: when the caller provides full `K`
    /// per camera, the Euclidean upgrade must recover (R, t) while preserving
    /// the supplied K exactly — no skew, no focal drift.
    #[test]
    fn test_euclidize_known_intrinsics() {
        let n_cams = 3;
        let n_pts = 15;

        // True intrinsics (non-zero principal points, non-square pixels
        // on purpose — the known-K path must reproduce them exactly).
        let pp_pts = [[320.0, 240.0], [300.0, 250.0], [310.0, 230.0]];
        let k_full_list = [
            Matrix3::new(500.0, 0.0, 320.0, 0.0, 510.0, 240.0, 0.0, 0.0, 1.0),
            Matrix3::new(600.0, 0.0, 300.0, 0.0, 605.0, 250.0, 0.0, 0.0, 1.0),
            Matrix3::new(550.0, 0.0, 310.0, 0.0, 540.0, 230.0, 0.0, 0.0, 1.0),
        ];

        let r_list = [
            Matrix3::identity(),
            Matrix3::new(
                0.866, 0.0, 0.5, //
                0.0, 1.0, 0.0, //
                -0.5, 0.0, 0.866,
            ),
            Matrix3::new(
                1.0, 0.0, 0.0, //
                0.0, 0.9397, -0.342, //
                0.0, 0.342, 0.9397,
            ),
        ];
        let c_list = [
            nalgebra::Vector3::new(0.0, 0.0, 0.0),
            nalgebra::Vector3::new(2.0, 0.0, 0.5),
            nalgebra::Vector3::new(-1.0, 2.0, 0.3),
        ];

        // Full (non-centred) camera matrices.
        let build_p = |k: &Matrix3<f64>,
                       r: &Matrix3<f64>,
                       c: &nalgebra::Vector3<f64>|
         -> Matrix3x4<f64> {
            let t = -(r * c);
            let mut p = Matrix3x4::zeros();
            for ii in 0..3 {
                for jj in 0..3 {
                    let mut val = 0.0;
                    for kk in 0..3 {
                        val += k[(ii, kk)] * r[(kk, jj)];
                    }
                    p[(ii, jj)] = val;
                }
                let mut val = 0.0;
                for kk in 0..3 {
                    val += k[(ii, kk)] * t[kk];
                }
                p[(ii, 3)] = val;
            }
            p
        };

        let p_full_blocks: Vec<_> = (0..n_cams)
            .map(|i| build_p(&k_full_list[i], &r_list[i], &c_list[i]))
            .collect();

        let mut p_joint_full = DMatrix::<f64>::zeros(9, 4);
        for (i, p) in p_full_blocks.iter().enumerate() {
            for r in 0..3 {
                for c in 0..4 {
                    p_joint_full[(i * 3 + r, c)] = p[(r, c)];
                }
            }
        }

        // 3-D world points in front of all cameras.
        let mut x_world = DMatrix::<f64>::zeros(4, n_pts);
        for j in 0..n_pts {
            x_world[(0, j)] = (j as f64 * 0.37).sin() * 2.0;
            x_world[(1, j)] = (j as f64 * 0.53).cos() * 2.0;
            x_world[(2, j)] = 5.0 + (j as f64 * 0.71).sin();
            x_world[(3, j)] = 1.0;
        }

        // Full measurement matrix (what cameras would actually observe).
        let m_mat_full = &p_joint_full * &x_world;

        // Principal points for the centring.
        let mut pp = DMatrix::<f64>::zeros(n_cams, 3);
        for i in 0..n_cams {
            pp[(i, 0)] = pp_pts[i][0];
            pp[(i, 1)] = pp_pts[i][1];
        }

        // Centred P blocks and centred K matrices.
        let k_centred_list: Vec<Matrix3<f64>> = (0..n_cams)
            .map(|i| {
                let mut k = k_full_list[i];
                k[(0, 2)] -= pp[(i, 0)];
                k[(1, 2)] -= pp[(i, 1)];
                k
            })
            .collect();

        let p_centred_blocks: Vec<_> = (0..n_cams)
            .map(|i| build_p(&k_centred_list[i], &r_list[i], &c_list[i]))
            .collect();

        let mut p_joint_c = DMatrix::<f64>::zeros(9, 4);
        for (i, p) in p_centred_blocks.iter().enumerate() {
            for r in 0..3 {
                for c in 0..4 {
                    p_joint_c[(i * 3 + r, c)] = p[(r, c)];
                }
            }
        }

        // Move away from Euclidean with a projective H.
        let h = nalgebra::Matrix4::new(
            1.2, 0.1, -0.05, 0.3, //
            -0.08, 0.9, 0.03, -0.15, //
            0.02, -0.04, 1.1, 0.1, //
            0.15, -0.1, 0.08, 0.8,
        );
        let h_inv = h.try_inverse().unwrap();
        let p_proj = &p_joint_c * &DMatrix::from_fn(4, 4, |r, c| h[(r, c)]);
        let x_proj = DMatrix::from_fn(4, 4, |r, c| h_inv[(r, c)]) * &x_world;

        let rmat = &p_proj * &x_proj;
        let mut lambda_proj = DMatrix::<f64>::zeros(n_cams, n_pts);
        for i in 0..n_cams {
            for j in 0..n_pts {
                lambda_proj[(i, j)] = rmat[(i * 3 + 2, j)];
            }
        }

        // Supply full K per camera.
        let k_full_vec: Vec<Option<Matrix3<f64>>> =
            k_full_list.iter().map(|k| Some(*k)).collect();
        let k_c_vec: Vec<Option<Matrix3<f64>>> =
            k_centred_list.iter().map(|k| Some(*k)).collect();

        let result = euclidize(
            &rmat,
            &lambda_proj,
            &p_proj,
            &x_proj,
            &pp,
            true,
            &k_full_vec,
            &k_c_vec,
        )
        .expect("euclidize with known K must succeed");

        // Invariant 1: Pe_rt * Xe must reproduce the FULL pixel matrix.
        let pe_xe = &result.pe * &result.xe;
        let mut max_reproj = 0.0_f64;
        for i in 0..n_cams {
            for j in 0..n_pts {
                let w_orig = m_mat_full[(i * 3 + 2, j)];
                let w_rec = pe_xe[(i * 3 + 2, j)];
                if w_orig.abs() > 1e-10 && w_rec.abs() > 1e-10 {
                    let u_orig = m_mat_full[(i * 3, j)] / w_orig;
                    let v_orig = m_mat_full[(i * 3 + 1, j)] / w_orig;
                    let u_rec = pe_xe[(i * 3, j)] / w_rec;
                    let v_rec = pe_xe[(i * 3 + 1, j)] / w_rec;
                    let err = ((u_orig - u_rec).powi(2) + (v_orig - v_rec).powi(2)).sqrt();
                    max_reproj = max_reproj.max(err);
                }
            }
        }
        // Much tighter than the 2.0 px tolerance of the self-cal test:
        // with K fixed a priori, only SVD round-off stands between us
        // and exact recovery.
        assert!(
            max_reproj < 0.1,
            "Known-K reprojection should be near-exact: max err {max_reproj:.6e} pixels"
        );

        // Invariant 2: the 3×3 block of each recovered Pe_rt must
        // factor as K_full · R with R a proper rotation (det = +1,
        // orthonormal columns).  Since K_full has zero skew, this is
        // the key property that the Octave / self-cal path loses.
        for i in 0..n_cams {
            let mut m33 = Matrix3::zeros();
            for r in 0..3 {
                for c in 0..3 {
                    m33[(r, c)] = result.pe[(i * 3 + r, c)];
                }
            }
            // R = K_full^{-1} * M should be orthonormal.
            let k_inv = k_full_list[i].try_inverse().unwrap();
            let r_recovered = k_inv * m33;
            let r_rt_r = r_recovered.transpose() * r_recovered;
            for a in 0..3 {
                for b in 0..3 {
                    let expected = if a == b { 1.0 } else { 0.0 };
                    assert!(
                        (r_rt_r[(a, b)] - expected).abs() < 1e-5,
                        "cam {i}: K_full^-1 * Pe_rt(3x3) is not a rotation at ({a},{b}): {}, expected {}",
                        r_rt_r[(a, b)],
                        expected
                    );
                }
            }
            let det = r_recovered.determinant();
            assert!(
                (det - 1.0).abs() < 1e-5,
                "cam {i}: recovered R has det {det}, expected +1"
            );
        }
    }
}
