use ads_webasm::components::{parse_csv, MaybeCsvData};
use braid_april_cal::*;

#[cfg(feature = "solve-pnp")]
use opencv_ros_camera::{NamedIntrinsicParameters, RosCameraInfo};

fn gen_cal() -> CalibrationResult {
    let fiducial_3d_coords_buf = include_bytes!("apriltags_coordinates.csv");
    let fiducial_3d_coords =
        parse_csv::<Fiducial3DCoords>("apriltags_coordinates.csv".into(), fiducial_3d_coords_buf);
    let fiducial_3d_coords = match fiducial_3d_coords {
        MaybeCsvData::Valid(data) => data.rows().to_vec(),
        _ => panic!("failed parsing"),
    };

    let cams_bufs = vec![
        include_bytes!("22055_apriltags20210824_164901.csv").to_vec(),
        include_bytes!("25042_apriltags20210824_164949.csv").to_vec(),
        include_bytes!("25383_apriltags20210824_165042.csv").to_vec(),
    ];

    let per_camera_2d = cams_bufs
        .into_iter()
        .map(|buf| {
            let detections = parse_csv::<AprilDetection>("camera-detections.csv".into(), &buf);
            match detections {
                MaybeCsvData::Valid(csv_data) => {
                    let datavec = csv_data.rows().to_vec();
                    let raw_buf: &[u8] = csv_data.raw_buf();
                    let cfg = get_apriltag_cfg(raw_buf).unwrap();
                    (cfg.camera_name.clone(), (cfg, datavec))
                }
                _ => panic!("failed parsing"),
            }
        })
        .collect();

    let src_data = CalData {
        fiducial_3d_coords,
        per_camera_2d,
        known_good_intrinsics: None,
    };

    let cal_result = do_calibrate_system(&src_data).unwrap();
    assert_eq!(
        cal_result.cam_system.cams_by_name().len(),
        src_data.per_camera_2d.len()
    );

    assert_eq!(
        cal_result.cam_system.cams_by_name().len(),
        cal_result.mean_reproj_dist.len(),
    );

    for (cam_name, reproj_dist) in cal_result.mean_reproj_dist.iter() {
        println!("Camera {}: mean reproj dist: {}", cam_name, reproj_dist);
        assert!(*reproj_dist < 5.0);
    }
    cal_result
}

#[test]
fn test_calibration_xml() {
    let cal_result = gen_cal();

    let xml_buf = cal_result.to_flydra_xml().unwrap();

    use flydra_mvg::FlydraMultiCameraSystem;
    let loaded: FlydraMultiCameraSystem<f64> =
        FlydraMultiCameraSystem::from_flydra_xml(xml_buf.as_slice()).unwrap();
    if loaded.has_refractive_boundary() {
        todo!("test XML calibration with water.");
    }

    for (cam_name, points) in cal_result.points.iter() {
        let cam = loaded.system().cam_by_name(cam_name).unwrap();
        let actual = compute_mean_reproj_dist(cam, points);
        let expected = cal_result.mean_reproj_dist.get(cam_name).unwrap();
        assert!(
            (actual - expected).abs() < 1e-10,
            "Reprojection error different after saving and loading calibration."
        );
    }
}

#[test]
fn test_calibration_pymvg() {
    let cal_result = gen_cal();

    let mut pymvg_json_buf = Vec::new();
    cal_result
        .cam_system
        .to_pymvg_writer(&mut pymvg_json_buf)
        .unwrap();

    use mvg::MultiCameraSystem;
    let loaded: MultiCameraSystem<f64> =
        MultiCameraSystem::from_pymvg_json(pymvg_json_buf.as_slice()).unwrap();

    for (cam_name, points) in cal_result.points.iter() {
        let cam = loaded.cam_by_name(cam_name).unwrap();
        let actual = compute_mean_reproj_dist(cam, points);
        let expected = cal_result.mean_reproj_dist.get(cam_name).unwrap();
        assert!(
            (actual - expected).abs() < 1e-10,
            "Reprojection error different after saving and loading calibration."
        );
    }
}

#[test]
#[cfg(feature = "solve-pnp")]
fn solve_pnp_with_prior_intrinsics() -> anyhow::Result<()> {
    let fiducial_3d_coords_buf = include_bytes!("data-single-cam/apriltags_coordinates.csv");
    let fiducial_3d_coords = parse_csv::<Fiducial3DCoords>(
        "data-single-cam/apriltags_coordinates.csv".into(),
        fiducial_3d_coords_buf,
    );
    let fiducial_3d_coords = match fiducial_3d_coords {
        MaybeCsvData::Valid(data) => data.rows().to_vec(),
        _ => panic!("failed parsing"),
    };

    let cams_bufs = vec![include_bytes!("data-single-cam/all.csv").to_vec()];

    let mut cam_name = None;
    let per_camera_2d = cams_bufs
        .into_iter()
        .map(|buf| {
            let detections = parse_csv::<AprilDetection>("camera-detections.csv".into(), &buf);
            match detections {
                MaybeCsvData::Valid(csv_data) => {
                    let datavec = csv_data.rows().to_vec();
                    let raw_buf: &[u8] = csv_data.raw_buf();
                    let cfg = get_apriltag_cfg(raw_buf).unwrap();
                    assert!(cam_name.is_none()); // this is a single camera test
                    cam_name = Some(cfg.camera_name.clone());
                    (cfg.camera_name.clone(), (cfg, datavec))
                }
                _ => panic!("failed parsing"),
            }
        })
        .collect();

    let cam_name = cam_name.unwrap();

    let mut all_intrinsics = std::collections::BTreeMap::new();
    let intrinsics_yaml =
        include_str!("data-single-cam/Basler_22149788.20230613_155639.yaml").as_bytes();
    let intrinsics: RosCameraInfo<f64> = serde_yaml::from_reader(intrinsics_yaml)?;

    let mut named_intrinsics: NamedIntrinsicParameters<f64> = intrinsics.try_into().unwrap();
    // Would like to use name in .yaml file, but this has been converted to "ROS form".
    named_intrinsics.name = cam_name.clone();

    all_intrinsics.insert(cam_name.clone(), named_intrinsics);

    let src_data = CalData {
        fiducial_3d_coords,
        per_camera_2d,
        known_good_intrinsics: Some(all_intrinsics),
    };

    let cal_result = do_calibrate_system(&src_data)?;

    dbg!(&cal_result.mean_reproj_dist);
    assert_eq!(cal_result.mean_reproj_dist.keys().len(), 1);

    let mean_reproj_dist = cal_result.mean_reproj_dist[&cam_name];
    assert!(mean_reproj_dist < 10.0);

    Ok(())
}
