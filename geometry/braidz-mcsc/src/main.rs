use bundle_adj::CameraModelType;
use camino::{Utf8Path, Utf8PathBuf};
use clap::Parser;
use eyre::{self, Context, Result};
use flydra_mvg::flydra_xml_support::{FlydraDistortionModel, SingleCameraCalibration};
use levenberg_marquardt::LeastSquaresProblem;
use opencv_ros_camera::RosOpenCvIntrinsics;
use std::{
    collections::{BTreeMap, HashMap},
    io::Read,
};

use mcsc_native::McscCfg;

#[cfg(test)]
mod tests;

#[cfg(feature = "with-octave")]
pub(crate) mod with_octave;

#[derive(Parser, Default)]
pub(crate) struct Cli {
    /// Input braidz filename.
    #[arg(long)]
    pub(crate) input: Utf8PathBuf,

    /// Input directory to be searched for YAML calibration files from
    /// checkerboard calibration. (Typically
    /// "~/.config/strand-cam/camera_info").
    #[arg(long)]
    pub(crate) checkerboard_cal_dir: Option<Utf8PathBuf>,

    #[arg(long)]
    pub(crate) force_allow_no_checkerboard_cal: bool,

    /// Rather than using each frame, use only 1/N of them.
    #[arg(long)]
    pub(crate) use_nth_observation: Option<u16>,

    /// Do not perform bundle adjustment
    #[arg(long)]
    pub(crate) no_bundle_adjustment: bool,

    /// Let MCSC perform bundle adjustment
    #[arg(long)]
    pub(crate) do_mcsc_bundle_adjustment: bool,

    /// Type of bundle adjustment to perform
    #[arg(long, value_enum, default_value_t)]
    pub(crate) bundle_adjustment_model: CameraModelType,

    /// Source of camera intrinsics when initializing bundle adjustment
    #[arg(long, value_enum, default_value_t)]
    pub(crate) bundle_adjustment_intrinsics_source: BAIntrinsicsSource,

    #[cfg(feature = "with-rerun")]
    /// Log data to rerun viewer at this socket address. (The typical address is
    /// "127.0.0.1:9876".) DEPRECATED. Use `rerun_url` instead.
    #[arg(long, hide = true)]
    pub(crate) rerun: Option<String>,

    #[cfg(not(feature = "with-rerun"))]
    /// Disabled. To enable, recompile with the `with-rerun` feature.
    #[arg(long, hide = true)]
    pub(crate) rerun: Option<String>,

    #[cfg(feature = "with-rerun")]
    /// Log data to rerun viewer at this URL. (A typical url is
    /// "rerun+http://127.0.0.1:9876/proxy\\").")
    #[arg(long)]
    pub(crate) rerun_url: Option<String>,

    #[cfg(not(feature = "with-rerun"))]
    /// Disabled. To enable, recompile with the `with-rerun` feature.
    #[arg(long)]
    pub(crate) rerun_url: Option<String>,
}

#[derive(Debug, Clone, clap::ValueEnum, Default, PartialEq)]
pub(crate) enum BAIntrinsicsSource {
    #[default]
    MCSCNoSkew,
    CheckerboardCal,
}

/// One row from `cam_info.csv`.
#[derive(serde::Deserialize)]
pub(crate) struct CamInfoRow {
    pub(crate) cam_id: String,
    pub(crate) camn: i64,
}

/// Read `cam_info.csv` from a `Read` source into a `Vec<CamInfoRow>`.
pub(crate) fn read_cam_info<R: Read>(reader: R) -> Result<Vec<CamInfoRow>> {
    let mut rdr = csv::Reader::from_reader(reader);
    let rows = rdr.deserialize().collect::<Result<Vec<CamInfoRow>, _>>()?;
    Ok(rows)
}

/// One row from `data2d_distorted.csv` (only the columns we need).
#[derive(serde::Deserialize)]
pub(crate) struct Data2dRow {
    pub(crate) frame: i64,
    pub(crate) camn: i64,
    pub(crate) x: f64,
    pub(crate) y: f64,
}

/// Read `data2d_distorted.csv` from a `Read` source, keeping only the columns
/// we need and discarding rows where `x` is NaN.
pub(crate) fn read_data2d<R: Read>(reader: R) -> Result<Vec<Data2dRow>> {
    let mut rdr = csv::Reader::from_reader(reader);
    let rows = rdr.deserialize().collect::<Result<Vec<Data2dRow>, _>>()?;
    Ok(rows.into_iter().filter(|r| !r.x.is_nan()).collect())
}

/// Group `Data2dRow` values by `frame`, preserving the order of first
/// appearance of each frame value (analogous to `partition_by_stable`).
pub(crate) fn group_by_frame(rows: Vec<Data2dRow>) -> Vec<Vec<Data2dRow>> {
    let mut frame_to_group: HashMap<i64, usize> = HashMap::new();
    let mut groups: Vec<Vec<Data2dRow>> = Vec::new();
    for row in rows {
        let group_idx = if let Some(&idx) = frame_to_group.get(&row.frame) {
            idx
        } else {
            let idx = groups.len();
            frame_to_group.insert(row.frame, idx);
            groups.push(Vec::new());
            idx
        };
        groups[group_idx].push(row);
    }
    groups
}

/// Degrees of freedom used for sample standard deviation.
const DDOF: usize = 1;

/// Compute the mean of a slice of `f64` values.  Returns `NaN` if empty.
pub(crate) fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        return f64::NAN;
    }
    values.iter().sum::<f64>() / values.len() as f64
}

/// Compute the sample standard deviation of a slice of `f64` values.
/// Returns `NaN` if the slice has fewer than `DDOF + 1` elements.
pub(crate) fn std_dev(values: &[f64]) -> f64 {
    if values.len() <= DDOF {
        return f64::NAN;
    }
    let m = mean(values);
    let variance =
        values.iter().map(|v| (v - m).powi(2)).sum::<f64>() / (values.len() - DDOF) as f64;
    variance.sqrt()
}

fn main() -> Result<()> {
    if std::env::var_os("RUST_LOG").is_none() {
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("RUST_LOG", "info") };
    }
    env_tracing_logger::init();
    let opt = Cli::parse();
    let (xml_out_name, mcsc_result) = braidz_mcsc(opt)?;
    let dist = mcsc_result.mean_reproj_distance;
    println!(
        "Unaligned calibration XML (mean reprojection distance: {dist:.2} pixels) saved to {xml_out_name}",
    );
    Ok(())
}

/// Run MCSC calibration using the native Rust implementation.
pub(crate) fn braidz_mcsc(opt: Cli) -> Result<(Utf8PathBuf, mcsc_native::McscResult)> {
    let use_nth_observation = opt.use_nth_observation.unwrap_or(1);

    let mut archive = zip_or_dir::ZipDirArchive::auto_from_path(&opt.input)
        .with_context(|| format!("Parsing file {}", opt.input))?;

    let cam_info_rows = {
        // Read `cam_info.csv` to memory.
        let data_fname = archive.path_starter().join(braid_types::CAM_INFO_CSV_FNAME);
        let mut rdr = braidz_parser::open_maybe_gzipped(data_fname)?;
        let mut buf = Vec::new();
        rdr.read_to_end(&mut buf)?;
        read_cam_info(buf.as_slice())?
    };

    let mut camn2cam_id = BTreeMap::new();
    let mut images = BTreeMap::new();
    let mut camera_order = vec![];
    let mut res = vec![];
    for row in cam_info_rows.iter() {
        let cam_id = row.cam_id.as_str();
        camera_order.push(cam_id.to_string());

        let image_fname = archive
            .path_starter()
            .join(braid_types::IMAGES_DIRNAME)
            .join(format!("{cam_id}.png"));

        let mut rdr = braidz_parser::open_maybe_gzipped(image_fname)?;
        let mut im_buf = Vec::new();
        rdr.read_to_end(&mut im_buf)?;

        let im = image::load_from_memory(&im_buf)?;
        res.push([im.width() as usize, im.height() as usize]);
        images.insert(cam_id, im);
    }
    let camns: Vec<i64> = cam_info_rows.iter().map(|r| r.camn).collect();
    for (camn, cam_id) in camns.iter().zip(camera_order.iter()) {
        camn2cam_id.insert(*camn, cam_id.clone());
    }

    // Load radial distortion parameters
    let (radfiles, checkerboard_intrinsics) = if let Some(checkerboard_cal_dir) =
        &opt.checkerboard_cal_dir
    {
        if opt.force_allow_no_checkerboard_cal {
            eyre::bail!(
                "--checkerboard-cal-dir was specified but --force-allow-no-checkerboard-cal is set."
            );
        }
        let mut radfiles = vec![];
        let mut checkerboard_intrinsics = vec![];
        for row in cam_info_rows.iter() {
            let cam_id = row.cam_id.as_str();
            let yaml_intrinsics_fname = checkerboard_cal_dir.join(format!("{cam_id}.yaml"));
            let yaml_buf = std::fs::read_to_string(&yaml_intrinsics_fname)
                .with_context(|| format!("while reading {yaml_intrinsics_fname}"))?;

            let cam_info: opencv_ros_camera::RosCameraInfo<f64> =
                serde_yaml::from_str(&yaml_buf)
                    .with_context(|| format!("while parsing {yaml_intrinsics_fname}"))?;

            let k = &cam_info.camera_matrix.data;
            let k_mat =
                nalgebra::Matrix3::new(k[0], k[1], k[2], k[3], k[4], k[5], k[6], k[7], k[8]);
            let d = &cam_info.distortion_coefficients.data;
            let kc = [
                d[0],
                d[1],
                d.get(2).copied().unwrap_or(0.0),
                d.get(3).copied().unwrap_or(0.0),
            ];
            radfiles.push((k_mat, kc));

            // Check that images have expected resolution.
            let im = &images[cam_id];
            let w: usize = im.width().try_into().unwrap();
            let h: usize = im.height().try_into().unwrap();
            if cam_info.image_width != w {
                eyre::bail!("PNG image resolution does not match YAML file.")
            }
            if cam_info.image_height != h {
                eyre::bail!("PNG image resolution does not match YAML file.")
            }

            let named: opencv_ros_camera::NamedIntrinsicParameters<f64> = cam_info.try_into()?;
            checkerboard_intrinsics.push(named.intrinsics);
        }
        (radfiles, Some(checkerboard_intrinsics))
    } else if opt.force_allow_no_checkerboard_cal {
        (vec![], None)
    } else {
        eyre::bail!(
            "No --checkerboard-cal-dir given and --force-allow-no-checkerboard-cal not set."
        );
    };

    let num_cameras = cam_info_rows.len();
    assert_eq!(num_cameras, camns.len());

    let data2d_rows = {
        // Read data2d_distorted to memory.
        let data_fname = archive
            .path_starter()
            .join(braid_types::DATA2D_DISTORTED_CSV_FNAME);
        let mut rdr = braidz_parser::open_maybe_gzipped(data_fname)?;
        let mut buf = Vec::new();
        rdr.read_to_end(&mut buf)?;
        read_data2d(buf.as_slice())?
    };

    // In this scope, we collect points for calibration.
    // Build visibility and observations matrices as nalgebra DMatrix
    let (visibility, observations) = {
        let mut observations = vec![];
        let mut visibility: Vec<bool> = vec![];
        let mut num_points = 0;
        let mut by_camn = BTreeMap::new();
        let mut by_n_pts = BTreeMap::new();

        // Iterate over frames
        for frame_rows in group_by_frame(data2d_rows).into_iter() {
            // Although at least 3 cameras per 3D point are needed to be useful
            // to MCSC, we can still optimize using bundle adjustment with less.
            // So we take everything we can get here (not just cases where we
            // have at least three cameras).
            let this_camns: Vec<i64> = frame_rows.iter().map(|r| r.camn).collect();
            let gx: Vec<f64> = frame_rows.iter().map(|r| r.x).collect();
            let gy: Vec<f64> = frame_rows.iter().map(|r| r.y).collect();
            let mut this_point_n_cams = 0;
            for camn in camns.iter() {
                let idx = this_camns.iter().position(|x| x == camn);
                if let Some(idx) = idx {
                    visibility.push(true);
                    observations.push(gx[idx]);
                    observations.push(gy[idx]);
                    observations.push(1.0);

                    let cam_entry = by_camn.entry(camn).or_insert(0usize);
                    *cam_entry += 1;
                    this_point_n_cams += 1;
                } else {
                    // no data for this camn on this frame
                    visibility.push(false);
                    observations.push(-1.0);
                    observations.push(-1.0);
                    observations.push(-1.0);
                }
            }
            num_points += 1;
            let npt_entry = by_n_pts.entry(this_point_n_cams).or_insert(0usize);
            *npt_entry += 1;
        }

        println!("{num_points} points");
        println!("by camera id:");
        for (camn, num_points_per_cam) in by_camn.iter() {
            let cam_id = &camn2cam_id[*camn];
            println!(" {cam_id}: {num_points_per_cam}");
        }
        println!("by n points:");
        for (npt, count_per_num_pts) in by_n_pts.iter() {
            println!(" {npt}: {count_per_num_pts}");
        }

        // Convert to DMatrix
        // visibility: num_points x num_cameras -> transpose to num_cameras x num_points
        let vis_dm =
            nalgebra::DMatrix::<bool>::from_row_slice(num_points, num_cameras, &visibility)
                .transpose();
        // observations: num_points x (num_cameras*3) -> transpose to (num_cameras*3) x num_points
        let obs_dm =
            nalgebra::DMatrix::<f64>::from_row_slice(num_points, num_cameras * 3, &observations)
                .transpose();
        (vis_dm, obs_dm)
    };

    if visibility.ncols() == 0 {
        eyre::bail!("No points detected.");
    }

    // Apply use_nth_observation
    let (visibility, observations) = if use_nth_observation > 1 {
        let nth: usize = use_nth_observation.into();
        // Match Octave's 1:nth:end which uses ceiling division
        let n_pts = visibility.ncols().div_ceil(nth);
        let n_cams = visibility.nrows();

        let mut new_vis = nalgebra::DMatrix::<bool>::from_element(n_cams, n_pts, false);
        let mut new_obs = nalgebra::DMatrix::<f64>::zeros(n_cams * 3, n_pts);
        for j in 0..n_pts {
            for i in 0..n_cams {
                new_vis[(i, j)] = visibility[(i, j * nth)];
                for r in 0..3 {
                    new_obs[(i * 3 + r, j)] = observations[(i * 3 + r, j * nth)];
                }
            }
        }
        (new_vis, new_obs)
    } else {
        (visibility, observations)
    };

    println!(
        "MCSC native: {} cameras, {} points",
        num_cameras,
        visibility.ncols()
    );

    let undo_radial = radfiles.len() == num_cameras;

    let radfiles_clone = radfiles.clone();
    let mcsc_input = mcsc_native::McscInput {
        id_mat: visibility.clone(),
        points: observations.clone(),
        res: res.clone(),
        radfiles,
        camera_names: camera_order.clone(),
    };

    let cfg = McscCfg {
        undo_radial,
        do_bundle_adjustment: opt.do_mcsc_bundle_adjustment,
        ..Default::default()
    };

    let mcsc_result = mcsc_native::run_mcsc(mcsc_input, cfg)?;

    // Convert results to flydra format
    let input_str = opt.input.as_str();
    let input_base_name = input_str
        .strip_suffix(".braidz")
        .ok_or_else(|| eyre::eyre!("expected input filename to end with '.braidz'."))?;

    // Connect to rerun prior to running Octave.

    let rerun_url = if let Some(socket_addr_str) = opt.rerun {
        tracing::warn!("'--rerun' CLI argument is deprecated in favor of '--rerun-url'.");
        if opt.rerun_url.is_some() {
            eyre::bail!("Cannot set both rerun and rerun_url CLI args.");
        }
        use std::net::ToSocketAddrs;
        let mut addrs_iter = socket_addr_str.to_socket_addrs()?;
        let socket_addr = addrs_iter.next().unwrap();
        Some(format!("rerun+http://{socket_addr}/proxy"))
    } else {
        opt.rerun_url
    };

    #[cfg(feature = "with-rerun")]
    let rec = if let Some(rerun_url) = rerun_url {
        let re_version = re_sdk::build_info().version;
        tracing::info!("Streaming data to rerun {re_version} at {rerun_url}");
        Some(
            re_sdk::RecordingStreamBuilder::new(env!["CARGO_PKG_NAME"])
                .connect_grpc_opts(rerun_url)?,
        )
    } else {
        None
    };

    #[cfg(not(feature = "with-rerun"))]
    if rerun_url.is_some() {
        eyre::bail!("rerun URL specified but binary not compiled with `with-rerun` feature.");
    };

    // Convert mcsc_result directly to FlydraMultiCameraSystem without file I/O
    let (mcsc_system, points4cals) = {
        let mut cameras = Vec::new();
        let n_cams = camera_order.len();
        assert_eq!(n_cams, mcsc_result.projection_matrices.len());

        for (i, cam_id) in camera_order.iter().enumerate() {
            let pmat = &mcsc_result.projection_matrices[i];
            let resolution = res[i];

            // Build non_linear_parameters
            let non_linear = if undo_radial {
                // Use the radfile parameters
                let (k_matrix, kc) = &radfiles_clone[i];
                FlydraDistortionModel {
                    fc1: k_matrix[(0, 0)],
                    fc2: k_matrix[(1, 1)],
                    cc1: k_matrix[(0, 2)],
                    cc2: k_matrix[(1, 2)],
                    k1: kc[0],
                    k2: kc[1],
                    p1: kc[2],
                    p2: kc[3],
                    k3: 0.0,
                    alpha_c: 0.0,
                    fc1p: None,
                    fc2p: None,
                    cc1p: None,
                    cc2p: None,
                }
            } else {
                // Use linear model from projection matrix
                FlydraDistortionModel::linear(pmat)
            };

            cameras.push(SingleCameraCalibration {
                cam_id: cam_id.clone(),
                calibration_matrix: *pmat,
                resolution: (resolution[0], resolution[1]),
                scale_factor: None,
                non_linear_parameters: non_linear,
            });
        }

        let mut cams = BTreeMap::new();
        for orig_cam in cameras.iter() {
            let epsilon = 1e2; // MCSC may produce large skew
            let (name, cam) = flydra_mvg::from_flydra_with_limited_skew(orig_cam, epsilon)?;
            cams.insert(name, cam);
        }
        (
            flydra_mvg::FlydraMultiCameraSystem::new(cams, None),
            mcsc_result.points4cal.clone(),
        )
    };

    let multi_cam_system = if !opt.no_bundle_adjustment {
        if true {
            // There is some kind of matrix indexing bug when not all 3d points
            // are visible from all cameras. Need to fix this.
            todo!("bundle adjustment code needs to be fixed");
        }
        let model_type = opt.bundle_adjustment_model;
        let isrc = opt.bundle_adjustment_intrinsics_source;

        println!("Performing bundle adjustment {model_type:?} {isrc:?}");

        // Create BundleAdjuster
        let (visibility, observations, ba, start_ba_system) = {
            // `visibility` is MxN where M is num cameras and N is num 3d world points.
            assert_eq!(
                visibility.nrows(),
                mcsc_system.system().cams_by_name().len()
            );
            assert_eq!(visibility.nrows() * 3, observations.nrows());

            // Store each (u,v) observation pair. This will be reshaped later to a 2xN matrix.
            let mut observed: Vec<f64> = Vec::new();
            let mut cam_idx = Vec::new();
            let mut pt_idx = Vec::new();

            let mut point_locs: std::collections::BTreeMap<usize, [f64; 3]> = Default::default();

            for i in 0..visibility.nrows() {
                let qq = &points4cals[i];

                let obs_start_idx = i * 3;
                for j in 0..visibility.ncols() {
                    if visibility[(i, j)] {
                        let obs_u = observations[(obs_start_idx, j)];
                        let obs_v = observations[(obs_start_idx + 1, j)];
                        observed.push(obs_u);
                        observed.push(obs_v);
                        cam_idx.push(i.try_into().unwrap());
                        pt_idx.push(j);
                        println!("cam {i} pt {j}: {obs_u:.2}, {obs_v:.2}");

                        let xyz = [qq[(j, 0)], qq[(j, 1)], qq[(j, 2)]];
                        let prev_xyz = point_locs.entry(j).or_insert_with(|| xyz);
                        for ii in 0..3 {
                            if !approx::relative_eq!(xyz[ii], prev_xyz[ii]) {
                                eyre::bail!(
                                    "MCSC returned different 3D points for the same 3D point?"
                                );
                            }
                        }
                    }
                }
            }

            // Reshape observations to 2xN matrix.
            let observed = nalgebra::Matrix2xX::<f64>::from_column_slice(&observed);
            assert_eq!(observed.ncols(), cam_idx.len());
            assert_eq!(pt_idx.len(), cam_idx.len());

            // Use MCSC camera positions as initial camera guess.
            let mut cams0 = Vec::new();
            let mut cam_names = Vec::new();
            let mut cam_dims = Vec::new();
            let mut cams_by_name_ba: BTreeMap<_, _> = Default::default();
            // Use extrinsics from MCSC as starting point.
            // For intrinsics, it depends on our model_type.
            for (i, (name, mcsc_cam)) in mcsc_system.system().cams_by_name().iter().enumerate() {
                cam_names.push(name.clone());
                cam_dims.push((mcsc_cam.width(), mcsc_cam.height()));
                // Remove potential skew from calibration.
                let cam = mcsc_cam.as_ref();
                let extrin = cam.extrinsics().clone();
                let intrin = match &isrc {
                    BAIntrinsicsSource::CheckerboardCal => {
                        if let Some(ci) = &checkerboard_intrinsics {
                            // Use intrinsics from checkerboard cal.
                            ci[i].clone()
                        } else {
                            eyre::bail!("Required intrinsic parameters not present.");
                        }
                    }
                    BAIntrinsicsSource::MCSCNoSkew => {
                        let intrin_mcsc = cam.intrinsics();
                        // Average fx and fy to compute focal length "f".
                        let f = (intrin_mcsc.fx() + intrin_mcsc.fy()) / 2.0;
                        let skew = 0.0;
                        let cx = intrin_mcsc.cx();
                        let cy = intrin_mcsc.cy();
                        let distortion = intrin_mcsc.distortion.clone();

                        RosOpenCvIntrinsics::from_params_with_distortion(
                            f, skew, f, cx, cy, distortion,
                        )
                    }
                };
                let cam_fixed = cam_geom::Camera::new(intrin, extrin);
                cams0.push(cam_fixed.clone());
                cams_by_name_ba.insert(
                    name.clone(),
                    braid_mvg::Camera::new_from_cam_geom(
                        mcsc_cam.width(),
                        mcsc_cam.height(),
                        cam_fixed,
                    )?,
                );
            }
            let start_ba_system = flydra_mvg::FlydraMultiCameraSystem::new(cams_by_name_ba, None);

            let mut points0 = nalgebra::Matrix3xX::<f64>::zeros(visibility.ncols());
            let mut labels3d = Vec::with_capacity(visibility.ncols());
            for j in 0..visibility.ncols() {
                match point_locs.get(&j) {
                    Some(xyz) => {
                        for ii in 0..3 {
                            points0[(ii, j)] = xyz[ii];
                        }
                        labels3d.push(format!("{j}"));
                    }
                    None => {
                        eyre::bail!("Point {} not found in point_locs", j);
                    }
                }
            }

            // Print results of MCSC.
            println!("# Results of MCSC");
            print_reproj_and_params(&mcsc_system, &points0, &visibility, &observations)?;

            let optimize_points = true;
            let ba = bundle_adj::BundleAdjuster::new(
                observed,
                cam_idx,
                pt_idx,
                cam_names,
                #[cfg(feature = "with-rerun")]
                cam_dims,
                cams0,
                points0,
                labels3d,
                model_type,
                optimize_points,
                #[cfg(feature = "with-rerun")]
                rec,
            )?;
            (visibility, observations, ba, start_ba_system)
        };

        let residuals_pre = ba.residuals().unwrap();
        // dbg!(&residuals_pre);
        println!("# Results prior to bundle adjustment");
        print_reproj_and_params(&start_ba_system, ba.points(), &visibility, &observations)?;
        let (ba, report) = levenberg_marquardt::LevenbergMarquardt::new().minimize(ba);
        println!("{:?}", report);
        if !report.termination.was_successful() {
            eyre::bail!("Bundle adjustment did not succeed.");
        };
        // dbg!(ba.points().column(0).as_slice());
        let residuals_post = ba.residuals().unwrap();
        // dbg!(&residuals_post);
        println!(
            "pre: {}, post: {}",
            residuals_pre.abs().row_sum()[(0, 0)],
            residuals_post.abs().row_sum()[(0, 0)]
        );

        let mut cams_by_name = std::collections::BTreeMap::new();
        let mut cam_names = Vec::new();
        for ((name, old_cam), ba_cam) in mcsc_system
            .system()
            .cams_by_name()
            .iter()
            .zip(ba.cams().iter())
        {
            let e = ba_cam.extrinsics().clone();
            let i = ba_cam.intrinsics().clone();
            let cam = braid_mvg::Camera::new(old_cam.width(), old_cam.height(), e, i)?;
            cams_by_name.insert(name.clone(), cam);
            cam_names.push(name.clone());
        }
        let ba_system = flydra_mvg::FlydraMultiCameraSystem::new(cams_by_name, None);

        // Show reprojections with new system.
        println!(
            "# Results of bundle adjustment (model: {model_type:?}, intrinsics source: {isrc:?})"
        );
        print_reproj_and_params(&ba_system, ba.points(), &visibility, &observations)?;
        ba_system
    } else {
        mcsc_system
    };

    let xml_out_name = Utf8PathBuf::from(format!("{}-unaligned.xml", input_base_name));
    if std::fs::exists(&xml_out_name)? {
        // Remove existing file for test re-runs
        std::fs::remove_file(&xml_out_name)?;
    }
    let mut out_fd = DeleteUnfinished::new(&xml_out_name)
        .with_context(|| format!("While creating XML calibration output file {xml_out_name}"))?;
    multi_cam_system.to_flydra_xml(out_fd.inner())?;
    out_fd.close()?;

    Ok((xml_out_name, mcsc_result))
}

fn print_reproj_and_params(
    system: &flydra_mvg::FlydraMultiCameraSystem<f64>,
    points: &nalgebra::Matrix3xX<f64>,
    visibility: &nalgebra::DMatrix<bool>,
    observations: &nalgebra::DMatrix<f64>,
) -> Result<()> {
    println!(
        "CamId           name           std     mean  #inliers    fx      skew    fy      cx      cy      k1      k2      k3      p1      p2"
    );
    assert_eq!(system.len(), visibility.nrows());
    for (i, (name, cam)) in system.system().cams_by_name().iter().enumerate() {
        // for i in 0..visibility.nrows() {
        // let cam = &ba.cams()[i];
        let mut cam_dists = Vec::new();
        let obs_start_idx = i * 3;
        for j in 0..visibility.ncols() {
            if visibility[(i, j)] {
                let obs_u = observations[(obs_start_idx, j)];
                let obs_v = observations[(obs_start_idx + 1, j)];
                // let pt = ba.points().column(j);
                let pt = points.column(j);
                let pts = cam_geom::Points::new(pt.transpose());
                let predicted = cam.as_ref().world_to_pixel(&pts).data.transpose();
                let dx = obs_u - predicted.x;
                let dy = obs_v - predicted.y;
                let dist = (dx * dx + dy * dy).sqrt();
                cam_dists.push(dist);
                if false {
                    // if i == 0 && j == 0 {
                    dbg!("camera loaded from MCSC");
                    dbg!(cam);
                    dbg!(pt.as_slice());
                    dbg!(predicted);
                    dbg!((obs_u, obs_v));
                }
            }
        }
        let count = cam_dists.len();
        let mean = mean(&cam_dists);
        let std = std_dev(&cam_dists);
        println!(
            "{camid:>3}   {name:>20} {std:>8.2} {mean:>7.2}     {count:>5}   {fx:>7.2} {skew:>7.2} {fy:>7.2} {cx:>7.2} {cy:>7.2} {k1:>7.2} {k2:>7.2} {k3:>7.2} {p1:>7.2} {p2:>7.2}",
            camid = i + 1,
            fx = cam.intrinsics().fx(),
            skew = cam.intrinsics().skew(),
            fy = cam.intrinsics().fy(),
            cx = cam.intrinsics().cx(),
            cy = cam.intrinsics().cy(),
            k1 = cam.intrinsics().distortion.opencv_vec()[0],
            k2 = cam.intrinsics().distortion.opencv_vec()[1],
            p1 = cam.intrinsics().distortion.opencv_vec()[2],
            p2 = cam.intrinsics().distortion.opencv_vec()[3],
            k3 = cam.intrinsics().distortion.opencv_vec()[4],
        );
    }
    Ok(())
}

/// Delete any unfinished file unless close() is called.
pub(crate) struct DeleteUnfinished {
    inner: std::fs::File,
    path: Utf8PathBuf,
    do_remove_file: bool,
}

impl Drop for DeleteUnfinished {
    fn drop(&mut self) {
        if self.do_remove_file {
            std::fs::remove_file(&self.path).unwrap();
            self.do_remove_file = false;
        }
    }
}

impl DeleteUnfinished {
    pub(crate) fn new<P: AsRef<Utf8Path>>(p: P) -> Result<Self> {
        let path = Utf8PathBuf::from(p.as_ref());
        let inner = std::fs::File::create_new(&path)?;
        Ok(Self {
            inner,
            path,
            do_remove_file: true,
        })
    }

    pub(crate) fn inner(&mut self) -> &mut std::fs::File {
        &mut self.inner
    }

    pub(crate) fn close(mut self) -> Result<()> {
        use std::io::Write;
        self.inner.flush()?;
        self.do_remove_file = false;
        Ok(())
    }
}
