use bundle_adj::CameraModelType;
use camino::{Utf8Path, Utf8PathBuf};
use clap::Parser;
use eyre::{self, Context, Result};
use levenberg_marquardt::LeastSquaresProblem;
use opencv_ros_camera::RosOpenCvIntrinsics;
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::{self, Read},
    net::ToSocketAddrs,
    path::Path,
};

use mcsc_structs::{DatMat, McscCfg, McscConfigDir, RadFile};

#[derive(Parser, Default)]
struct Cli {
    /// Input braidz filename.
    #[arg(long)]
    input: Utf8PathBuf,

    /// Input directory to be searched for YAML calibration files from
    /// checkerboard calibration. (Typically
    /// "~/.config/strand-cam/camera_info").
    #[arg(long)]
    checkerboard_cal_dir: Option<Utf8PathBuf>,

    #[arg(long)]
    force_allow_no_checkerboard_cal: bool,

    /// Rather than using each frame, use only 1/N of them.
    #[arg(long)]
    use_nth_observation: Option<u16>,

    /// If set, keep the intermediate MCSC calibration directory.
    #[arg(long)]
    keep: bool,

    /// Do not perform bundle adjustment
    #[arg(long)]
    no_bundle_adjustment: bool,

    /// Type of bundle adjustment to perform
    #[arg(long, value_enum, default_value_t)]
    bundle_adjustment_model: CameraModelType,

    /// Source of camera intrinsics when initializing bundle adjustment
    #[arg(long, value_enum, default_value_t)]
    bundle_adjustment_intrinsics_source: BAIntrinsicsSource,

    #[cfg(feature = "with-rerun")]
    /// Log data to rerun viewer at this socket address. (The typical address is
    /// "127.0.0.1:9876".) DEPRECATED. Use `rerun_url` instead.
    #[arg(long, hide = true)]
    rerun: Option<String>,

    #[cfg(not(feature = "with-rerun"))]
    /// Disabled. To enable, recompile with the `with-rerun` feature.
    #[arg(long, hide = true)]
    rerun: Option<String>,

    #[cfg(feature = "with-rerun")]
    /// Log data to rerun viewer at this URL. (A typical url is
    /// "rerun+http://127.0.0.1:9876/proxy\".)
    #[arg(long)]
    rerun_url: Option<String>,

    #[cfg(not(feature = "with-rerun"))]
    /// Disabled. To enable, recompile with the `with-rerun` feature.
    #[arg(long)]
    rerun_url: Option<String>,
}

#[derive(Debug, Clone, clap::ValueEnum, Default, PartialEq)]
enum BAIntrinsicsSource {
    #[default]
    MCSCNoSkew,
    CheckerboardCal,
}

fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> io::Result<()> {
    // modified from https://stackoverflow.com/a/65192210
    let entries: Vec<fs::DirEntry> =
        fs::read_dir(src)?.collect::<io::Result<Vec<fs::DirEntry>>>()?;
    fs::create_dir_all(&dst)?;
    for entry in entries.iter() {
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}

/// One row from `cam_info.csv`.
#[derive(serde::Deserialize)]
struct CamInfoRow {
    cam_id: String,
    camn: i64,
}

/// Read `cam_info.csv` from a `Read` source into a `Vec<CamInfoRow>`.
fn read_cam_info<R: Read>(reader: R) -> Result<Vec<CamInfoRow>> {
    let mut rdr = csv::Reader::from_reader(reader);
    let rows = rdr.deserialize().collect::<Result<Vec<CamInfoRow>, _>>()?;
    Ok(rows)
}

/// One row from `data2d_distorted.csv` (only the columns we need).
#[derive(serde::Deserialize)]
struct Data2dRow {
    frame: i64,
    camn: i64,
    x: f64,
    y: f64,
}

/// Read `data2d_distorted.csv` from a `Read` source, keeping only the columns
/// we need and discarding rows where `x` is NaN.
fn read_data2d<R: Read>(reader: R) -> Result<Vec<Data2dRow>> {
    let mut rdr = csv::Reader::from_reader(reader);
    let rows = rdr.deserialize().collect::<Result<Vec<Data2dRow>, _>>()?;
    Ok(rows.into_iter().filter(|r| !r.x.is_nan()).collect())
}

/// Group `Data2dRow` values by `frame`, preserving the order of first
/// appearance of each frame value (analogous to `partition_by_stable`).
fn group_by_frame(rows: Vec<Data2dRow>) -> Vec<Vec<Data2dRow>> {
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
fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        return f64::NAN;
    }
    values.iter().sum::<f64>() / values.len() as f64
}

/// Compute the sample standard deviation of a slice of `f64` values.
/// Returns `NaN` if the slice has fewer than `DDOF + 1` elements.
fn std_dev(values: &[f64]) -> f64 {
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
    let xml_out_name = braiz_mcsc(opt)?;
    println!("Unaligned calibration XML saved to {xml_out_name}");
    Ok(())
}

fn braiz_mcsc(opt: Cli) -> Result<Utf8PathBuf> {
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
        res.push(im.width() as usize);
        res.push(im.height() as usize);
        images.insert(cam_id, im);
    }
    let camns: Vec<i64> = cam_info_rows.iter().map(|r| r.camn).collect();
    for (camn, cam_id) in camns.iter().zip(camera_order.iter()) {
        camn2cam_id.insert(*camn, cam_id.clone());
    }

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

            radfiles.push(RadFile::new(&cam_info)?);

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

        let visibility = DatMat::new(num_points, num_cameras, visibility)?.transpose();
        let observations = DatMat::new(num_points, num_cameras * 3, observations)?.transpose();
        (visibility, observations)
    };

    if visibility.ncols() == 0 {
        eyre::bail!("No points detected.");
    }

    let undo_radial = radfiles.len() == num_cameras;

    let cfg = McscCfg {
        num_cameras,
        undo_radial,
        use_nth_observation,
    };

    let res = DatMat::new(num_cameras, 2, res)?;

    let mcsc_data = McscConfigDir {
        id_mat: visibility.clone().into(),
        radfiles,
        cfg,
        camera_order,
        res,
        points: observations.clone(),
    };

    #[expect(unused_variables)]
    let mut output_root_guard = None; // will cleanup on drop

    let input_str = opt
        .input
        .as_os_str()
        .to_str()
        .ok_or_else(|| eyre::eyre!("input filename is not valid unicode?"))?;
    let input_base_name = input_str
        .strip_suffix(".braidz")
        .ok_or_else(|| eyre::eyre!("expected input filename to end with '.braidz'."))?;
    let out_dir_name = if opt.keep {
        Utf8PathBuf::from(format!("{}.mcsc", input_base_name))
    } else {
        let output_root = tempfile::tempdir()?;
        let out_dir_name = Utf8Path::from_path(output_root.path()).unwrap().to_owned();
        #[expect(unused_assignments)]
        {
            output_root_guard = Some(output_root);
        }
        out_dir_name
    };
    let xml_out_name = Utf8PathBuf::from(format!("{}-unaligned.xml", input_base_name));

    mcsc_data.save_to_path(&out_dir_name)?;

    println!("Saved to directory \"{out_dir_name}\".");

    if std::fs::exists(&xml_out_name)? {
        eyre::bail!("XML calibration output file (\"{xml_out_name}\") exists. Will not overwrite.");
    }

    let (_mcsc_root, mcsc_base) = match std::env::var_os("MCSC_ROOT") {
        Some(v) => (None, std::path::PathBuf::from(v)),
        None => {
            // unpack MCSC into mcsc_root
            let mcsc_root = tempfile::tempdir()?;
            let mcsc_dir_name = std::path::PathBuf::from(mcsc_root.path());
            let mcsc_base = mcsc_structs::unpack_mcsc_into(&mcsc_dir_name)?;
            (Some(mcsc_root), mcsc_base)
        }
    };
    let mcsc_base = Utf8PathBuf::from_path_buf(mcsc_base).unwrap();

    let gocal_abs = mcsc_base.join("MultiCamSelfCal/gocal.m");

    let resultdir = out_dir_name.join("result");
    copy_dir_all(&out_dir_name, &resultdir)?;

    // Create output XML file prior to running Octave. This way, in case there
    // is a problem opening it, we don't wait for Octave to finish.
    let mut out_fd = DeleteUnfinished::new(&xml_out_name)
        .with_context(|| format!("While creating XML calibration output file {xml_out_name}"))?;

    // Connect to rerun prior to running Octave.

    let rerun_url = if let Some(socket_addr_str) = opt.rerun {
        tracing::warn!("'--rerun' CLI argument is deprecated in favor of '--rerun-url'.");
        if opt.rerun_url.is_some() {
            eyre::bail!("Cannot set both rerun and rerun_url CLI args.");
        }
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

    let config_arg = format!(
        "--config={resultdir}",
        resultdir = std::path::absolute(&resultdir)?.display()
    );
    let args = vec![gocal_abs.as_os_str(), config_arg.as_ref()];
    let current_dir = gocal_abs.parent().unwrap();
    const PROGRAM: &str = "octave";

    if !std::process::Command::new(PROGRAM)
        .args(["--version"])
        .status()
        .with_context(|| format!("While checking version of {PROGRAM:?}"))?
        .success()
    {
        eyre::bail!("octave version check failed");
    }

    if !std::process::Command::new(PROGRAM)
        .args(&args)
        .current_dir(current_dir)
        .status()
        .with_context(|| {
            format!("While running {PROGRAM:?} with args {args:?} in dir {current_dir:?}")
        })?
        .success()
    {
        eyre::bail!("octave failed");
    }

    println!("Octave MCSC completed.");

    // Do our own bundle adjustment here.

    // Load initial guess of camera positions and 3D world points from MCSC results.
    let (mcsc_system, points4cals) = {
        let flydra_mvg::McscDirData {
            cameras,
            points4cals,
        } = flydra_mvg::read_mcsc_dir::<f64, _>(&resultdir)
            .with_context(|| format!("while reading calibration at {resultdir}"))?;
        let mut cams = BTreeMap::new();
        for orig_cam in cameras.iter() {
            let epsilon = 1e2;
            let (name, cam) = flydra_mvg::from_flydra_with_limited_skew(orig_cam, epsilon)?;
            cams.insert(name, cam);
        }

        (
            flydra_mvg::FlydraMultiCameraSystem::new(cams, None),
            points4cals,
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
            // Downsample data if needed.
            let (visibility, observations) = if use_nth_observation == 1 {
                (visibility, observations)
            } else {
                // observations.save("orig.dat")?;
                let use_nth_observation: usize = use_nth_observation.into();
                let ncams = visibility.nrows();
                let npts = visibility.ncols() / use_nth_observation;

                let mut v2_vals = Vec::with_capacity(ncams * npts);
                for i in 0..ncams {
                    for j in 0..npts {
                        v2_vals.push(visibility[(i, j * use_nth_observation)]);
                    }
                }

                let mut o2_vals = Vec::with_capacity(ncams * npts * 3);
                for j in 0..npts {
                    for i in 0..ncams {
                        o2_vals.push(observations[(i * 3, j * use_nth_observation)]);
                        o2_vals.push(observations[(i * 3 + 1, j * use_nth_observation)]);
                        o2_vals.push(observations[(i * 3 + 2, j * use_nth_observation)]);
                    }
                }

                let v2 = DatMat::new(ncams, npts, v2_vals)?;
                let o2 = DatMat::new(npts, ncams * 3, o2_vals)?.transpose();
                (v2, o2)
            };

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
                                todo!(
                                    "return error: MCSC returned different 3D points for the same 3D point?"
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
                        // let fx = intrin_mcsc.fx();
                        // let fy = intrin_mcsc.fy();
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
                        todo!("return error");
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
    multi_cam_system.to_flydra_xml(out_fd.inner())?;
    out_fd.close()?;

    Ok(xml_out_name)
}

fn print_reproj_and_params(
    system: &flydra_mvg::FlydraMultiCameraSystem<f64>,
    points: &nalgebra::Matrix3xX<f64>,
    visibility: &DatMat<bool>,
    observations: &DatMat<f64>,
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
struct DeleteUnfinished {
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
    fn new<P: AsRef<Utf8Path>>(p: P) -> Result<Self> {
        let path = Utf8PathBuf::from(p.as_ref());
        let inner = std::fs::File::create_new(&path)?;
        Ok(Self {
            inner,
            path,
            do_remove_file: true,
        })
    }

    fn inner(&mut self) -> &mut std::fs::File {
        &mut self.inner
    }

    fn close(mut self) -> Result<()> {
        use std::io::Write;
        self.inner.flush()?;
        self.do_remove_file = false;
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use ::zip::ZipArchive;
    use approx::assert_relative_eq;
    use eyre::Result;
    use std::io::Seek;

    use super::*;

    #[test]
    fn test_mean_basic() {
        assert_relative_eq!(mean(&[1.0, 2.0, 3.0, 4.0, 5.0]), 3.0);
        assert_relative_eq!(mean(&[0.0, 10.0]), 5.0);
        assert_relative_eq!(mean(&[42.0]), 42.0);
    }

    #[test]
    fn test_mean_empty() {
        assert!(mean(&[]).is_nan());
    }

    #[test]
    fn test_std_dev_basic() {
        // Sample std dev of [1, 2, 3] == 1.0
        assert_relative_eq!(std_dev(&[1.0, 2.0, 3.0]), 1.0);
        // Two identical values → std dev == 0
        assert_relative_eq!(std_dev(&[3.0, 3.0]), 0.0, epsilon = 1e-10);
    }

    #[test]
    fn test_std_dev_insufficient_data() {
        assert!(std_dev(&[]).is_nan());
        assert!(std_dev(&[1.0]).is_nan());
    }

    const URL_BASE: &str = "https://strawlab-cdn.com/assets/";

    fn unpack_zip_into<R: Read + Seek>(
        mut archive: ZipArchive<R>,
        mcsc_dir_name: &Utf8Path,
    ) -> Result<()> {
        fs::create_dir_all(&mcsc_dir_name).unwrap();
        for i in 0..archive.len() {
            let mut file = archive.by_index(i).unwrap();
            let outpath = match file.enclosed_name() {
                Some(path) => Utf8PathBuf::from_path_buf(path.to_owned()).unwrap(),
                None => continue,
            };
            let outpath = mcsc_dir_name.join(outpath);

            if (*file.name()).ends_with('/') {
                fs::create_dir_all(&outpath).unwrap();
            } else {
                if let Some(p) = outpath.parent() {
                    if !p.exists() {
                        fs::create_dir_all(p).unwrap();
                    }
                }
                let mut outfile = fs::File::create(&outpath).unwrap();
                io::copy(&mut file, &mut outfile).unwrap();
            }
        }
        Ok(())
    }

    #[test]
    #[ignore] // Ignore normally because it is slow and requires Octave.
    fn test_braiz_mcsc() -> Result<()> {
        const FNAME: &str = "braidz-mcsc-cal-test-data.zip";
        const SHA256SUM: &str = "f0043d73749e9c2c161240436eca9101a4bf71cf81785a45b04877fe7ae6d33e";

        download_verify::download_verify(
            format!("{}/{}", URL_BASE, FNAME).as_str(),
            FNAME,
            &download_verify::Hash::Sha256(SHA256SUM.into()),
        )
        .unwrap();

        let data_root = tempfile::tempdir()?;
        let data_root_dir_name =
            Utf8PathBuf::from_path_buf(std::path::PathBuf::from(data_root.path())).unwrap();

        let rdr = std::fs::File::open(FNAME)?;
        let cal_data_archive = ZipArchive::new(rdr)?;

        unpack_zip_into(cal_data_archive, &data_root_dir_name)?;

        let input = data_root_dir_name.join("20241017_164418.braidz");
        let checkerboard_cal_dir = Some(data_root_dir_name.join("checkerboard-cal-results"));

        let opt = Cli {
            input,
            checkerboard_cal_dir,
            no_bundle_adjustment: true,
            ..Default::default()
        };
        let _xml_out_name = braiz_mcsc(opt)?;
        // TODO: check that the calibration makes sense...
        Ok(())
    }

    #[test]
    fn test_braiz_mcsc_skew() -> Result<()> {
        const FNAME: &str = "braidz-mcsc-skew-cal-test-data.zip";
        const SHA256SUM: &str = "82294b0b9fa2a0d6f43bb410e133722abffa55bf3abab934dbb165791a3f334c";

        let local_fname = format!("scratch/{FNAME}");

        download_verify::download_verify(
            format!("{}/{}", URL_BASE, FNAME).as_str(),
            &local_fname,
            &download_verify::Hash::Sha256(SHA256SUM.into()),
        )
        .unwrap();

        let data_root = tempfile::tempdir()?;
        let data_root_dir_name =
            Utf8PathBuf::from_path_buf(std::path::PathBuf::from(data_root.path())).unwrap();

        let rdr = std::fs::File::open(&local_fname)?;
        let cal_data_archive = ZipArchive::new(rdr)?;

        unpack_zip_into(cal_data_archive, &data_root_dir_name)?;

        let input = data_root_dir_name.join("20250131_192425.braidz");
        let checkerboard_cal_dir = Some(data_root_dir_name.join("camera_info"));

        let opt = Cli {
            input,
            checkerboard_cal_dir,
            use_nth_observation: Some(10),
            keep: true,
            no_bundle_adjustment: true,
            ..Default::default()
        };
        let _xml_out_name = braiz_mcsc(opt)?;
        // TODO: check that the calibration makes sense...
        Ok(())
    }
}
