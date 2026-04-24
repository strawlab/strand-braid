use approx::assert_relative_eq;
use camino::{Utf8Path, Utf8PathBuf};
use eyre::{Context, Result};
use mcsc_structs::DatMat;
use std::collections::BTreeMap;
use std::io::Read;
use std::io::Seek;
use zip::ZipArchive;

use crate::{Cli, braidz_mcsc, group_by_frame, mean, read_cam_info, read_data2d, std_dev};

#[cfg(feature = "with-octave")]
use crate::with_octave::{braidz_mcsc_octave, braidz_mcsc_octave_raw};

const ENV_VAR_NAME: &str = "BRAIDZ_MCSC_SAVE_TEST_OUTPUT";

/// Check calibration quality by computing reprojection distances
fn check_calibration_quality_from_xml(
    xml_path: &Utf8Path,
    input_braidz: &Utf8Path,
    #[cfg(feature = "with-octave")] only_load: bool,
) -> Result<()> {
    #[cfg(not(feature = "with-octave"))]
    let only_load = false;

    if only_load {
        let rdr = std::fs::File::open(xml_path)?;

        let _recon: flydra_mvg::flydra_xml_support::FlydraReconstructor<f64> =
            serde_xml_rs::from_reader(rdr)?;
        return Ok(());
    }

    // Load calibration from XML file
    let loaded_system = flydra_mvg::FlydraMultiCameraSystem::from_path(&xml_path, false)
        .with_context(|| format!("while attempting to read calibration file {xml_path}"))?;

    // Reload observations from braidz file
    let mut archive = zip_or_dir::ZipDirArchive::auto_from_path(input_braidz)?;

    let cam_info_rows = {
        let data_fname = archive.path_starter().join(braid_types::CAM_INFO_CSV_FNAME);
        let mut rdr = braidz_parser::open_maybe_gzipped(data_fname)?;
        let mut buf = Vec::new();
        rdr.read_to_end(&mut buf)?;
        read_cam_info(buf.as_slice())?
    };

    let camns: Vec<i64> = cam_info_rows.iter().map(|r| r.camn).collect();
    let mut camn2cam_id = BTreeMap::new();
    let mut camera_order = vec![];
    for row in cam_info_rows.iter() {
        camera_order.push(row.cam_id.clone());
    }
    for (camn, cam_id) in camns.iter().zip(camera_order.iter()) {
        camn2cam_id.insert(*camn, cam_id.clone());
    }

    let data2d_rows = {
        let data_fname = archive
            .path_starter()
            .join(braid_types::DATA2D_DISTORTED_CSV_FNAME);
        let mut rdr = braidz_parser::open_maybe_gzipped(data_fname)?;
        let mut buf = Vec::new();
        rdr.read_to_end(&mut buf)?;
        read_data2d(buf.as_slice())?
    };

    let num_cameras = cam_info_rows.len();

    // Rebuild visibility and observations matrices
    let (visibility, observations) = {
        let mut observations = vec![];
        let mut visibility: Vec<bool> = vec![];
        let mut num_points = 0;

        for frame_rows in group_by_frame(data2d_rows).into_iter() {
            let this_camns: Vec<i64> = frame_rows.iter().map(|r| r.camn).collect();
            let gx: Vec<f64> = frame_rows.iter().map(|r| r.x).collect();
            let gy: Vec<f64> = frame_rows.iter().map(|r| r.y).collect();
            for camn in camns.iter() {
                let idx = this_camns.iter().position(|x| x == camn);
                if let Some(idx) = idx {
                    visibility.push(true);
                    observations.push(gx[idx]);
                    observations.push(gy[idx]);
                    observations.push(1.0);
                } else {
                    visibility.push(false);
                    observations.push(-1.0);
                    observations.push(-1.0);
                    observations.push(-1.0);
                }
            }
            num_points += 1;
        }

        let visibility = DatMat::new(num_points, num_cameras, visibility)?.transpose();
        let observations = DatMat::new(num_points, num_cameras * 3, observations)?.transpose();
        (visibility, observations)
    };

    // Triangulate 3D points from observations using the calibration, then
    // compute reprojection errors
    println!(
        "\nCalibration quality check ({nobs} observation points):",
        nobs = visibility.ncols()
    );
    let mut all_reproj_dists = Vec::new();
    let mut total_observations = 0;

    for (i, (cam_name, cam)) in loaded_system.system().cams_by_name().iter().enumerate() {
        let mut cam_dists = Vec::new();
        let obs_start_idx = i * 3;

        // For each 3D point, triangulate from all visible cameras and
        // compute reprojection for this camera
        for j in 0..visibility.ncols() {
            // Collect all camera observations for this point to triangulate
            let mut obs_for_point = Vec::new();

            for (k, (other_name, _other_cam)) in
                loaded_system.system().cams_by_name().iter().enumerate()
            {
                if visibility[(k, j)] {
                    let obs_start = k * 3;
                    let u = observations[(obs_start, j)];
                    let v = observations[(obs_start + 1, j)];

                    let distorted_pixel = braid_mvg::DistortedPixel {
                        coords: nalgebra::Point2::new(u, v),
                    };
                    obs_for_point.push((other_name.clone(), distorted_pixel));
                }
            }

            // Need at least 2 cameras to triangulate
            if obs_for_point.len() >= 2 && visibility[(i, j)] {
                // Triangulate using the system's built-in method
                let world_coord = loaded_system.find3d_distorted(&obs_for_point)?;
                let pt_3d = world_coord.point();

                // Compute reprojection error for this camera
                let obs_u = observations[(obs_start_idx, j)];
                let obs_v = observations[(obs_start_idx + 1, j)];
                let predicted = cam.project_3d_to_distorted_pixel(&pt_3d);
                let dx = obs_u - predicted.coords.x;
                let dy = obs_v - predicted.coords.y;
                let dist = (dx * dx + dy * dy).sqrt();

                cam_dists.push(dist);
            }
        }

        if cam_dists.is_empty() {
            println!("  Camera {cam_name}: no valid observations");
            continue;
        }

        let mean_dist = mean(&cam_dists);
        println!(
            "  Camera {cam_name}: mean reprojection distance = {mean_dist:.2} pixels ({nobs} observations)",
            nobs = cam_dists.len()
        );
        all_reproj_dists.push(mean_dist);
        total_observations += cam_dists.len();

        // Assert reasonable reprojection error (more lenient since we're using simple triangulation)
        assert!(
            mean_dist < 5.0,
            "Camera {cam_name} has excessive reprojection error: {mean_dist:.2} pixels",
        );
    }

    if all_reproj_dists.is_empty() {
        println!("\nNo valid observations for quality check.");
        return Ok(());
    }

    let overall_mean = mean(&all_reproj_dists);
    println!(
        "  Overall mean reprojection distance = {overall_mean:.2} pixels ({total_observations} total observations)"
    );
    assert!(
        overall_mean < 3.0,
        "Overall mean reprojection error too high: {overall_mean:.2} pixels"
    );

    Ok(())
}

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
    std::fs::create_dir_all(mcsc_dir_name).unwrap();
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).unwrap();
        let outpath = match file.enclosed_name() {
            Some(path) => Utf8PathBuf::from_path_buf(path.to_owned()).unwrap(),
            None => continue,
        };
        let outpath = mcsc_dir_name.join(outpath);

        if (*file.name()).ends_with('/') {
            std::fs::create_dir_all(&outpath).unwrap();
        } else {
            if let Some(p) = outpath.parent()
                && !p.exists()
            {
                std::fs::create_dir_all(p).unwrap();
            }
            let mut outfile = std::fs::File::create(&outpath).unwrap();
            std::io::copy(&mut file, &mut outfile).unwrap();
        }
    }
    Ok(())
}

#[cfg(feature = "with-octave")]
#[test]
#[ignore] // Ignore normally because it is slow and requires Octave.
fn test_braidz_octave_mcsc_slow() -> Result<()> {
    const FNAME: &str = "braidz-mcsc-cal-test-data.zip";
    const SHA256SUM: &str = "f0043d73749e9c2c161240436eca9101a4bf71cf81785a45b04877fe7ae6d33e";
    let dest = format!("scratch/{FNAME}");

    download_verify::download_verify(
        format!("{}/{}", URL_BASE, FNAME).as_str(),
        &dest,
        &download_verify::Hash::Sha256(SHA256SUM.into()),
    )
    .unwrap();

    let data_root = tempfile::tempdir()?;
    let data_root_dir_name =
        Utf8PathBuf::from_path_buf(std::path::PathBuf::from(data_root.path())).unwrap();

    // Potentially do not delete temporary directory
    let save_output = match std::env::var_os(ENV_VAR_NAME) {
        Some(v) => &v != "0",
        None => false,
    };

    if save_output {
        std::mem::forget(data_root); // do not drop it, so do not delete it
    }

    let rdr = std::fs::File::open(&dest)?;
    let cal_data_archive = ZipArchive::new(rdr)?;

    unpack_zip_into(cal_data_archive, &data_root_dir_name)?;

    let input = data_root_dir_name.join("20241017_164418.braidz");
    let checkerboard_cal_dir = Some(data_root_dir_name.join("checkerboard-cal-results"));

    let opt = Cli {
        input: input.clone(),
        checkerboard_cal_dir,
        no_bundle_adjustment: true,
        ..Default::default()
    };
    let xml_out_name = braidz_mcsc_octave(opt)?;

    // Check that the calibration makes sense
    check_calibration_quality_from_xml(
        &xml_out_name,
        &input,
        #[cfg(feature = "with-octave")]
        true,
    )?;

    Ok(())
}

#[test]
#[ignore] // Ignore normally because it is slow.
fn test_braidz_mcsc_slow() -> Result<()> {
    const FNAME: &str = "braidz-mcsc-cal-test-data.zip";
    const SHA256SUM: &str = "f0043d73749e9c2c161240436eca9101a4bf71cf81785a45b04877fe7ae6d33e";
    let dest = format!("scratch/{FNAME}");

    download_verify::download_verify(
        format!("{}/{}", URL_BASE, FNAME).as_str(),
        &dest,
        &download_verify::Hash::Sha256(SHA256SUM.into()),
    )
    .unwrap();

    let data_root = tempfile::tempdir()?;
    let data_root_dir_name =
        Utf8PathBuf::from_path_buf(std::path::PathBuf::from(data_root.path())).unwrap();

    // Potentially do not delete temporary directory
    let save_output = match std::env::var_os(ENV_VAR_NAME) {
        Some(v) => &v != "0",
        None => false,
    };

    if save_output {
        std::mem::forget(data_root); // do not drop it, so do not delete it
    }

    let rdr = std::fs::File::open(&dest)?;
    let cal_data_archive = ZipArchive::new(rdr)?;

    unpack_zip_into(cal_data_archive, &data_root_dir_name)?;

    let input = data_root_dir_name.join("20241017_164418.braidz");
    let checkerboard_cal_dir = Some(data_root_dir_name.join("checkerboard-cal-results"));

    let opt = Cli {
        input: input.clone(),
        checkerboard_cal_dir,
        no_bundle_adjustment: true,
        ..Default::default()
    };
    let (xml_out_name, mcsc_result) = braidz_mcsc(opt)?;
    assert!(
        mcsc_result.mean_reproj_distance < 0.6,
        "Mean reprojection distance too high: {:.2} pixels",
        mcsc_result.mean_reproj_distance
    );

    // Check that the calibration makes sense
    check_calibration_quality_from_xml(
        &xml_out_name,
        &input,
        #[cfg(feature = "with-octave")]
        false,
    )?;

    Ok(())
}

#[cfg(feature = "with-octave")]
#[ignore] // Ignore normally because it is slow and requires Octave.
#[test]
fn test_braidz_octave_20cams() -> Result<()> {
    const FNAME: &str = "sample_calibration_data.braidz.h5.recal.zip";
    const SHA256SUM: &str = "c713b90297db46bd0781fb60e95a0aff05f1d8e34ee504b44f8e55cc8ea80468";

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

    // Potentially do not delete temporary directory
    let save_output = match std::env::var_os(ENV_VAR_NAME) {
        Some(v) => &v != "0",
        None => false,
    };

    if save_output {
        std::mem::forget(data_root); // do not drop it, so do not delete it
    }

    let rdr = std::fs::File::open(&local_fname)?;
    let cal_data_archive = ZipArchive::new(rdr)?;

    unpack_zip_into(cal_data_archive, &data_root_dir_name)?;

    let _input = data_root_dir_name.join("multicamselfcal.cfg");

    let resultdir = camino::absolute_utf8(data_root_dir_name.join("multicamselfcal.mcsc/result"))?;
    crate::with_octave::copy_dir_all(&data_root_dir_name, &resultdir)?;

    let input_base_name = format!("{data_root_dir_name}/multicamselfcal");

    let xml_out_name = braidz_mcsc_octave_raw(resultdir, input_base_name)?;

    // This test data is MCSC input files, not a braidz file, so we can't use
    // check_calibration_quality. Just verify Octave MCSC completed successfully
    // by checking that the calibration output files exist.
    assert!(
        xml_out_name.exists(),
        "XML calibration output file should exist"
    );

    Ok(())
}

#[cfg(feature = "with-octave")]
#[test]
fn test_braidz_octave_mcsc_skew() -> Result<()> {
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

    // Potentially do not delete temporary directory
    let save_output = match std::env::var_os(ENV_VAR_NAME) {
        Some(v) => &v != "0",
        None => false,
    };

    if save_output {
        std::mem::forget(data_root); // do not drop it, so do not delete it
    }

    let rdr = std::fs::File::open(&local_fname)?;
    let cal_data_archive = ZipArchive::new(rdr)?;

    unpack_zip_into(cal_data_archive, &data_root_dir_name)?;

    let input = data_root_dir_name.join("20250131_192425.braidz");
    let checkerboard_cal_dir = Some(data_root_dir_name.join("camera_info"));

    let opt = Cli {
        input: input.clone(),
        checkerboard_cal_dir,
        use_nth_observation: Some(10),
        no_bundle_adjustment: true,
        ..Default::default()
    };
    let xml_out_name = braidz_mcsc_octave(opt)?;

    // Check that the calibration makes sense
    check_calibration_quality_from_xml(
        &xml_out_name,
        &input,
        #[cfg(feature = "with-octave")]
        true,
    )?;

    Ok(())
}

#[test]
fn test_braidz_mcsc_skew() -> Result<()> {
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

    // Potentially do not delete temporary directory
    let save_output = match std::env::var_os(ENV_VAR_NAME) {
        Some(v) => &v != "0",
        None => false,
    };

    if save_output {
        std::mem::forget(data_root); // do not drop it, so do not delete it
    }

    let rdr = std::fs::File::open(&local_fname)?;
    let cal_data_archive = ZipArchive::new(rdr)?;

    unpack_zip_into(cal_data_archive, &data_root_dir_name)?;

    let input = data_root_dir_name.join("20250131_192425.braidz");
    let checkerboard_cal_dir = Some(data_root_dir_name.join("camera_info"));

    let opt = Cli {
        input: input.clone(),
        checkerboard_cal_dir,
        use_nth_observation: Some(10),
        no_bundle_adjustment: true,
        ..Default::default()
    };
    let (xml_out_name, mcsc_result) = braidz_mcsc(opt)?;
    // Because we are supplying intrinsics ("known K"), including distortion,
    // MCSC returns only extrinsics and thus the reprojection residual is
    // typically a few pixels on real data; closing the gap to sub-pixel needs
    // the downstream bundle-adjustment pass. This threshold is therefore much
    // looser than the self-cal path would produce, but known-K output is usable
    // as a BA seed and crucially does not carry the spurious skew that the
    // self-cal path introduces.
    assert!(
        mcsc_result.mean_reproj_distance < 5.0,
        "Mean reprojection distance too high: {:.2} pixels",
        mcsc_result.mean_reproj_distance
    );

    // Check that the calibration makes sense.
    check_calibration_quality_from_xml(
        &xml_out_name,
        &input,
        #[cfg(feature = "with-octave")]
        false,
    )?;

    Ok(())
}

#[test]
fn test_braidz_mcsc_bundle_adjustment() -> Result<()> {
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

    // Potentially do not delete temporary directory
    let save_output = match std::env::var_os(ENV_VAR_NAME) {
        Some(v) => &v != "0",
        None => false,
    };

    if save_output {
        std::mem::forget(data_root); // do not drop it, so do not delete it
    }

    let rdr = std::fs::File::open(&local_fname)?;
    let cal_data_archive = ZipArchive::new(rdr)?;

    unpack_zip_into(cal_data_archive, &data_root_dir_name)?;

    let input = data_root_dir_name.join("20250131_192425.braidz");
    let checkerboard_cal_dir = Some(data_root_dir_name.join("camera_info"));

    let opt = Cli {
        input: input.clone(),
        checkerboard_cal_dir,
        use_nth_observation: Some(10),
        ..Default::default()
    };
    let (xml_out_name, mcsc_result) = braidz_mcsc(opt)?;

    assert!(
        mcsc_result.mean_reproj_distance < 3.0,
        "Mean reprojection distance too high: {:.2} pixels",
        mcsc_result.mean_reproj_distance
    );

    // Check that the calibration makes sense.
    check_calibration_quality_from_xml(
        &xml_out_name,
        &input,
        #[cfg(feature = "with-octave")]
        false,
    )?;

    Ok(())
}

#[cfg(feature = "with-octave")]
#[test]
fn test_braidz_octave_mcsc_no_radfiles() -> Result<()> {
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

    // Potentially do not delete temporary directory
    let save_output = match std::env::var_os(ENV_VAR_NAME) {
        Some(v) => &v != "0",
        None => false,
    };

    if save_output {
        std::mem::forget(data_root); // do not drop it, so do not delete it
    }

    let rdr = std::fs::File::open(&local_fname)?;
    let cal_data_archive = ZipArchive::new(rdr)?;

    unpack_zip_into(cal_data_archive, &data_root_dir_name)?;

    let input = data_root_dir_name.join("20250131_192425.braidz");
    std::fs::remove_dir_all(data_root_dir_name.join("camera_info"))?;

    let opt = Cli {
        input: input.clone(),
        checkerboard_cal_dir: None,
        use_nth_observation: Some(10),
        no_bundle_adjustment: true,
        force_allow_no_checkerboard_cal: true,
        do_mcsc_projective_ba: true,
        ..Default::default()
    };
    let xml_out_name = braidz_mcsc_octave(opt)?;

    // Check that the calibration makes sense
    check_calibration_quality_from_xml(
        &xml_out_name,
        &input,
        #[cfg(feature = "with-octave")]
        true,
    )?;

    Ok(())
}

#[test]
fn test_braidz_mcsc_no_radfiles() -> Result<()> {
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

    // Potentially do not delete temporary directory
    let save_output = match std::env::var_os(ENV_VAR_NAME) {
        Some(v) => &v != "0",
        None => false,
    };

    if save_output {
        std::mem::forget(data_root); // do not drop it, so do not delete it
    }

    let rdr = std::fs::File::open(&local_fname)?;
    let cal_data_archive = ZipArchive::new(rdr)?;

    unpack_zip_into(cal_data_archive, &data_root_dir_name)?;

    let input = data_root_dir_name.join("20250131_192425.braidz");
    std::fs::remove_dir_all(data_root_dir_name.join("camera_info"))?;

    let opt = Cli {
        input: input.clone(),
        checkerboard_cal_dir: None,
        use_nth_observation: Some(10),
        force_allow_no_checkerboard_cal: true,
        do_mcsc_projective_ba: true,
        ..Default::default()
    };
    let (xml_out_name, mcsc_result) = braidz_mcsc(opt)?;
    assert!(
        mcsc_result.mean_reproj_distance < 0.3,
        "Mean reprojection distance too high: {:.2} pixels",
        mcsc_result.mean_reproj_distance
    );

    // Check that the calibration makes sense
    // Native MCSC may produce slightly more skew than Octave version
    check_calibration_quality_from_xml(
        &xml_out_name,
        &input,
        #[cfg(feature = "with-octave")]
        false,
    )?;

    Ok(())
}
