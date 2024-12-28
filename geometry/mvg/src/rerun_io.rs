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

fn rr_translation_and_mat3<R: RealField>(
    extrinsics: &cam_geom::ExtrinsicParameters<R>,
) -> re_types::archetypes::Transform3D {
    let t = extrinsics.camcenter();
    let translation = re_types::datatypes::Vec3D([t[0].f32(), t[1].f32(), t[2].f32()]);
    let rot = extrinsics.rotation();
    let rot = rot.matrix();
    let mut col_major = [0.0; 9];
    for row in 0..3 {
        for col in 0..3 {
            let idx = col * 3 + row;
            col_major[idx] = rot[(col, row)].f32();
        }
    }
    let mat3x3 = re_types::datatypes::Mat3x3(col_major);
    re_types::archetypes::Transform3D::from_translation_mat3x3(translation, mat3x3)
}

pub trait AsRerunTransform3D {
    fn as_rerun_transform3d(&self) -> impl Into<re_types::archetypes::Transform3D>;
}

impl AsRerunTransform3D for cam_geom::ExtrinsicParameters<f64> {
    fn as_rerun_transform3d(&self) -> impl Into<re_types::archetypes::Transform3D> {
        rr_translation_and_mat3(self)
    }
}

#[test]
fn test_extrinsics_rerun_roundtrip() {
    use nalgebra::Vector3;
    let cc = Vector3::new(0.1, 2.3, 4.5);
    let lookdir = Vector3::new(1.0, 1.0, 0.1);
    let lookat = cc + lookdir;
    let up = Vector3::new(0.0, 0.0, 1.0);
    let up_unit = na::core::Unit::new_normalize(up);

    let orig = cam_geom::ExtrinsicParameters::<f64>::from_view(&cc, &lookat, &up_unit);
    let rmat = orig.rotation().matrix();

    // convert to re_types
    let rr = rr_translation_and_mat3(&orig);

    // check camcenter
    for i in 0..3 {
        approx::assert_relative_eq!(
            rr.translation.as_ref().unwrap()[i],
            cc[i] as f32,
            epsilon = 1e-10
        );
    }

    // check rotation
    for i in 0..3 {
        for j in 0..3 {
            approx::assert_relative_eq!(
                rr.mat3x3.as_ref().unwrap().col(j)[i],
                rmat[(j, i)] as f32,
                epsilon = 1e-10
            );
        }
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
    // Does re_types actually cameras with nonzero skew? Here we are say "yes", but actually it might not.
    for loc in [(1, 0), (2, 0), (2, 1)] {
        if cam.p[loc].clone().abs() > 1e-16.R() {
            return Err(MvgError::RerunUnsupportedIntrinsics);
        }
    }
    if (cam.p[(2, 2)].clone() - na::one()).abs() > 1e-16.R() {
        return Err(MvgError::RerunUnsupportedIntrinsics);
    }

    Ok(pinhole_projection_component_lossy(cam))
}

fn pinhole_projection_component_lossy<R: RealField>(
    cam: &RosOpenCvIntrinsics<R>,
) -> re_types::components::PinholeProjection {
    let fx = cam.p[(0, 0)].f32();
    let fy = cam.p[(1, 1)].f32();
    let cx = cam.p[(0, 2)].f32();
    let cy = cam.p[(1, 2)].f32();

    let m = re_types::datatypes::Mat3x3([fx, 0.0, 0.0, 0.0, fy, 0.0, cx, cy, 1.0]);
    re_types::components::PinholeProjection(m)
}

pub fn cam_geom_to_rr_pinhole_archetype<R: RealField>(
    cam: &cam_geom::Camera<R, cam_geom::IntrinsicParametersPerspective<R>>,
    width: usize,
    height: usize,
) -> re_types::archetypes::Pinhole {
    let i = cam.intrinsics();
    let m = re_types::datatypes::Mat3x3([
        i.fx().f32(),
        0.0,
        0.0,
        0.0,
        i.fy().f32(),
        0.0,
        i.cx().f32(),
        i.cy().f32(),
        1.0,
    ]);
    let image_from_camera = re_types::components::PinholeProjection(m);
    let resolution: re_types::datatypes::Vec2D = (width as f32, height as f32).into();
    re_types::archetypes::Pinhole {
        image_from_camera,
        resolution: Some(resolution.into()),
        camera_xyz: None,
        image_plane_distance: None,
    }
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
    pub fn rr_resolution_component(&self) -> re_types::components::Resolution {
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
        let image_from_camera = pinhole_projection_component(self.intrinsics())?;
        let resolution = Some(self.rr_resolution_component());
        Ok(re_types::archetypes::Pinhole {
            image_from_camera,
            resolution,
            camera_xyz: None,
            image_plane_distance: None,
        })
    }
    /// return a [re_types::archetypes::Pinhole]
    ///
    /// The conversion always succeed, even if the camera cannot be represented
    /// exactly in re_types.
    pub fn rr_pinhole_archetype_lossy(&self) -> re_types::archetypes::Pinhole {
        let image_from_camera = pinhole_projection_component_lossy(self.intrinsics());
        let resolution = Some(self.rr_resolution_component());
        re_types::archetypes::Pinhole {
            image_from_camera,
            resolution,
            camera_xyz: None,
            image_plane_distance: None,
        }
    }
}

#[test]
fn test_intrinsics_rerun_roundtrip() {
    // create camera, including intrinsics
    let orig = crate::Camera::<f64>::default();
    // convert to re_types
    let rr = orig.rr_pinhole_archetype().unwrap();
    // convert back from re_types
    let intrinsics = opencv_intrinsics::<f64>(&rr.image_from_camera);
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
    let rr = orig.rr_pinhole_archetype().unwrap();

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
