#[macro_use]
extern crate approx;

use flydra_mvg::FlydraMultiCameraSystem;

use nalgebra::geometry::{Point2, Point3};

use mvg::{DistortedPixel, PointWorldFrame};

macro_rules! check_project_3d_roundtrip {
    ($cam: expr) => {{
        let uv_raws = generate_uv_raw($cam.width(), $cam.height());
        for distorted in uv_raws.into_iter() {
            let ray = $cam.project_distorted_pixel_to_ray(&distorted);
            let result_2 = $cam.project_ray_to_distorted_pixel(&ray);
            assert_relative_eq!(distorted.coords, result_2.coords, max_relative = 1.0);
        }
    }};
}

fn generate_uv_raw(width: usize, height: usize) -> Vec<DistortedPixel<f64>> {
    let step = 5;
    let border = 65;
    let mut uv_raws = Vec::new();
    for row in num_iter::range_step(border, height - border, step) {
        for col in num_iter::range_step(border, width - border, step) {
            uv_raws.push(DistortedPixel {
                coords: Point2::new(col as f64, row as f64),
            });
        }
    }
    uv_raws
}

#[test]
fn test_distortion_and_water_flydra_xml() {
    // to fix: Camera.project_3d_to_pixel() for water must be called.

    let buf = include_str!("flydra/sample_calibration_water.xml");
    let cams =
        FlydraMultiCameraSystem::<f64>::from_flydra_xml(buf.as_bytes()).expect("from_flydra_xml");

    // check that basic camera 2d->3d->2d roundtrip works
    for cam in cams.cameras() {
        // 3d world coord must be at or above water surface if
        // distance along ray is height of camera above water surface.
        check_project_3d_roundtrip!(cam);
    }

    // The 2d pixel coords were obtained by running this through the original flydra code.
    let pt = PointWorldFrame {
        coords: Point3::new(0.01, 0.02, -0.03),
    };

    #[rustfmt::skip]
    let points_orig = {
        vec![
        ("Basler_21425994".to_string(), DistortedPixel { coords: Point2::new( 324.36365541, 230.81705908)}),
        ("Basler_21425998".to_string(), DistortedPixel { coords: Point2::new( 295.26854971, 180.42595947)}),
        ("Basler_21426001".to_string(), DistortedPixel { coords: Point2::new( 366.88089027, 241.41399229)}),
        ("Basler_21426006".to_string(), DistortedPixel { coords: Point2::new( 248.31394233, 238.94298696)}),
    ]};

    for &(ref cam_name, ref expected) in points_orig.iter() {
        let cam = cams.cam_by_name(cam_name).unwrap();
        let actual = cam.project_3d_to_distorted_pixel(&pt);
        assert_relative_eq!(actual.coords, expected.coords, max_relative = 1e-3);
    }

    let pt_actual = cams.find3d_distorted(&points_orig).unwrap().point();

    // In flydra, we get [ 0.0100904   0.02010991  0.04320179], which isn't
    // perfect, be we should at least get roughly that.
    assert_relative_eq!(pt.coords, pt_actual.coords, max_relative = 1e-5);

    // Now do it again for under water. There we do not have the true value,
    // but still the round trip should work.

    // check that basic camera 2d->3d->2d roundtrip works
    for cam in cams.cameras() {
        // 3d world coord must be at or above water surface if
        // distance along ray is height of camera above water surface.
        check_project_3d_roundtrip!(cam);
    }
}

#[test]
fn test_distortion_above_water_flydra_xml() {
    let buf = include_str!("flydra/sample_calibration_water.xml");
    let cams =
        FlydraMultiCameraSystem::<f64>::from_flydra_xml(buf.as_bytes()).expect("from_flydra_xml");

    // check that basic camera 2d->3d->2d roundtrip works
    for cam in cams.cameras() {
        // 3d world coord must be at or above water surface if
        // distance along ray is height of camera above water surface.
        check_project_3d_roundtrip!(cam);
    }

    let pt = PointWorldFrame {
        coords: Point3::new(0.01, 0.02, 0.03),
    };

    // flydra has bugs here and so we do not compare with flydra output.
    let mut found_points = Vec::new();
    for cam in cams.cameras() {
        let actual = cam.project_3d_to_distorted_pixel(&pt);
        found_points.push((cam.name().to_string(), actual));
    }

    let pt_actual = cams.find3d_distorted(&found_points).unwrap().point();

    assert_relative_eq!(pt.coords, pt_actual.coords, max_relative = 1e-5);
}

#[test]
fn test_flydra_xml_writing() {
    for input_xml in [
        include_str!("flydra/sample_calibration.xml"),
        include_str!("flydra/sample_calibration_water.xml"),
    ]
    .iter()
    {
        let flydra_xml_orig = input_xml.as_bytes();
        let cams_orig = FlydraMultiCameraSystem::<f64>::from_flydra_xml(flydra_xml_orig)
            .expect("from_flydra_xml orig");

        let mut flydra_xml_new: Vec<u8> = Vec::new();
        cams_orig
            .to_flydra_xml(&mut flydra_xml_new)
            .expect("to_flydra_xml");

        let cams_new = FlydraMultiCameraSystem::<f64>::from_flydra_xml(flydra_xml_new.as_slice())
            .expect("from_flydra_xml new");

        for cam_orig in cams_orig.cameras() {
            let cam_new = cams_new.cam_by_name(cam_orig.name()).unwrap();
            assert_eq!(cam_orig.width(), cam_new.width());
            assert_eq!(cam_orig.height(), cam_new.height());
            // assert_eq!(cam_orig.extrinsics(), cam_new.extrinsics()); // brittle

            #[rustfmt::skip]
            {
                for pt in &[
                    PointWorldFrame { coords: Point3::new(0.01, 0.02, -0.03) },
                    PointWorldFrame { coords: Point3::new(0.01, 0.02, 0.00) },
                    PointWorldFrame { coords: Point3::new(0.01, 0.02, 0.03) },
                    PointWorldFrame { coords: Point3::new(-0.01, -0.02, -0.03) },
                ] {
                    let expected = cam_orig.project_3d_to_distorted_pixel(pt);
                    let actual = cam_new.project_3d_to_distorted_pixel(pt);
                    assert_relative_eq!(actual.coords, expected.coords, max_relative = 1e-10);
                }
            };
        }

        assert_eq!(cams_orig.len(), cams_new.len());
    }
}

#[test]
fn test_simple_flydra_xml() {
    let buf = include_str!("flydra/sample_calibration.xml");
    let cams =
        FlydraMultiCameraSystem::<f64>::from_flydra_xml(buf.as_bytes()).expect("from_flydra_xml");

    for cam in cams.cameras() {
        check_project_3d_roundtrip!(cam);
    }

    // The 2d pixel coords were obtained by running this through the original flydra code.
    let pt = PointWorldFrame {
        coords: Point3::new(0.01, 0.02, 0.03),
    };

    #[rustfmt::skip]
    let points_orig = {
        vec![
        ("cam1_0".to_string(), DistortedPixel { coords: Point2::new( 504.0980046,  268.07103667)}),
        ("cam2_0".to_string(), DistortedPixel { coords: Point2::new(  27.42947924, 263.73799455)}),
        ("cam3_0".to_string(), DistortedPixel { coords: Point2::new(  91.75180821, 263.81488127)}),
        ("cam4_0".to_string(), DistortedPixel { coords: Point2::new( 398.1000139,  242.91397877)}),
        ("cam5_0".to_string(), DistortedPixel { coords: Point2::new( 412.130819,   240.21668937)}),
    ]};

    for &(ref cam_name, ref expected) in points_orig.iter() {
        let cam = cams.cam_by_name(cam_name).unwrap();
        let actual = cam.project_3d_to_distorted_pixel(&pt);
        println!("{}: actual {:?}, expected {:?}", cam_name, actual, expected);
        assert_relative_eq!(actual.coords, expected.coords, max_relative = 1e-5);
    }

    let pt_actual = cams.find3d_distorted(&points_orig).unwrap().point();
    assert_relative_eq!(pt.coords, pt_actual.coords, max_relative = 1e-5);
}

#[test]
fn test_jacobian() {
    for input_xml in [
        include_str!("flydra/sample_calibration.xml"),
        include_str!("flydra/sample_calibration_water.xml"),
    ]
    .iter()
    {
        let flydra_xml_orig = input_xml.as_bytes();
        let cams = FlydraMultiCameraSystem::<f64>::from_flydra_xml(flydra_xml_orig)
            .expect("from_flydra_xml orig");

        let center = PointWorldFrame {
            coords: Point3::new(0.01, 0.02, -0.03),
        };

        let offset: Point3<f64> = Point3::new(0.0, 0.0, 0.01);

        for cam in cams.cameras() {
            let center_projected = cam.project_3d_to_pixel(&center).coords;

            // linearize camera model
            let linearized_cam = cam.linearize_numerically_at(&center, 0.001).unwrap();

            // get the point with the (non-linear) camera model
            let nonlin = cam
                .project_3d_to_pixel(&PointWorldFrame {
                    coords: Point3 {
                        coords: center.coords.coords + offset.coords,
                    },
                })
                .coords;

            // get the point with the linearized camera model
            let lin_pred_undist = center_projected.coords + linearized_cam * offset.coords;

            for i in 0..2 {
                assert_relative_eq!(nonlin[i], lin_pred_undist[i], max_relative = 1e-2);
            }
        }
    }
}
