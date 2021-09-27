use ads_webasm::components::{parse_csv, MaybeCsvData};
use braid_april_cal_webapp::{
    do_calibrate_system, get_cfg, CalData, DetectionSerializer, Fiducial3DCoords,
};

#[test]
fn test_calibration() {
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
            let detections = parse_csv::<DetectionSerializer>("camera-detections.csv".into(), &buf);
            match detections {
                MaybeCsvData::Valid(csv_data) => {
                    let datavec = csv_data.rows().to_vec();
                    let raw_buf: &[u8] = csv_data.raw_buf();
                    let cfg = get_cfg(raw_buf).unwrap();
                    (cfg.camera_name.clone(), (cfg, datavec))
                }
                _ => panic!("failed parsing"),
            }
        })
        .collect();

    let src_data = CalData {
        fiducial_3d_coords,
        per_camera_2d,
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

    // Now roundtrip through XML (TODO: PyMVG)

    let xml_buf = cal_result.to_flydra_xml().unwrap();

    use flydra_mvg::FlydraMultiCameraSystem;
    let loaded: FlydraMultiCameraSystem<f64> =
        FlydraMultiCameraSystem::from_flydra_xml(xml_buf.as_slice()).unwrap();

    for (cam_name, points) in cal_result.points.iter() {
        let cam = cal_result.cam_system.cam_by_name(cam_name).unwrap();
        let actual = braid_april_cal_webapp::compute_mean_reproj_dist(&cam, &points);
        let expected = cal_result.mean_reproj_dist.get(cam_name).unwrap();
        assert!(
            (actual - expected).abs() < 1e-10,
            "Reprojection error different after saving and loading calibration."
        );
    }
}
