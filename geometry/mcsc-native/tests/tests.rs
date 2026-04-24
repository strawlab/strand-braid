use camino::{Utf8Path, Utf8PathBuf};
use eyre::{Result, WrapErr};
use std::io::Read;
use std::io::Seek;
use zip::ZipArchive;

const URL_BASE: &str = "https://strawlab-cdn.com/assets/";

const ENV_VAR_NAME: &str = "MCSC_NATIVE_SAVE_TEST_OUTPUT";

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

#[ignore] // Ignore normally because it is slow.
#[test]
fn test_braidz_20cams() -> Result<()> {
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

    let config_path = data_root_dir_name.join("multicamselfcal.cfg");

    // Load MCSC config
    let config = mcsc_native::parse_mcsc_config(&config_path)?;

    println!(
        "Config: use_nth_frame={}, num_cams_fill_raw={:?}",
        config.use_nth_frame, config.num_cams_fill_raw
    );

    // Load MCSC data from config directory (handles Use-Nth-Frame subsampling)
    let mcsc_input = mcsc_native::load_mcsc_data(&config)?;

    // Convert to McscConfig
    let mcsc_config = mcsc_native::ini_to_mcsc_config(&config);

    // Run MCSC calibration using the native API
    let mcsc_result = mcsc_native::run_mcsc(mcsc_input, mcsc_config)
        .with_context(|| "MCSC calibration failed")?;

    println!(
        "mcsc_result.mean_reproj_distance: {:.2}/{:.2} pixels",
        mcsc_result.mean_reproj_distance, mcsc_result.std_reproj_distance
    );
    assert!(
        mcsc_result.mean_reproj_distance <= 0.8,
        "Mean reprojection distance should be less than or equal to 0.8 pixels"
    );

    // Verify results
    assert_eq!(
        mcsc_result.projection_matrices.len(),
        config.num_cameras,
        "Should have projection matrices for all cameras"
    );
    assert_eq!(
        mcsc_result.camera_centers.len(),
        config.num_cameras,
        "Should have camera centers for all cameras"
    );
    assert_eq!(
        mcsc_result.rotations.len(),
        config.num_cameras,
        "Should have rotation matrices for all cameras"
    );

    println!(
        "test_braidz_20cams: Calibration complete. {} cameras, {} inliers, {} 3D points",
        mcsc_result.projection_matrices.len(),
        mcsc_result.inlier_indices.len(),
        mcsc_result.points_3d.ncols()
    );

    Ok(())
}
