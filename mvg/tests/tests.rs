extern crate nalgebra as na;
extern crate num_traits;
#[macro_use]
extern crate approx;
extern crate alga;
extern crate num_iter;

extern crate mvg;

use opencv_ros_camera::{from_ros_yaml, NamedIntrinsicParameters};

use nalgebra::core::Unit;
use nalgebra::geometry::{Point2, Point3, Quaternion};

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

#[test]
fn test_load_pymvg() -> anyhow::Result<()> {
    let buf = include_str!("pymvg-example.json");
    let _system = mvg::MultiCameraSystem::<f64>::from_pymvg_file_json(buf.as_bytes())?;
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
    use nalgebra::{Dynamic, OMatrix, U2, U3};

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

    let x3d = OMatrix::<_, Dynamic, U3>::from_row_slice(&x3d_data);
    let x2d = OMatrix::<_, Dynamic, U2>::from_row_slice(&x2d_data);

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
