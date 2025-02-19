use nalgebra::{self as na, RealField};
use opencv_ros_camera::RosOpenCvIntrinsics;

use crate::MvgError;

// conversion helpers -----------

trait ToF32 {
    fn f32(&self) -> f32;
}

impl<R> ToF32 for R
where
    R: RealField,
{
    #[inline]
    fn f32(&self) -> f32 {
        <R as simba::scalar::SupersetOf<f32>>::to_subset(self).unwrap()
    }
}

// trait ToF64 {
//     fn f64(&self) -> f64;
// }

// impl<R> ToF64 for R
// where
//     R: RealField,
// {
//     fn f64(&self) -> f64 {
//         self.to_subset().unwrap()
//     }
// }

trait ToR<T: RealField> {
    #[allow(non_snake_case)]
    fn R(&self) -> T;
}

impl<T: RealField> ToR<T> for f32 {
    #[inline]
    fn R(&self) -> T {
        na::convert(*self as f64)
    }
}

impl<T: RealField> ToR<T> for f64 {
    #[inline]
    fn R(&self) -> T {
        na::convert(*self)
    }
}

// extrinsics -----------

fn rr_translation_and_rotation<R: RealField>(
    extrinsics: &cam_geom::ExtrinsicParameters<R>,
) -> (
    re_types::components::Translation3D,
    re_types::components::RotationQuat,
) {
    use re_types::{components::RotationQuat, datatypes::Quaternion};
    let r = &extrinsics.pose().rotation.coords;
    // convert from nalgebra to rerun
    let rquat = RotationQuat(Quaternion::from_xyzw([
        r.x.f32(),
        r.y.f32(),
        r.z.f32(),
        r.w.f32(),
    ]));
    let t = extrinsics.translation();
    let translation = re_types::datatypes::Vec3D([t[0].f32(), t[1].f32(), t[2].f32()]).into();
    (translation, rquat)
}

pub trait AsRerunTransform3D {
    fn as_rerun_transform3d(&self) -> impl Into<re_types::archetypes::Transform3D>;
}

impl AsRerunTransform3D for cam_geom::ExtrinsicParameters<f64> {
    fn as_rerun_transform3d(&self) -> impl Into<re_types::archetypes::Transform3D> {
        use re_types::components::TransformRelation;
        let (t, r) = rr_translation_and_rotation(self);
        re_types::archetypes::Transform3D::from_translation_rotation(t, r)
            .with_relation(TransformRelation::ChildFromParent)
    }
}

#[test]
fn test_rerun_transform3d() {
    use nalgebra::{Matrix3xX, Vector3};
    use re_types::external::glam::{self, Affine3A};

    // You can log this to rerun with the `export-rerun-log` example.

    // Create extrinsic parameters
    let cc = Vector3::new(3.0, 2.0, 1.0);
    let lookat = Vector3::new(0.0, 0.0, 0.0);
    let up = Vector3::new(0.0, 0.0, 1.0);
    let up_unit = na::core::Unit::new_normalize(up);

    let extrinsics = cam_geom::ExtrinsicParameters::from_view(&cc, &lookat, &up_unit);

    #[rustfmt::skip]
    let points3d = Matrix3xX::<f64>::from_column_slice(
        &[
        0.0, 0.0, 0.0,
        1.0, 0.0, 0.0,
        1.0, 1.0, 0.0,
        0.0, 1.0, 0.0,
        0.0, 0.0, 1.0,
        1.0, 0.0, 1.0,
        1.0, 1.0, 1.0,
        0.0, 1.0, 1.0,
    ]);

    // Transform points using cam_geom
    let world = cam_geom::Points::new(points3d.transpose());
    let cam = extrinsics.world_to_camera(&world);

    // Transform points using rerun/glam
    let (rrt, rrq) = rr_translation_and_rotation(&extrinsics);
    let glam_t: Affine3A = rrt.into();
    let glam_r: Affine3A = rrq.try_into().unwrap();
    for i in 0..points3d.ncols() {
        let (x, y, z) = (points3d[(0, i)], points3d[(1, i)], points3d[(2, i)]);
        let pt = glam::Vec3::new(x as f32, y as f32, z as f32);
        let p2 = glam_t.transform_point3(glam_r.transform_point3(pt));

        // Now test transformed point
        approx::assert_relative_eq!(p2.x as f64, cam.data[(i, 0)], epsilon = 1e-6);
        approx::assert_relative_eq!(p2.y as f64, cam.data[(i, 1)], epsilon = 1e-6);
        approx::assert_relative_eq!(p2.z as f64, cam.data[(i, 2)], epsilon = 1e-6);
    }
}

// intrinsics -----------

fn pinhole_projection_component<R: RealField>(
    cam: &RosOpenCvIntrinsics<R>,
) -> Result<re_types::components::PinholeProjection, MvgError> {
    // Check if camera can be exactly represented in re_types.
    if !cam.distortion.is_linear() {
        return Err(MvgError::RerunUnsupportedIntrinsics);
    }
    // re_types does not model all possible intrinsic matrices, raise error.
    for loc in [(0, 1), (1, 0), (2, 0), (2, 1)] {
        if cam.p[loc].clone().abs() > 1e-16.R() {
            return Err(MvgError::RerunUnsupportedIntrinsics);
        }
    }
    if (cam.p[(2, 2)].clone() - na::one()).abs() > 1e-16.R() {
        return Err(MvgError::RerunUnsupportedIntrinsics);
    }

    let fx = cam.p[(0, 0)].f32();
    let fy = cam.p[(1, 1)].f32();
    let cx = cam.p[(0, 2)].f32();
    let cy = cam.p[(1, 2)].f32();

    Ok(
        re_types::components::PinholeProjection::from_focal_length_and_principal_point(
            (fx, fy),
            (cx, cy),
        ),
    )
}

pub fn cam_geom_to_rr_pinhole_archetype<R: RealField>(
    intrinsics: &cam_geom::IntrinsicParametersPerspective<R>,
    width: usize,
    height: usize,
) -> Result<re_types::archetypes::Pinhole, MvgError> {
    let i = intrinsics;
    if i.skew().f32().abs() > 1e-10 {
        return Err(MvgError::RerunUnsupportedIntrinsics);
    }
    let image_from_camera =
        re_types::components::PinholeProjection::from_focal_length_and_principal_point(
            (i.fx().f32(), i.fy().f32()),
            (i.cx().f32(), i.cy().f32()),
        );
    let resolution: re_types::datatypes::Vec2D = (width as f32, height as f32).into();
    Ok(re_types::archetypes::Pinhole::new(image_from_camera).with_resolution(resolution))
}

#[cfg(test)]
fn opencv_intrinsics<R: RealField>(
    orig: &re_types::components::PinholeProjection,
) -> RosOpenCvIntrinsics<R> {
    let m = orig.0;
    let col0 = m.col(0);
    let col1 = m.col(1);
    let col2 = m.col(2);
    let fx = col0[0].R();
    let skew = col1[0].R();
    let fy = col1[1].R();
    let cx = col2[0].R();
    let cy = col2[1].R();
    RosOpenCvIntrinsics::from_params(fx, skew, fy, cx, cy)
}

impl<R: RealField + Copy> crate::Camera<R> {
    /// return [re_types::components::Resolution]
    fn rr_resolution_component(&self) -> re_types::components::Resolution {
        re_types::components::Resolution(re_types::datatypes::Vec2D::new(
            self.width() as f32,
            self.height() as f32,
        ))
    }

    /// return a [re_types::archetypes::Pinhole]
    ///
    /// The conversion will not succeed if the camera cannot be represented
    /// exactly in re_types.
    pub fn rr_pinhole_archetype(&self) -> Result<re_types::archetypes::Pinhole, MvgError> {
        let pinhole_projection = pinhole_projection_component(self.intrinsics())?;
        let resolution = self.rr_resolution_component();
        Ok(re_types::archetypes::Pinhole::new(pinhole_projection).with_resolution(resolution))
    }
}

#[test]
fn test_intrinsics_rerun_roundtrip() {
    // create camera, including intrinsics
    let orig = crate::Camera::<f64>::default();
    // convert to re_types
    let rr = pinhole_projection_component(orig.intrinsics()).unwrap();
    // convert back from re_types
    let intrinsics = opencv_intrinsics::<f64>(&rr);
    // compare original intrinsics with converted
    assert_eq!(orig.intrinsics(), &intrinsics);
}

#[test]
fn test_intrinsics_rerun() {
    use cam_geom::Points;
    use nalgebra::{OMatrix, U3, U7};

    // create camera, including intrinsics
    let orig = crate::Camera::<f64>::default();
    // convert to re_types
    let rr = pinhole_projection_component(orig.intrinsics()).unwrap();

    let orig_intrinsics = orig.intrinsics();

    #[rustfmt::skip]
    let pts = Points::new(
        OMatrix::<f64, U7, U3>::from_row_slice(
            &[0.0,  0.0, 1.0,
            1.0,  0.0, 1.0,
            0.0,  1.0, 1.0,
            1.0,  1.0, 1.0,
            -1.0,  0.0, 1.0,
            0.0, -1.0, 1.0,
            -1.0, -1.0, 1.0]
        )
    );

    let pixels_orig = orig_intrinsics.camera_to_undistorted_pixel(&pts);

    for (pt3d, px_orig) in pts.data.row_iter().zip(pixels_orig.data.row_iter()) {
        let pt3d_v2 =
            re_types::external::glam::Vec3::new(pt3d[0] as f32, pt3d[1] as f32, pt3d[2] as f32);
        let pix_rr = rr.project(pt3d_v2);
        approx::assert_relative_eq!(pix_rr.x, px_orig[0] as f32, epsilon = 1e-10);
        approx::assert_relative_eq!(pix_rr.y, px_orig[1] as f32, epsilon = 1e-10);
    }
}
