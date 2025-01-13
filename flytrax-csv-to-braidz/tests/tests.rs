use test_log::test;

use flytrax_csv_to_braidz::{parse_configs_and_run, RowFilter};

#[test(tokio::test)]
async fn test_run_end_to_end() {
    const INPUT_CSV: &str = include_str!("data/flytrax20191122_103500.csv");
    const CALIBRATION_PARAMS_FILENAME: &str = "tests/data/cal1.toml";
    let point_detection_csv_reader = INPUT_CSV.as_bytes();

    let flydra_csv_temp_dir = Some(tempfile::Builder::new().tempdir().unwrap());

    // Create unique dir for this test so we do not conflict with other
    // concurrent tests.
    let output_dir = tempfile::Builder::new().tempdir().unwrap();
    // The output .braidz filename:
    let output_braidz = output_dir.as_ref().join("out.braidz");

    let tracking_params_buf = Some(include_str!("data/tracking.toml"));

    let row_filters = vec![];
    parse_configs_and_run(
        point_detection_csv_reader,
        flydra_csv_temp_dir.as_ref(),
        None,
        &output_braidz,
        CALIBRATION_PARAMS_FILENAME,
        tracking_params_buf,
        &row_filters,
        true,
        None,
        braid_offline::KalmanizeOptions::default(),
    )
    .await
    .unwrap();

    let reader = zip_or_dir::ZipDirArchive::auto_from_path(output_braidz).unwrap();
    let parsed = braidz_parser::braidz_parse(reader).unwrap();

    let kalman_estimates_info = parsed.kalman_estimates_info.as_ref().unwrap();

    assert!(kalman_estimates_info.trajectories.len() >= 7);
    assert!(kalman_estimates_info.trajectories.len() < 1000);

    flydra_csv_temp_dir.unwrap().close().unwrap();
    output_dir.close().unwrap();
}

#[test(tokio::test)]
async fn test_z_values_zero() {
    const INPUT_CSV: &str = include_str!("data/flytrax20191122_103500.csv");
    const CALIBRATION_PARAMS_FILENAME: &str = "tests/data/cal1.toml";

    let point_detection_csv_reader = INPUT_CSV.as_bytes();

    let flydra_csv_temp_dir = Some(tempfile::Builder::new().tempdir().unwrap());

    // Create unique dir for this test so we do not conflict with other
    // concurrent tests.
    let output_dir = tempfile::Builder::new().tempdir().unwrap();
    // The output .braidz filename:
    let output_braidz = output_dir.as_ref().join("out.braidz");

    let row_filters = vec![RowFilter::InPseudoCalRegion];
    parse_configs_and_run(
        point_detection_csv_reader,
        flydra_csv_temp_dir.as_ref(),
        None,
        &output_braidz,
        CALIBRATION_PARAMS_FILENAME,
        None,
        &row_filters,
        true,
        None,
        braid_offline::KalmanizeOptions::default(),
    )
    .await
    .unwrap();

    let reader = zip_or_dir::ZipDirArchive::auto_from_path(output_braidz).unwrap();
    let parsed = braidz_parser::braidz_parse(reader).unwrap();

    let kalman_estimates_info = parsed.kalman_estimates_info.as_ref().unwrap();
    let trajs = &kalman_estimates_info.trajectories;

    let mut count = 0;
    for traj_data in trajs.values() {
        for row in traj_data.position.iter() {
            count += 1;
            assert!(row[2].abs() < 1e-6);
        }
    }

    assert!(count >= 1);

    flydra_csv_temp_dir.unwrap().close().unwrap();

    // Delete the temporary directory.
    output_dir.close().unwrap();
}

#[test(tokio::test)]
#[cfg(feature = "with_apriltags")]
async fn mini_arenas_with_apriltags() -> eyre::Result<()> {
    const URL_BASE: &str = "https://strawlab-cdn.com/assets";

    const CHECKERBOARD_CAL_FNAME: &str = "20230629-ob9-data/20230629_optobehav9_calibration.yaml";
    const CHECKERBOARD_CAL_SUM: &str =
        "1287b90bdbd0631b6dbd3b9d7c32c7aca6db687556b6ad9cd14d04a2df0c9b72";
    download_verify::download_verify(
        format!("{URL_BASE}/{CHECKERBOARD_CAL_FNAME}").as_str(),
        CHECKERBOARD_CAL_FNAME,
        &download_verify::Hash::Sha256(CHECKERBOARD_CAL_SUM.into()),
    )?;

    const APRILTAGS_COORDS_FNAME: &str = "20230629-ob9-data/apriltags_coordinates_arena9.csv";
    const APRILTAGS_COORDS_SUM: &str =
        "e465427988a2f98ba01af2193f059ca7f01618a4646ad98c4404cd469728179f";
    download_verify::download_verify(
        format!("{URL_BASE}/{APRILTAGS_COORDS_FNAME}").as_str(),
        APRILTAGS_COORDS_FNAME,
        &download_verify::Hash::Sha256(APRILTAGS_COORDS_SUM.into()),
    )?;

    const FLYTRAX_DATA_FNAME: &str = "20230629-ob9-data/flytrax20230629_092903_optobehav9.csv";
    const FLYTRAX_DATA_SUM: &str =
        "1867ec0cffa4ea2db4f18100e6d6845e985fdbd4b0ee22319a5bd4e6287159d0";
    download_verify::download_verify(
        format!("{URL_BASE}/{FLYTRAX_DATA_FNAME}").as_str(),
        FLYTRAX_DATA_FNAME,
        &download_verify::Hash::Sha256(FLYTRAX_DATA_SUM.into()),
    )?;

    const FLYTRAX_IMAGE_FNAME: &str = "20230629-ob9-data/flytrax20230629_092903_optobehav9.jpg";
    const FLYTRAX_IMAGE_SUM: &str =
        "ebcd891f2ba349f7756a1b4c85873a70b5c35e1359c72772d7c847205c635ed2";
    download_verify::download_verify(
        format!("{URL_BASE}/{FLYTRAX_IMAGE_FNAME}").as_str(),
        FLYTRAX_IMAGE_FNAME,
        &download_verify::Hash::Sha256(FLYTRAX_IMAGE_SUM.into()),
    )?;

    const TRACKING_PARAMS_FNAME: &str = "20230629-ob9-data/tracking_params_grid.toml";
    const TRACKING_PARAMS_SUM: &str =
        "9f5b200361a6e2f66fe5aa76a98cae6a12a7f6c21559b882e21037a3f309824b";
    download_verify::download_verify(
        format!("{URL_BASE}/{TRACKING_PARAMS_FNAME}").as_str(),
        TRACKING_PARAMS_FNAME,
        &download_verify::Hash::Sha256(TRACKING_PARAMS_SUM.into()),
    )?;

    // ----

    let point_detection_csv_reader =
        std::io::BufReader::new(std::fs::File::open(FLYTRAX_DATA_FNAME)?);

    // Create unique dir for this test so we do not conflict with other
    // concurrent tests.
    let output_dir = tempfile::Builder::new().tempdir().unwrap();
    // The output .braidz filename:
    let output_braidz = output_dir
        .as_ref()
        .join("mini_arenas_with_apriltags.braidz");

    let row_filters = vec![];

    let jpeg_buf = std::fs::read(&FLYTRAX_IMAGE_FNAME)?;
    let flytrax_image = Some(image::load_from_memory_with_format(
        &jpeg_buf,
        image::ImageFormat::Jpeg,
    )?);

    let eargs = Some(flytrax_csv_to_braidz::ExtrinsicsArgs {
        apriltags_3d_fiducial_coords: APRILTAGS_COORDS_FNAME.into(),
        flytrax_csv: std::path::PathBuf::from(FLYTRAX_DATA_FNAME),
        image_filename: std::path::PathBuf::from(FLYTRAX_IMAGE_FNAME),
    });

    let opt2 = braid_offline::KalmanizeOptions {
        stop_frame: Some(2100),
        ..Default::default()
    };

    let tracking_params_buf = Some(std::fs::read_to_string(TRACKING_PARAMS_FNAME)?);

    parse_configs_and_run(
        point_detection_csv_reader,
        None,
        flytrax_image,
        &output_braidz,
        CHECKERBOARD_CAL_FNAME,
        tracking_params_buf.as_deref(),
        &row_filters,
        true,
        eargs,
        opt2,
    )
    .await?;

    let reader = zip_or_dir::ZipDirArchive::auto_from_path(output_braidz)?;
    let parsed = braidz_parser::braidz_parse(reader)?;

    let kalman_estimates_info = parsed.kalman_estimates_info.as_ref().unwrap();
    let trajs = &kalman_estimates_info.trajectories;

    assert!(50 <= trajs.len());
    assert!(trajs.len() <= 100);

    // TODO: Check output more extensively than this.

    Ok(())
}
