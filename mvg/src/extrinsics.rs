use nalgebra as na;
use nalgebra::core::Vector3;
use nalgebra::geometry::{Point3, UnitQuaternion};
use nalgebra::RealField;

use cam_geom::ExtrinsicParameters;

pub fn make_default_extrinsics<R: RealField + Copy>() -> ExtrinsicParameters<R> {
    let axis = na::core::Unit::new_normalize(Vector3::x());
    let angle = na::convert(0.0);
    let rquat = UnitQuaternion::from_axis_angle(&axis, angle);

    let camcenter = Point3::new(na::convert(1.0), na::convert(2.0), na::convert(3.0));
    ExtrinsicParameters::from_rotation_and_camcenter(rquat, camcenter)
}

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
    #[cfg(feature = "serde-serialize")]
    fn test_serde() {
        let expected = crate::extrinsics::make_default_extrinsics();
        let buf = serde_json::to_string(&expected).unwrap();
        let actual: crate::ExtrinsicParameters<f64> = serde_json::from_str(&buf).unwrap();
        assert!(expected == actual);
    }
}
