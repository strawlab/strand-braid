use strand_cam_offline_kalmanize::parse_configs_and_run;

const INPUT_CSV: &'static str = include_str!("data/flytrax20191122_103500.csv");
const CALIBRATION_PARAMS_TOML: &'static str = include_str!("data/cal1.toml");

#[test]
fn test_run_end_to_end() {
    let point_detection_csv_reader = INPUT_CSV.as_bytes();

    let data_dir = tempdir::TempDir::new("strand-convert").unwrap();

    // Create unique dir for this test so we do not conflict with other
    // concurrent tests.
    let output_dir = tempdir::TempDir::new("strand-convert-output").unwrap();
    // The output .braid dir and ultimately .braidz filename:
    let output_dirname = output_dir.as_ref().join("out.braid");

    let tracking_params_buf = Some(include_str!("data/tracking.toml"));

    parse_configs_and_run(
        point_detection_csv_reader,
        data_dir,
        &output_dirname,
        &CALIBRATION_PARAMS_TOML,
        tracking_params_buf,
    )
    .unwrap();

    let braidz_fname = output_dirname.with_extension("braidz");

    let reader = zip_or_dir::ZipDirArchive::auto_from_path(braidz_fname).unwrap();
    let parsed = braidz_parser::braidz_parse(reader).unwrap();

    let kalman_estimates_info = parsed.kalman_estimates_info.as_ref().unwrap();

    assert!(kalman_estimates_info.trajectories.len() >= 7);
    assert!(kalman_estimates_info.trajectories.len() < 1000);
}

#[test]
fn test_z_values_zero() {
    let point_detection_csv_reader = INPUT_CSV.as_bytes();

    let data_dir = tempdir::TempDir::new("strand-convert").unwrap();

    // Create unique dir for this test so we do not conflict with other
    // concurrent tests.
    let output_dir = tempdir::TempDir::new("strand-convert-output").unwrap();
    // The output .braid dir and ultimately .braidz filename:
    let output_dirname = output_dir.as_ref().join("out.braid");

    parse_configs_and_run(
        point_detection_csv_reader,
        data_dir,
        &output_dirname,
        &CALIBRATION_PARAMS_TOML,
        None,
    )
    .unwrap();

    let braidz_fname = output_dirname.with_extension("braidz");

    let reader = zip_or_dir::ZipDirArchive::auto_from_path(braidz_fname).unwrap();
    let parsed = braidz_parser::braidz_parse(reader).unwrap();

    let kalman_estimates_info = parsed.kalman_estimates_info.as_ref().unwrap();
    let trajs = &kalman_estimates_info.trajectories;

    for (_obj_id, traj_data) in trajs {
        for row in traj_data.iter() {
            assert!(row.2.abs() < 1e-6);
        }
    }
}
