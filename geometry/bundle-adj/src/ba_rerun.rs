use nalgebra::{self as na, UnitQuaternion};
use num_traits::Float;

#[derive(Clone)]
pub(crate) struct BundleAdjusterRerun {
    pub(crate) cam_dims: Vec<(usize, usize)>,

    /// rerun viewer
    pub(crate) rec: Option<re_sdk::RecordingStream>,
    pub(crate) did_show_rerun_warning: bool,
}

// makes ExtrinsicParameters<F> into ExtrinsicParameters<f64>
pub(crate) fn extrinsics_f64<F: na::RealField + Float>(
    e: &cam_geom::ExtrinsicParameters<F>,
) -> cam_geom::ExtrinsicParameters<f64> {
    let r = e.pose().rotation.as_ref().coords;
    let rotation: UnitQuaternion<f64> = UnitQuaternion::from_quaternion(na::Quaternion {
        coords: na::Vector4::new(
            r[0].to_f64().unwrap(),
            r[1].to_f64().unwrap(),
            r[2].to_f64().unwrap(),
            r[3].to_f64().unwrap(),
        ),
    });
    let c = e.camcenter();
    let camcenter = na::Point3 {
        coords: na::Vector3::new(
            c[0].to_f64().unwrap(),
            c[1].to_f64().unwrap(),
            c[2].to_f64().unwrap(),
        ),
    };
    cam_geom::ExtrinsicParameters::from_rotation_and_camcenter(rotation, camcenter)
}

#[test]
fn test_extrinsics_f64() {
    let rotation: UnitQuaternion<f64> = UnitQuaternion::from_quaternion(na::Quaternion {
        coords: na::Vector4::new(1.0, 2.0, 3.0, 4.0), // will be normalized
    });
    let camcenter = na::Point3 {
        coords: na::Vector3::new(1.2, 3.4, 5.6),
    };
    let orig =
        cam_geom::ExtrinsicParameters::<f64>::from_rotation_and_camcenter(rotation, camcenter);
    let converted = extrinsics_f64(&orig);
    approx::assert_relative_eq!(orig.pose(), converted.pose(), epsilon = 1e-7);
}
