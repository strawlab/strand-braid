// Copyright 2016-2025 Andrew D. Straw.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT
// or http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use nalgebra as na;
use nalgebra::core::Vector3;
use nalgebra::geometry::{Point3, UnitQuaternion};
use nalgebra::RealField;

use cam_geom::ExtrinsicParameters;

/// Creates default extrinsic parameters for testing and prototyping.
///
/// This function generates reasonable default extrinsic parameters that place
/// the camera at a specific location with no rotation. The defaults are:
///
/// - **Camera center**: (1, 2, 3) in world coordinates
/// - **Rotation**: Identity (no rotation from world coordinate system)
///
/// These parameters are useful for algorithm testing, unit tests, and as
/// starting points for calibration procedures.
///
/// # ⚠️ Important Note
///
/// These parameters are **not suitable for real applications** - always perform
/// proper camera calibration for production use. The default position is arbitrary
/// and chosen only for testing convenience.
///
/// # Returns
///
/// [`ExtrinsicParameters`] with the default camera position and orientation.
///
/// # Example
///
/// ```rust
/// use braid_mvg::extrinsics::make_default_extrinsics;
///
/// let extrinsics = make_default_extrinsics::<f64>();
/// println!("Default camera center: {:?}", extrinsics.camcenter());
/// ```
pub fn make_default_extrinsics<R: RealField + Copy>() -> ExtrinsicParameters<R> {
    let axis = na::core::Unit::new_normalize(Vector3::x());
    let angle = na::convert(0.0);
    let rquat = UnitQuaternion::from_axis_angle(&axis, angle);

    let camcenter = Point3::new(na::convert(1.0), na::convert(2.0), na::convert(3.0));
    ExtrinsicParameters::from_rotation_and_camcenter(rquat, camcenter)
}

/// Creates extrinsic parameters from a rotation quaternion and translation vector.
///
/// This function constructs camera extrinsic parameters from a rotation represented
/// as a unit quaternion and a translation vector. This is a common parameterization
/// used in robotics and SLAM applications.
///
/// # Mathematical Details
///
/// The relationship between camera center `C` and translation vector `t` is:
/// ```text
/// t = -R * C
/// C = -R^T * t
/// ```
/// where `R` is the rotation matrix corresponding to `rquat`.
///
/// # Arguments
///
/// * `rquat` - Unit quaternion representing the camera rotation
/// * `translation` - Translation vector from world origin to camera position
///
/// # Returns
///
/// [`ExtrinsicParameters`] constructed from the rotation and translation
///
/// # Example
///
/// ```rust
/// use braid_mvg::extrinsics::from_rquat_translation;
/// use nalgebra::{UnitQuaternion, Point3, Vector3};
///
/// let rotation = UnitQuaternion::from_axis_angle(&Vector3::z_axis(), 0.5);
/// let translation = Point3::new(1.0, 2.0, 3.0);
///
/// let extrinsics = from_rquat_translation(rotation, translation);
/// ```
pub fn from_rquat_translation<R: RealField + Copy>(
    rquat: UnitQuaternion<R>,
    translation: Point3<R>,
) -> ExtrinsicParameters<R> {
    let camcenter = -(rquat.inverse() * translation);
    ExtrinsicParameters::from_rotation_and_camcenter(rquat, camcenter)
}

#[cfg(test)]
mod tests {
    use cam_geom::ExtrinsicParameters;
    use na::geometry::Point3;
    use na::Vector3;
    use nalgebra as na;

    #[test]
    fn test_from_view() {
        let cc = Vector3::new(0.1, 2.3, 4.5);
        let lookdir = Vector3::new(1.0, 0.0, 0.0);
        let lookat = cc + lookdir;
        let up = Vector3::new(0.0, 0.0, 1.0);
        let up_unit = na::core::Unit::new_normalize(up);

        let extrinsics1 = ExtrinsicParameters::from_view(&cc, &lookat, &up_unit);
        let extrinsics2 = ExtrinsicParameters::from_pose(extrinsics1.pose());

        // We have a right-handed system but use `look_at_lh` have +z axis being
        // forward. We flip the up axis to keep our right-handedness.
        let pose = nalgebra::Isometry3::look_at_lh(
            &Point3 { coords: cc },
            &Point3 { coords: lookat },
            &-up_unit,
        );
        let extrinsics3 = ExtrinsicParameters::from_pose(&pose);

        // println!("extrinsics1 {:?}", extrinsics1);
        // println!("extrinsics2 {:?}", extrinsics2);
        // println!("extrinsics3 {:?}", extrinsics3);

        for extrinsics in &[extrinsics1, extrinsics2, extrinsics3] {
            // println!("{:?} {}:{}", extrinsics, file!(), line!());
            let iso = extrinsics.pose();
            approx::assert_relative_eq!(
                iso * Point3 { coords: cc },
                Point3::origin(),
                epsilon = 1e-10
            );

            let zero = na::convert(0.0);
            let one = na::convert(1.0);
            let forward_cam: cam_geom::Points<cam_geom::CameraFrame, _, _, _> =
                cam_geom::Points::new(na::Matrix1x3::new(zero, zero, one));
            let lookat_actual1: crate::PointWorldFrame<_> =
                extrinsics.camera_to_world(&forward_cam).into();
            let lookat_actual = lookat_actual1.coords.coords;
            let lookdir_actual = lookat_actual - cc;

            let up_cam: cam_geom::Points<cam_geom::CameraFrame, _, _, _> =
                cam_geom::Points::new(na::Matrix1x3::new(zero, -one, zero));

            let up_actual1: crate::PointWorldFrame<_> = extrinsics.camera_to_world(&up_cam).into();
            let up_actual = up_actual1.coords.coords - cc;

            approx::assert_relative_eq!(lookat, lookat_actual, epsilon = 1e-10);

            approx::assert_relative_eq!(lookdir, lookdir_actual, epsilon = 1e-10);

            approx::assert_relative_eq!(up, up_actual, epsilon = 1e-10);
        }
    }

    #[test]
    fn test_serde() {
        let expected = crate::extrinsics::make_default_extrinsics();
        let buf = serde_json::to_string(&expected).unwrap();
        let actual: crate::ExtrinsicParameters<f64> = serde_json::from_str(&buf).unwrap();
        assert!(expected == actual);
    }
}
