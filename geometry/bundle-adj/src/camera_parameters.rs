use nalgebra as na;
use num_traits::Float;
use opencv_ros_camera::RosOpenCvIntrinsics;

use super::*;

/// Create a (partial) vector of fixed parameters from a camera.
pub(crate) fn to_fixed_params<F: na::RealField + Float>(
    cam: &cam_geom::Camera<F, RosOpenCvIntrinsics<F>>,
    model_type: CameraModelType,
) -> Vec<F> {
    let i = cam.intrinsics();
    match model_type {
        CameraModelType::OpenCV5 | CameraModelType::OpenCV4 => Vec::with_capacity(0),
        CameraModelType::Linear => Vec::with_capacity(0),
        CameraModelType::ExtrinsicsOnly => {
            let mut p = vec![i.fx(), i.fy(), i.cx(), i.cy()];
            p.extend(i.distortion.opencv_vec().as_slice());
            p
        }
    }
}

/// Create a (partial) parameter vector from a camera.
pub(crate) fn to_params<F: na::RealField + Float>(
    cam: &cam_geom::Camera<F, RosOpenCvIntrinsics<F>>,
    model_type: CameraModelType,
) -> Vec<F> {
    let i = cam.intrinsics();
    debug_assert!(i.is_opencv_compatible);
    let e = cam.extrinsics();
    let rquat = e.pose().rotation;
    let abc = rquat.scaled_axis();
    let cc = e.camcenter();

    match model_type {
        CameraModelType::OpenCV5 => {
            let mut p = vec![i.fx(), i.cx(), i.cy()];
            p.extend(i.distortion.opencv_vec().as_slice());
            p.extend(&[abc.x, abc.y, abc.z]);
            p.extend(&[cc.x, cc.y, cc.z]);
            p
        }
        CameraModelType::OpenCV4 => {
            let mut p = vec![i.fx(), i.cx(), i.cy()];
            p.extend(&i.distortion.opencv_vec().as_slice()[..4]);
            p.extend(&[abc.x, abc.y, abc.z]);
            p.extend(&[cc.x, cc.y, cc.z]);
            p
        }
        CameraModelType::Linear => {
            let mut p = vec![i.fx(), i.cx(), i.cy()];
            p.extend(&[abc.x, abc.y, abc.z]);
            p.extend(&[cc.x, cc.y, cc.z]);
            p
        }
        CameraModelType::ExtrinsicsOnly => {
            let mut p = vec![];
            p.extend(&[abc.x, abc.y, abc.z]);
            p.extend(&[cc.x, cc.y, cc.z]);
            p
        }
    }
}

/// Convert a (partial) parameter vector to a camera.
pub(crate) fn to_cam<F: na::RealField + Float>(
    params: &[F],
    model_type: CameraModelType,
    fixed_params: &[F],
) -> cam_geom::Camera<F, RosOpenCvIntrinsics<F>> {
    debug_assert_eq!(params.len(), model_type.info().num_cam_params());
    debug_assert_eq!(fixed_params.len(), model_type.info().num_fixed_params);
    let skew = na::convert(0.0);
    let mut distortion: [F; 5] = [na::convert(0.0); 5];
    let (fx, fy, cx, cy) = match &model_type {
        CameraModelType::OpenCV5 | CameraModelType::OpenCV4 => {
            let fx = params[0];
            let fy = fx;
            let cx = params[1];
            let cy = params[2];

            let nd = model_type.info().num_distortion_params;
            distortion[0..nd].copy_from_slice(&params[3..3 + nd]);
            (fx, fy, cx, cy)
        }
        CameraModelType::Linear => {
            let fx = params[0];
            let fy = fx;
            let cx = params[1];
            let cy = params[2];
            (fx, fy, cx, cy)
        }
        CameraModelType::ExtrinsicsOnly => {
            let fx = fixed_params[0];
            let fy = fixed_params[1];
            let cx = fixed_params[2];
            let cy = fixed_params[3];
            let d = &fixed_params[4..];
            distortion.copy_from_slice(d);
            (fx, fy, cx, cy)
        }
    };

    let distortion = opencv_ros_camera::Distortion::from_opencv_vec(
        na::Vector5::from_column_slice(&distortion[..]),
    );

    let intrinsics =
        RosOpenCvIntrinsics::from_params_with_distortion(fx, skew, fy, cx, cy, distortion);

    let extrinsics = {
        let eparams = &params[model_type.info().num_intrinsic_params..];
        let axisangle = na::Vector3::new(eparams[0], eparams[1], eparams[2]);
        let rquat = na::UnitQuaternion::new(axisangle);
        let camcenter = na::OPoint::from_slice(&eparams[3..]);
        cam_geom::ExtrinsicParameters::from_rotation_and_camcenter(rquat, camcenter)
    };

    cam_geom::Camera::new(intrinsics, extrinsics)
}

#[test]
fn test_cam_param_roundtrip() {
    for params in [
        [
            1.0, 2.0, 3.0, 0.01, 0.001, -0.01, -0.001, 0.0001, 0.2, 1.0, 0.0, 7.0, 8.0, 9.0,
        ],
        [
            0.1, 2.1, 3.1, 0.01, 0.001, -0.01, -0.001, 0.0001, 0.0, 0.0, 1.0, 7.1, 8.1, 9.1,
        ],
        [
            0.2, 2.2, 3.2, 0.01, 0.001, -0.01, -0.001, 0.0001, 0.0, 0.4, 0.5, 7.2, 8.2, 9.2,
        ],
    ] {
        let cam = to_cam::<f64>(&params, CameraModelType::OpenCV5, &[]);
        let p2 = to_params::<f64>(&cam, CameraModelType::OpenCV5);
        assert_eq!(p2.len(), CameraModelType::OpenCV5.info().num_cam_params());

        let orig = na::DVector::from_column_slice(&params);
        let extracted = na::DVector::from_column_slice(&p2);
        approx::assert_relative_eq!(orig, extracted, epsilon = 1.0e-6);
    }
}

#[test]
fn test_parameterization_extrinsics_only() {
    for full_params in [[
        1.0, 1.1, 2.0, 3.0, 0.01, 0.001, -0.01, -0.001, 0.0, 0.0, 1.0, 0.0, 7.0, 8.0, 9.0,
    ]] {
        let model_type = CameraModelType::ExtrinsicsOnly;
        let fixed_params = &full_params[..model_type.info().num_fixed_params];
        let params = &full_params[model_type.info().num_fixed_params..];
        // Part 1: roundtrip
        let cam = to_cam::<f64>(&params, CameraModelType::ExtrinsicsOnly, fixed_params);
        let p2 = to_params::<f64>(&cam, CameraModelType::ExtrinsicsOnly);
        assert_eq!(
            p2.len(),
            CameraModelType::ExtrinsicsOnly.info().num_cam_params()
        );

        let orig = na::DVector::from_column_slice(&params);
        let extracted = na::DVector::from_column_slice(&p2);
        approx::assert_relative_eq!(orig, extracted, epsilon = 1.0e-6);

        // Part 2: compare spot check values with expected.
        // px: 1.0, py: 2.0, pz: 3.0,
        let pts = cam_geom::Points::new(na::RowVector3::new(1.0, 2.0, 3.0));
        let predicted = cam.world_to_pixel(&pts).data.transpose();

        approx::assert_relative_eq!(predicted.x, -9.15875414775472, epsilon = 1.0e-10);
        approx::assert_relative_eq!(predicted.y, -6.21053637093717, epsilon = 1.0e-10);
    }
}
