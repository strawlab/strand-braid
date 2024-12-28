#[macro_use]
extern crate approx;

use eyre as anyhow;
use opencv_ros_camera::{from_ros_yaml, Distortion, NamedIntrinsicParameters, RosOpenCvIntrinsics};

use nalgebra::{
    self as na,
    core::Unit,
    geometry::{Point2, Point3, Quaternion},
};

use mvg::{Camera, DistortedPixel, PointWorldFrame};

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
fn test_distortion_roundtrip() {
    let buf = include_str!("ros/camera.yaml");
    let named_intrinsics: NamedIntrinsicParameters<f64> = from_ros_yaml(buf.as_bytes()).unwrap();
    let intrinsics = named_intrinsics.intrinsics;
    let uv_raws = generate_uv_raw(named_intrinsics.width, named_intrinsics.height);
    for distorted in uv_raws.into_iter() {
        let d2 = (&distorted).into();
        let undistorted = intrinsics.undistort(&d2);
        let distorted2 = intrinsics.distort(&undistorted);
        assert_relative_eq!(d2.data, distorted2.data, max_relative = 1.0);
    }
}

// #[test]
// fn test_linearized_camera() -> anyhow::Result<()> {
//     let (width, height) = (640, 480);
//     let distortion = Distortion::from_opencv_vec(na::Vector5::new(-0.1, 0.05, 0.1, -0.05, 0.01));
//     let intrinsics = RosOpenCvIntrinsics::from_params_with_distortion(
//         100.0, 1.0, 101.0, 320.0, 240.0, distortion,
//     );
//     let extrinsics = mvg::extrinsics::make_default_extrinsics();
//     let cam = mvg::Camera::new(width, height, extrinsics, intrinsics.clone())?;
//     let linearized = cam.linearize()?;
//     assert!(linearized.intrinsics().distortion.is_linear());
//     let uv_raws = generate_uv_raw(width, height);
//     for distorted_orig in uv_raws.iter() {
//         // Project the distorted pixel coordinate into 3D world space coordinate.
//         let world_coord = cam.project_distorted_pixel_to_3d_with_dist(distorted_orig, 1.0);
//         // Image the 3D world space coordinate with the linearized camera.
//         let distorted_linear = linearized.project_3d_to_distorted_pixel(&world_coord);
//         // Although the points are "distorted", they will have identical
//         // coordinates to their undistorted version because there is no
//         // distortion.
//         let distorted_linear2 = (&distorted_linear).into();
//         let undistorted_linear = linearized.intrinsics().undistort(&distorted_linear2);
//         // These should be exactly equal without any floating point errors.
//         assert_eq!(
//             undistorted_linear.data.transpose(),
//             distorted_linear.coords.coords
//         );

//         // And, perhaps most importantly, these coordinates from the linear
//         // camera should be the same (withing floating point numerical error) as
//         // the undistorted variant from the original camera.
//         let distorted_orig2 = (&*distorted_orig).into();
//         let undistorted_orig = intrinsics.undistort(&distorted_orig2);
//         approx::assert_relative_eq!(
//             undistorted_orig.data,
//             undistorted_linear.data,
//             epsilon = 1e-6
//         );
//     }
//     Ok(())
// }

#[test]
fn test_linearized_cam_geom_camera() -> anyhow::Result<()> {
    let (width, height) = (640, 480);
    let distortion = Distortion::from_opencv_vec(na::Vector5::new(-0.1, 0.05, 0.1, -0.05, 0.01));
    let intrinsics = RosOpenCvIntrinsics::from_params_with_distortion(
        100.0, 1.0, 101.0, 320.0, 240.0, distortion,
    );
    let extrinsics = mvg::extrinsics::make_default_extrinsics();
    let cam = mvg::Camera::new(width, height, extrinsics, intrinsics.clone())?;
    let linearized = cam.linearize_to_cam_geom();
    let uv_raws = generate_uv_raw(width, height);
    for distorted_orig in uv_raws.iter() {
        // Project the distorted pixel coordinate into 3D world space coordinate.
        let world_coord = cam.project_distorted_pixel_to_3d_with_dist(distorted_orig, 1.0);
        // Image the 3D world space coordinate with the linearized camera.
        let world_coords = cam_geom::Points::new(world_coord.coords.coords.transpose());
        let linear_2d = linearized.world_to_pixel(&world_coords);

        // The coordinates from the linear camera should be the same (within
        // floating point numerical error) as the undistorted variant from the
        // original camera.
        let distorted_orig2 = (&*distorted_orig).into();
        let undistorted_orig = intrinsics.undistort(&distorted_orig2);
        approx::assert_relative_eq!(undistorted_orig.data, linear_2d.data, epsilon = 1e-6);
    }
    Ok(())
}

#[test]
fn test_cam_system_pymvg_roundtrip() -> anyhow::Result<()> {
    let buf = include_str!("pymvg-example.json");
    let system1 = mvg::MultiCameraSystem::<f64>::from_pymvg_json(buf.as_bytes())?;
    let mut buf2 = Vec::new();
    system1.to_pymvg_writer(&mut buf2)?;
    let system2 = mvg::MultiCameraSystem::<f64>::from_pymvg_json(buf.as_bytes())?;
    assert_eq!(system1, system2);

    // Now check again by passing points. Note that if this fails while the
    // above passes, this would indicate a bug in the PartialEq implementation
    // of MultiCameraSystem or its fields. Therefore, we should never fail below
    // here.
    let orig = system1.cam_by_name("cam1").unwrap();
    let reloaded = system2.cam_by_name("cam1").unwrap();
    let p1 = PointWorldFrame {
        coords: Point3::new(1.0, 2.0, 3.0),
    };
    let p2 = PointWorldFrame {
        coords: Point3::new(0.0, 0.0, 0.0),
    };

    let o1 = orig.project_3d_to_distorted_pixel(&p1);
    let o2 = orig.project_3d_to_distorted_pixel(&p2);

    let r1 = reloaded.project_3d_to_distorted_pixel(&p1);
    let r2 = reloaded.project_3d_to_distorted_pixel(&p2);

    approx::assert_relative_eq!(o1.coords, r1.coords, epsilon = 1e-6);
    approx::assert_relative_eq!(o2.coords, r2.coords, epsilon = 1e-6);

    Ok(())
}

#[test]
fn test_load_pymvg() -> anyhow::Result<()> {
    let buf = include_str!("pymvg-example.json");
    let system = mvg::MultiCameraSystem::<f64>::from_pymvg_json(buf.as_bytes())?;
    assert_eq!(system.cams().len(), 1);
    let cam = system.cam_by_name("cam1").unwrap();

    // First test some key points with data from Python ----------------

    /* This script was used
    from pymvg.multi_camera_system import MultiCameraSystem
    import numpy as np

    system = MultiCameraSystem.from_pymvg_file("pymvg-example.json")

    point_3d = np.array([1.0, 2.0, 3.0])

    for camera_name in system.get_names():
        for point_3d in [np.array([1.0, 2.0, 3.0]),
                        np.array([0.0, 0.0, 0.0])]:
            print("point_3d: %r" % (point_3d,))
            this_pt2d = system.find2d(camera_name, point_3d)
            # data.append((camera_name, this_pt2d))
            print("%r: %r" % (camera_name, this_pt2d))
        */

    let p1 = cam.project_3d_to_distorted_pixel(&PointWorldFrame {
        coords: Point3::new(1.0, 2.0, 3.0),
    });
    let p2 = cam.project_3d_to_distorted_pixel(&PointWorldFrame {
        coords: Point3::new(0.0, 0.0, 0.0),
    });

    approx::assert_relative_eq!(
        p1.coords,
        na::Point2::new(1833.09435806, 2934.94805999),
        epsilon = 1e-6
    );
    approx::assert_relative_eq!(
        p2.coords,
        na::Point2::new(616.11767192, 269.61411571),
        epsilon = 1e-6
    );

    // Now test some values directly read from the pymvg json file -----------------
    assert_eq!(cam.width(), 1080);
    assert_eq!(cam.height(), 720);
    assert_eq!(cam.intrinsics().p[(0, 0)], 1236.529440113545);
    assert_eq!(cam.intrinsics().p[(1, 0)], 0.0);
    assert_eq!(cam.intrinsics().p[(0, 1)], 33.472107763674444);
    assert_eq!(cam.intrinsics().p[(2, 2)], 1.0);
    assert_eq!(cam.intrinsics().p[(2, 3)], 0.0);
    approx::assert_relative_eq!(
        cam.intrinsics().distortion.opencv_vec(),
        &na::Vector5::new(
            -0.15287156437154006,
            0.8689691428266413,
            -0.01219554893369256,
            0.0014329742677790488,
            0.0
        )
    );
    Ok(())
}

#[test]
fn test_whole_vs_parts() {
    let cam = get_cam();
    let extrinsics = cam.extrinsics();
    let intrinsics = cam.intrinsics();

    let pt = PointWorldFrame {
        coords: Point3::new(0.01, 0.02, -0.03),
    };

    let whole = cam.project_3d_to_distorted_pixel(&pt);
    let camcoords3d = extrinsics.world_to_camera(&((&pt).into()));
    use cam_geom::IntrinsicParameters;
    let parts: DistortedPixel<_> = intrinsics.camera_to_pixel(&camcoords3d).into();

    assert_relative_eq!(whole.coords, parts.coords, max_relative = 1e-5);
}

fn get_cam() -> Camera<f64> {
    let buf = include_str!("ros/camera.yaml");
    let named_intrinsics: NamedIntrinsicParameters<f64> = from_ros_yaml(buf.as_bytes()).unwrap();

    let rquat = Unit::new_normalize(Quaternion::new(
        0.357288321925,
        0.309377331102,
        0.600893485738,
        0.644637681813,
    ));
    let translation = Point3::new(0.273485679077, 0.0707310128808, 0.0877802104531);
    let extrinsics = mvg::extrinsics::from_rquat_translation(rquat, translation);

    Camera::new(
        named_intrinsics.width,
        named_intrinsics.height,
        extrinsics,
        named_intrinsics.intrinsics,
    )
    .expect("Camera::new")
}

macro_rules! check_project_3d_roundtrip {
    ($cam: expr, $dist: expr) => {{
        let uv_raws = generate_uv_raw($cam.width(), $cam.height());
        for distorted in uv_raws.into_iter() {
            let pt3d_1 = $cam.project_distorted_pixel_to_3d_with_dist(&distorted, $dist);
            let result_1 = $cam.project_3d_to_distorted_pixel(&pt3d_1);
            assert_relative_eq!(distorted.coords, result_1.coords, max_relative = 1.0);
        }
    }};
}

#[test]
fn test_project_3d_roundtrip() {
    let cam = get_cam();
    check_project_3d_roundtrip!(cam, na::convert(1.0));
}

#[test]
fn test_dlt_mvg() {
    use nalgebra::{Dyn, OMatrix, U2, U3};

    let cam = mvg::Camera::<f64>::default();

    let mut x3d_data: Vec<f64> = Vec::new();
    let mut x2d_data: Vec<f64> = Vec::new();
    let mut orig_points = Vec::new();
    let mut orig_uv = Vec::new();

    // project several 3D points to 2D
    for x in [-1.0f64, 0.0, 1.0].iter() {
        for y in [-1.0f64, 0.0, 1.0].iter() {
            for z in [-1.0f64, 0.0, 1.0].iter() {
                let pt = mvg::PointWorldFrame {
                    coords: Point3::new(*x, *y, *z),
                };
                let uv = cam.project_3d_to_distorted_pixel(&pt);
                x3d_data.push(*x);
                x3d_data.push(*y);
                x3d_data.push(*z);
                x2d_data.push(uv.coords[0]);
                x2d_data.push(uv.coords[1]);
                orig_points.push(pt);
                orig_uv.push(uv);
            }
        }
    }

    let x3d = OMatrix::<_, Dyn, U3>::from_row_slice(&x3d_data);
    let x2d = OMatrix::<_, Dyn, U2>::from_row_slice(&x2d_data);

    // perform DLT
    let dlt_results = dlt::dlt(&x3d, &x2d, 1e-10).unwrap();

    // create new camera from DLT results
    let cam2 = mvg::Camera::from_pmat(cam.width(), cam.height(), &dlt_results).unwrap();

    // project original points again to 2D with the new camera
    let epsilon = 1e-7;
    for (orig_pt, orig_uv) in orig_points.into_iter().zip(orig_uv.into_iter()) {
        let new_uv = cam2.project_3d_to_distorted_pixel(&orig_pt);
        approx::assert_relative_eq!(orig_uv.coords[0], new_uv.coords[0], epsilon = epsilon);
        approx::assert_relative_eq!(orig_uv.coords[1], new_uv.coords[1], epsilon = epsilon);
    }
}
