extern crate approx;
extern crate nalgebra as na;
extern crate num_iter;
extern crate num_traits;

extern crate mvg;

use nalgebra::geometry::Point3;

use opencv_ros_camera::{Distortion, RosOpenCvIntrinsics};

use mvg::{Camera, DistortedPixel, PointWorldFrame};

fn is_similar(cam1: &crate::Camera<f64>, cam2: &crate::Camera<f64>) -> bool {
    let world_pts = [
        PointWorldFrame {
            coords: Point3::new(1.23, 4.56, 7.89),
        },
        PointWorldFrame {
            coords: Point3::new(1.0, 2.0, 3.0),
        },
    ];

    let pts1: Vec<DistortedPixel<_>> = world_pts
        .iter()
        .map(|world| cam1.project_3d_to_distorted_pixel(world))
        .collect();

    let pts2: Vec<DistortedPixel<_>> = world_pts
        .iter()
        .map(|world| cam2.project_3d_to_distorted_pixel(world))
        .collect();

    let epsilon = 1e-10;

    for (im1, im2) in pts1.iter().zip(pts2) {
        let diff = im1.coords - im2.coords;
        let dist_squared = diff.dot(&diff);
        if dist_squared > epsilon {
            return false;
        }
    }
    true
}

fn get_test_cameras() -> Vec<(String, Camera<f64>)> {
    let mut result = Vec::new();
    let extrinsics = mvg::extrinsics::make_default_extrinsics();
    for (int_name, intrinsics) in get_test_intrinsics().into_iter() {
        let name = format!("cam-{}", int_name);
        let cam = Camera::new(640, 480, extrinsics.clone(), intrinsics).unwrap();
        result.push((name, cam));
    }
    result.push(("default-cam".to_string(), Camera::default()));
    result
}

fn get_test_intrinsics() -> Vec<(String, RosOpenCvIntrinsics<f64>)> {
    use na::Vector5;
    let mut result = Vec::new();

    for (name, dist) in &[
        (
            "linear",
            Distortion::from_opencv_vec(Vector5::new(0.0, 0.0, 0.0, 0.0, 0.0)),
        ),
        (
            "d1",
            Distortion::from_opencv_vec(Vector5::new(0.1001, 0.2002, 0.3003, 0.4004, 0.5005)),
        ),
    ] {
        for skew in &[0, 10] {
            let fx = 100.0;
            let fy = 100.0;
            let cx = 320.0;
            let cy = 240.0;

            let cam = RosOpenCvIntrinsics::from_params_with_distortion(
                fx,
                *skew as f64,
                fy,
                cx,
                cy,
                dist.clone(),
            );
            result.push((format!("dist-{}_skew{}", name, skew), cam));
        }
    }

    result.push(("default".to_string(), mvg::make_default_intrinsics()));

    result
}

#[test]
fn test_from_pmat2() {
    for (name, cam1) in get_test_cameras().iter() {
        println!("testing camera {}", name);
        if name != "cam-dist-linear_skew0" {
            continue;
        }
        let pmat = match cam1.as_pmat() {
            Some(pmat) => pmat,
            None => {
                println!("skipping camera {}: no pmat", name);
                continue;
            }
        };
        let cam2 = crate::Camera::from_pmat(cam1.width(), cam1.height(), pmat).unwrap();
        println!("cam1 {:?}", cam1);
        println!("cam2 {:?}", cam2);
        assert!(is_similar(cam1, &cam2));
    }
}
