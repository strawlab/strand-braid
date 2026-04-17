use super::*;
use camino::{Utf8Path, Utf8PathBuf};
use eyre::WrapErr;
use eyre::{self, Result};
use mcsc_structs::{DatMat, RadFile};
use mcsc_structs::{McscCfg, McscConfigDir};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read},
    net::ToSocketAddrs,
    path::Path,
};

pub(crate) fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> io::Result<()> {
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

/// Run MCSC calibration by calling the external Octave implementation.
///
/// This is retained only for comparison testing against the native Rust
/// port `braidz_mcsc`.  It requires `octave-cli` to be installed and
/// the `MCSC_ROOT` environment variable (or the bundled MCSC sources)
/// to locate `gocal.m`.
pub(crate) fn braidz_mcsc_octave(opt: Cli) -> Result<Utf8PathBuf> {
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

    let radfiles = if let Some(checkerboard_cal_dir) = &opt.checkerboard_cal_dir {
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

        radfiles
    } else if opt.force_allow_no_checkerboard_cal {
        vec![]
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
        do_bundle_adjustment: opt.do_mcsc_bundle_adjustment,
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

    let input_str = opt.input.as_str();
    let input_base_name = input_str
        .strip_suffix(".braidz")
        .ok_or_else(|| eyre::eyre!("expected input filename to end with '.braidz'."))?
        .to_string();
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

    mcsc_data.save_to_path(&out_dir_name)?;

    println!("Saved to directory \"{out_dir_name}\".");

    let resultdir = camino::absolute_utf8(out_dir_name.join("result"))?;
    copy_dir_all(&out_dir_name, &resultdir)?;

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
    braidz_mcsc_octave_raw(resultdir, input_base_name)
}

/// Run MCSC calibration by calling the external Octave implementation.
///
/// This is retained only for comparison testing against the native Rust
/// port `braidz_mcsc`.  It requires `octave-cli` to be installed and
/// the `MCSC_ROOT` environment variable (or the bundled MCSC sources)
/// to locate `gocal.m`.
pub(crate) fn braidz_mcsc_octave_raw(
    resultdir: Utf8PathBuf,
    input_base_name: String,
) -> Result<Utf8PathBuf> {
    let xml_out_name = Utf8PathBuf::from(format!("{}-unaligned.xml", input_base_name));
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

    let gocal_path = mcsc_base.join("MultiCamSelfCal/gocal.m");

    // If we are going to fail when we create the output XML file, fail early by
    // creating the file now, rather than waiting for Octave to finish.
    let mut out_fd = DeleteUnfinished::new(&xml_out_name)
        .with_context(|| format!("While creating XML calibration output file {xml_out_name}"))?;

    let config_arg = format!("--config={resultdir}");
    let args = vec![gocal_path.as_os_str(), config_arg.as_ref()];
    let current_dir = gocal_path.parent().unwrap();
    const PROGRAM: &str = "octave-cli";

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
        eprintln!(
            "Failed to run Octave for MCSC calibration. \
        Please check that {PROGRAM} is installed. Args: {args:?}"
        );
        eyre::bail!("octave failed");
    }

    println!("Octave MCSC completed.");

    // Load initial guess of camera positions and 3D world points from MCSC results.
    let mcsc_system = {
        // This loads the cameras, but doesn't enforce low skew.
        let flydra_mvg::McscDirData {
            cameras,
            points4cals: _,
        } = flydra_mvg::read_mcsc_dir::<f64, _>(&resultdir, false)
            .with_context(|| format!("while reading calibration at {resultdir}"))?;

        // Here we limit the skew (although epsilon is large).
        let mut cams = BTreeMap::new();
        for orig_cam in cameras.iter() {
            let epsilon = 1e2;
            let (name, cam) = flydra_mvg::from_flydra_with_limited_skew(orig_cam, epsilon)?;
            cams.insert(name, cam);
        }
        flydra_mvg::FlydraMultiCameraSystem::new(cams, None)
    };

    mcsc_system.to_flydra_xml(out_fd.inner())?;
    out_fd.close()?;

    Ok(xml_out_name)
}
