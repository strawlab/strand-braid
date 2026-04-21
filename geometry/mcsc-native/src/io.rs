//! I/O and parsing utilities for MCSC data files.
//!
//! Shared functions for reading the various MCSC input files:
//! multicamselfcal.cfg, camera_order.txt, Res.dat, IdMat.dat, points.dat,
//! and .rad files.

use crate::McscIniConfig;
use eyre::{Context, Result};
use nalgebra::{DMatrix, Matrix3};

/// Parse camera order from camera_order.txt.
///
/// Returns a vector of camera names, one per line.
pub fn parse_camera_order(content: &str) -> Result<Vec<String>> {
    content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| Ok(l.trim().to_string()))
        .collect()
}

/// Parse camera resolutions from Res.dat.
///
/// Returns a vector of [width, height] pairs, one per camera.
///
/// # Format
/// Each line contains two unsigned integers: width height
pub fn parse_res_dat(content: &str) -> Result<Vec<[usize; 2]>> {
    let mut res_vec = Vec::new();
    for (i, line) in content.lines().enumerate() {
        let parts: Vec<usize> = line
            .split_whitespace()
            .map(|s| {
                s.parse()
                    .with_context(|| format!("Invalid resolution value at line {}", i + 1))
            })
            .collect::<Result<Vec<usize>>>()?;

        if parts.len() != 2 {
            eyre::bail!(
                "Res.dat line {} should have 2 values, got {}",
                i + 1,
                parts.len()
            );
        }
        res_vec.push([parts[0], parts[1]]);
    }
    Ok(res_vec)
}

/// Parse visibility matrix from IdMat.dat.
///
/// Returns a boolean matrix where true indicates the point is visible
/// from the camera. The data file format uses 0/1 integers.
///
/// # Format
/// ASCII matrix with n_cams rows and n_points columns, values are 0 or 1.
pub fn parse_id_mat(content: &str, n_cams: usize, n_points: usize) -> Result<DMatrix<bool>> {
    let mut id_mat_vec = Vec::new();
    for (line_idx, line) in content.lines().enumerate() {
        for (col_idx, val_str) in line.split_whitespace().enumerate() {
            let val_i8: i8 = val_str.parse().with_context(|| {
                format!(
                    "Invalid IdMat value at line {}, column {}",
                    line_idx + 1,
                    col_idx + 1
                )
            })?;
            id_mat_vec.push(val_i8 != 0);
        }
    }

    if id_mat_vec.len() != n_cams * n_points {
        eyre::bail!(
            "IdMat.dat has {} values, expected {} (num_cameras * n_points)",
            id_mat_vec.len(),
            n_cams * n_points
        );
    }

    Ok(DMatrix::from_row_slice(n_cams, n_points, &id_mat_vec))
}

/// Parse observations matrix from points.dat.
///
/// Returns a dense matrix of shape (3*n_cams x n_points) with f64 values.
///
/// # Format
/// ASCII matrix with 3*n_cams rows and n_points columns.
pub fn parse_points_dat(content: &str, n_cams: usize, n_points: usize) -> Result<DMatrix<f64>> {
    let mut points_vec = Vec::new();
    for (line_idx, line) in content.lines().enumerate() {
        for (col_idx, val_str) in line.split_whitespace().enumerate() {
            let val: f64 = val_str.parse().with_context(|| {
                format!(
                    "Invalid points.dat value at line {}, column {}",
                    line_idx + 1,
                    col_idx + 1
                )
            })?;
            points_vec.push(val);
        }
    }

    let expected_values = 3 * n_cams * n_points;
    if points_vec.len() != expected_values {
        eyre::bail!(
            "points.dat has {} values, expected {} (3*num_cameras*n_points)",
            points_vec.len(),
            expected_values
        );
    }

    Ok(DMatrix::from_row_slice(3 * n_cams, n_points, &points_vec))
}

/// Parse a .rad file containing camera intrinsics and distortion coefficients.
///
/// Returns (K matrix, kc array) where K is 3x3 and kc has 4 elements.
///
/// # Format
/// Key-value pairs separated by '=':
/// K11, K12, K13, K21, K22, K23, K31, K32, K33: camera matrix elements
/// kc1, kc2, kc3, kc4: distortion coefficients
pub fn parse_rad_file(content: &str) -> Result<(Matrix3<f64>, [f64; 4])> {
    let mut k = [0.0_f64; 9];
    let mut kc = [0.0_f64; 4];

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Split on '=' allowing spaces around it
        let mut parts = line.splitn(2, '=');
        let key = match parts.next() {
            Some(k) => k.trim(),
            None => continue,
        };
        let val_str = match parts.next() {
            Some(v) => v.trim(),
            None => continue,
        };
        match key {
            "K11" => k[0] = parse_f64(val_str)?,
            "K12" => k[1] = parse_f64(val_str)?,
            "K13" => k[2] = parse_f64(val_str)?,
            "K21" => k[3] = parse_f64(val_str)?,
            "K22" => k[4] = parse_f64(val_str)?,
            "K23" => k[5] = parse_f64(val_str)?,
            "K31" => k[6] = parse_f64(val_str)?,
            "K32" => k[7] = parse_f64(val_str)?,
            "K33" => k[8] = parse_f64(val_str)?,
            "kc1" => kc[0] = parse_f64(val_str)?,
            "kc2" => kc[1] = parse_f64(val_str)?,
            "kc3" => kc[2] = parse_f64(val_str)?,
            "kc4" => kc[3] = parse_f64(val_str)?,
            _ => {} // ignore unknown keys
        }
    }

    let k_mat = Matrix3::new(k[0], k[1], k[2], k[3], k[4], k[5], k[6], k[7], k[8]);

    Ok((k_mat, kc))
}

/// Parse a float value, handling potential trailing characters.
fn parse_f64(s: &str) -> Result<f64> {
    s.trim()
        .parse()
        .with_context(|| format!("Failed to parse f64: {}", s))
}

fn apply_subsampling(content: &str, nth: usize) -> String {
    let mut subsampled_lines: Vec<String> = Vec::new();
    for line in content.lines() {
        let vals: Vec<&str> = line.split_whitespace().collect();
        let mut subsampled_vals = Vec::new();
        for (idx, val) in vals.iter().enumerate() {
            if idx % nth == 0 {
                subsampled_vals.push(*val);
            }
        }
        subsampled_lines.push(subsampled_vals.join(" "));
    }
    subsampled_lines.join("\n")
}

/// Load MCSC data files from a directory referenced by McscIniConfig.
/// Returns McscInput ready for processing.
pub fn load_mcsc_data(config: &McscIniConfig) -> Result<crate::McscInput, eyre::Error> {
    let dir = &config.config_dir;

    // Read camera_order.txt
    let camera_order_path = dir.join("camera_order.txt");
    let camera_order_content = std::fs::read_to_string(&camera_order_path)
        .with_context(|| format!("Failed to read {camera_order_path}"))?;
    let camera_names = parse_camera_order(&camera_order_content)?;

    if camera_names.len() != config.num_cameras {
        return Err(eyre::eyre!(
            "camera_order.txt has {} cameras, but Num-Cameras is {}",
            camera_names.len(),
            config.num_cameras
        ));
    }

    // Read Res.dat (camera resolutions, n_cams x 2)
    let res_path = dir.join("Res.dat");
    let res_content =
        std::fs::read_to_string(&res_path).with_context(|| format!("Failed to read {res_path}"))?;
    let res_vec = parse_res_dat(&res_content)?;

    if res_vec.len() != config.num_cameras {
        return Err(eyre::eyre!(
            "Res.dat has {} entries, but Num-Cameras is {}",
            res_vec.len(),
            config.num_cameras
        ));
    }

    // Read IdMat.dat (visibility matrix)
    let id_mat_path = dir.join("IdMat.dat");
    let id_mat_content = std::fs::read_to_string(&id_mat_path)
        .with_context(|| format!("Failed to read {id_mat_path}"))?;

    // Determine n_points from IdMat.dat dimensions
    let n_points = compute_n_points(&id_mat_content, config.num_cameras)?;

    // Apply Use-Nth-Frame subsampling to IdMat.dat if configured
    let (n_points, id_mat_content) = if config.use_nth_frame > 1 {
        let nth = config.use_nth_frame as usize;
        let n_points_before = n_points;
        let new_n_points = n_points_before.div_ceil(nth);
        let new_content = apply_subsampling(&id_mat_content, nth);
        tracing::debug!(
            "Use-Nth-Frame={}: subsampled IdMat.dat from {} to {} points",
            nth,
            n_points_before,
            new_n_points
        );
        (new_n_points, new_content)
    } else {
        (n_points, id_mat_content)
    };

    let id_mat = parse_id_mat(&id_mat_content, config.num_cameras, n_points)?;
    let points_path = dir.join("points.dat");
    let mut points_content = std::fs::read_to_string(&points_path)
        .with_context(|| format!("Failed to read {points_path}"))?;

    // Apply Use-Nth-Frame subsampling to points.dat if configured
    if config.use_nth_frame > 1 {
        let nth = config.use_nth_frame as usize;
        points_content = apply_subsampling(&points_content, nth);
    }

    let points = parse_points_dat(&points_content, config.num_cameras, n_points)?;

    // Load radial distortion files if undo_radial is true
    let intrinsics = if config.undo_radial {
        let mut intrinsics = Vec::new();
        for i in 0..config.num_cameras {
            let rad_path = dir.join(format!("basename{}.rad", i + 1));
            if !rad_path.exists() {
                eyre::bail!(
                    "Configuration specified undo_radial, but expected radial file {rad_path} does not exist"
                );
            }
            let rad_content = std::fs::read_to_string(&rad_path)
                .with_context(|| format!("Failed to read {rad_path}"))?;
            let (k, kc) = parse_rad_file(&rad_content)?;
            intrinsics.push((k, kc));
        }

        if intrinsics.len() != config.num_cameras {
            return Err(eyre::eyre!(
                "Loaded {} rad files, but Num-Cameras is {}",
                intrinsics.len(),
                config.num_cameras
            ));
        }

        intrinsics
    } else {
        vec![]
    };

    Ok(crate::McscInput {
        id_mat,
        points,
        res: res_vec,
        intrinsics,
        camera_names,
    })
}

/// Compute the number of points from a matrix file content.
/// This is needed because we need n_points before parsing.
fn compute_n_points(content: &str, n_cams: usize) -> Result<usize, eyre::Error> {
    let mut total_values = 0usize;
    for line in content.lines() {
        total_values += line.split_whitespace().count();
    }
    if total_values % n_cams != 0 {
        return Err(eyre::eyre!(
            "Matrix has {} values, not a multiple of Num-Cameras ({})",
            total_values,
            n_cams
        ));
    }
    Ok(total_values / n_cams)
}

/// Parse multicamselfcal.cfg file.
///
/// Returns a configuration structure with the key parameters needed for MCSC.
pub fn parse_mcsc_config(cfg_path: &camino::Utf8Path) -> Result<McscIniConfig> {
    let cfg_dir = cfg_path
        .parent()
        .ok_or_else(|| eyre::eyre!("config path has no parent directory"))?;

    let cfg_file = cfg_dir.join("multicamselfcal.cfg");
    let cfg_content =
        std::fs::read_to_string(&cfg_file).with_context(|| format!("Failed to read {cfg_file}"))?;

    let (num_cameras, num_cams_fill_raw, do_ba, undo_radial, use_nth_frame, inl_tol) =
        parse_multicamselfcal_cfg(&cfg_content)?;

    Ok(McscIniConfig {
        config_dir: cfg_dir.to_path_buf(),
        num_cameras,
        num_cams_fill_raw,
        do_ba,
        undo_radial,
        use_nth_frame,
        inl_tol,
    })
}

/// Parse the contents of multicamselfcal.cfg.
///
/// Returns (num_cameras, num_cams_fill_raw, do_ba, undo_radial, use_nth_frame, inl_tol)
fn parse_multicamselfcal_cfg(
    content: &str,
) -> Result<(usize, Option<usize>, bool, bool, u16, f64)> {
    use eyre::eyre;

    let mut num_cameras: Option<usize> = None;
    let mut num_projectors: Option<usize> = None;
    let mut do_ba = false;
    let mut undo_radial = false;
    let mut use_nth_frame: Option<u16> = None;
    let mut do_global_iterations: Option<i8> = None;
    let mut inl_tol: Option<f64> = None;
    let mut num_cams_fill: Option<usize> = None;

    enum Section {
        None,
        Files,
        Images,
        Calibration,
    }

    let mut current_section = Section::None;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            current_section = match line {
                "[Files]" => Section::Files,
                "[Images]" => Section::Images,
                "[Calibration]" => Section::Calibration,
                _ => return Err(eyre!("Unknown section: {}", line)),
            };
            continue;
        }

        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() != 2 {
            return Err(eyre!("Invalid config line: {}", line));
        }

        let key = parts[0].trim();
        let value = parts[1].trim();

        match current_section {
            Section::Files => {
                match key {
                    "Basename" => {
                        // Expected format: basename (e.g., "basename")
                        // This should match the pattern basename*.rad files
                    }
                    "Image-Extension" => {
                        // Expected: jpg or png - we only validate this exists
                        let ext = value.to_lowercase();
                        if !matches!(ext.as_str(), "jpg" | "jpeg" | "png") {
                            return Err(eyre!(
                                "Unsupported Image-Extension: {}. Expected jpg or png",
                                value
                            ));
                        }
                    }
                    _ => return Err(eyre!("Unsupported [Files] key: {}", key)),
                }
            }
            Section::Images => match key {
                "Subpix" => {
                    let subpix: f64 = value
                        .parse()
                        .with_context(|| format!("Invalid Subpix value: {}", value))?;
                    if !subpix.is_finite() {
                        return Err(eyre!("Subpix must be finite: {}", subpix));
                    }
                }
                _ => return Err(eyre!("Unsupported [Images] key: {}", key)),
            },
            Section::Calibration => {
                match key {
                    "Num-Cameras" => {
                        let val = value
                            .parse()
                            .with_context(|| format!("Invalid Num-Cameras value: {}", value))?;
                        num_cameras = Some(val);
                    }
                    "Num-Projectors" => {
                        let val = value
                            .parse()
                            .with_context(|| format!("Invalid Num-Projectors value: {}", value))?;
                        num_projectors = Some(val);
                    }
                    "Nonlinear-Parameters" => {
                        // Expected: 6 space-separated integers
                        let vals: Vec<i8> = value
                            .split_whitespace()
                            .map(|s| {
                                s.parse().with_context(|| {
                                    format!("Invalid Nonlinear-Parameters value: {}", s)
                                })
                            })
                            .collect::<Result<Vec<i8>>>()?;

                        if vals.len() != 6 {
                            return Err(eyre!(
                                "Nonlinear-Parameters must have exactly 6 values, got {}",
                                vals.len()
                            ));
                        }
                        // We ignore nonlinear parameters (not used in this port)
                        if vals.iter().any(|&v| v != 0) {
                            tracing::debug!(
                                "Warning: Nonlinear-Parameters {:?} will be ignored",
                                vals
                            );
                        }
                    }
                    "Nonlinear-Update" => {
                        // Expected: 6 space-separated integers
                        let vals: Vec<i8> = value
                            .split_whitespace()
                            .map(|s| {
                                s.parse().with_context(|| {
                                    format!("Invalid Nonlinear-Update value: {}", s)
                                })
                            })
                            .collect::<Result<Vec<i8>>>()?;

                        if vals.len() != 6 {
                            return Err(eyre!(
                                "Nonlinear-Update must have exactly 6 values, got {}",
                                vals.len()
                            ));
                        }
                        // We ignore nonlinear update (not used in this port)
                        if vals.iter().any(|&v| v != 0) {
                            tracing::debug!("Warning: Nonlinear-Update {:?} will be ignored", vals);
                        }
                    }
                    "Initial-Tolerance" => {
                        let val: f64 = value.parse().with_context(|| {
                            format!("Invalid Initial-Tolerance value: {}", value)
                        })?;
                        if !val.is_finite() || val <= 0.0 {
                            return Err(eyre!(
                                "Initial-Tolerance must be positive and finite, got {}",
                                val
                            ));
                        }
                        inl_tol = Some(val);
                    }
                    "Do-Global-Iterations" => {
                        do_global_iterations = Some(value.parse().with_context(|| {
                            format!("Invalid Do-Global-Iterations value: {}", value)
                        })?);
                    }
                    "Num-Cameras-Fill" => {
                        let val: usize = value.parse().with_context(|| {
                            format!("Invalid Num-Cameras-Fill value: {}", value)
                        })?;
                        num_cams_fill = Some(val);
                    }
                    "Undo-Radial" => {
                        let val: i8 = value
                            .parse()
                            .with_context(|| format!("Invalid Undo-Radial value: {}", value))?;
                        if val != 0 && val != 1 {
                            return Err(eyre!("Undo-Radial must be 0 or 1, got {}", val));
                        }
                        undo_radial = val != 0;
                    }
                    "Use-Nth-Frame" => {
                        let val: u16 = value
                            .parse()
                            .with_context(|| format!("Invalid Use-Nth-Frame value: {}", value))?;
                        if val == 0 {
                            return Err(eyre!("Use-Nth-Frame must be >= 1"));
                        }
                        use_nth_frame = Some(val);
                    }
                    "Do-Bundle-Adjustment" => {
                        let val: i8 = value.parse().with_context(|| {
                            format!("Invalid Do-Bundle-Adjustment value: {}", value)
                        })?;
                        if val != 0 && val != 1 {
                            return Err(eyre!("Do-Bundle-Adjustment must be 0 or 1, got {}", val));
                        }
                        do_ba = val != 0;
                    }
                    key => {
                        tracing::debug!("Warning: ignoring unknown [Calibration] key: {}", key);
                    }
                }
            }
            Section::None => {
                return Err(eyre!("Key '{}' appears before any section", key));
            }
        }
    }

    let num_cameras = num_cameras.ok_or_else(|| eyre!("Num-Cameras not specified"))?;

    // Validate Num-Projectors if specified
    if let Some(np) = num_projectors
        && np != 0
    {
        return Err(eyre!("Num-Projectors must be 0. Got: {}", np));
    }

    // Validate Do-Global-Iterations if specified
    if let Some(dgi) = do_global_iterations
        && dgi != 0
    {
        return Err(eyre!("Do-Global-Iterations must be 0. Got: {}", dgi));
    }

    let use_nth_frame = use_nth_frame.unwrap_or(1);
    // Octave default (see CommonCfgAndIO/read_configuration.m): INL_TOL = 5 if unspecified.
    let inl_tol = inl_tol.unwrap_or(5.0);

    Ok((
        num_cameras,
        num_cams_fill,
        do_ba,
        undo_radial,
        use_nth_frame,
        inl_tol,
    ))
}

/// Convert McscIniConfig to McscConfig, applying any necessary transformations.
pub fn ini_to_mcsc_config(config: &McscIniConfig) -> crate::McscCfg {
    // Apply the same clamp as Octave gocal.m:
    //   NUM_CAMS_FILL defaults to (via configdata.m) often a large value
    //   (e.g. CAMS), and then
    //     if CAMS - NUM_CAMS_FILL < 3, NUM_CAMS_FILL = CAMS - 3;
    // so that points visible in at least 3 cameras are kept.
    let requested_fill = config.num_cams_fill_raw.unwrap_or(2);
    let clamped_fill =
        if config.num_cameras >= 3 && config.num_cameras.saturating_sub(requested_fill) < 3 {
            config.num_cameras - 3
        } else {
            requested_fill
        };

    crate::McscCfg {
        num_cams_fill: clamped_fill,
        inl_tol: config.inl_tol,
        do_bundle_adjustment: config.do_ba,
        undo_radial: config.undo_radial,
        square_pix: true,
    }
}
