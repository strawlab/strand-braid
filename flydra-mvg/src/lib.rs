#![cfg_attr(feature = "backtrace", feature(backtrace))]

use std::collections::BTreeMap;
use std::io::{Read, Write};

extern crate log;

use serde::de::DeserializeOwned;

use num_traits::{One, Zero};

use na::core::dimension::{U2, U3, U4};
use na::core::{Matrix3, OMatrix, Vector3, Vector5};
use na::geometry::Point3;
use na::RealField;
use na::{allocator::Allocator, DefaultAllocator, U1};
use nalgebra as na;

use cam_geom::ExtrinsicParameters;
use ncollide::{query::Ray, shape::Plane};
use ncollide3d as ncollide;
use opencv_ros_camera::{Distortion, RosOpenCvIntrinsics};

use mvg::{
    rq_decomposition, vec_sum, Camera, DistortedPixel, MultiCameraSystem, MvgError,
    PointWorldFrame, PointWorldFrameMaybeWithSumReprojError, PointWorldFrameWithSumReprojError,
    UndistortedPixel, WorldCoordAndUndistorted2D,
};

mod fermats_least_time;

pub mod flydra_xml_support;

use crate::flydra_xml_support::{FlydraDistortionModel, SingleCameraCalibration};

const AIR_REFRACTION: f64 = 1.0003;

pub use mvg::Result;

// MultiCameraIter -------------------------------------------------------

/// implements an `Iterator` which returns cameras as `MultiCamera`s.
pub struct MultiCameraIter<'a, 'b, R: RealField + Copy + Default + serde::Serialize> {
    name_iter: CamNameIter<'b, R>,
    flydra_system: &'a FlydraMultiCameraSystem<R>,
}

impl<'a, 'b, R: RealField + Copy + Default + serde::Serialize> Iterator
    for MultiCameraIter<'a, 'b, R>
{
    type Item = MultiCamera<R>;
    fn next(&mut self) -> Option<Self::Item> {
        self.name_iter
            .next()
            .map(|name| self.flydra_system.cam_by_name(name).unwrap())
    }
}

// CamNameIter -------------------------------------------------------

/// implements an `Iterator` which returns camera names as `&str`s.
pub struct CamNameIter<'a, R: RealField + Copy + Default + serde::Serialize>(
    std::collections::btree_map::Keys<'a, String, Camera<R>>,
);

impl<'a, R: RealField + Copy + Default + serde::Serialize> Iterator for CamNameIter<'a, R> {
    type Item = &'a str;
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(AsRef::as_ref)
    }
}

// RayCamera -------------------------------------------------------

/// defines operations with Ray type
///
/// Rays can be easier to work with when the camera system may have water as
/// rays are defined from an origin (typically the camera center) in a
/// direction rather than a point in 3D space, which may be on the other side
/// of a refractive boundary.
trait RayCamera<R: RealField + Copy> {
    fn project_pixel_to_ray(&self, pt: &UndistortedPixel<R>) -> ncollide3d::query::Ray<R>;
    fn project_distorted_pixel_to_ray(&self, pt2d: &DistortedPixel<R>)
        -> ncollide3d::query::Ray<R>;
    fn project_ray_to_distorted_pixel(&self, ray: &ncollide3d::query::Ray<R>) -> DistortedPixel<R>;
    fn project_ray_to_pixel(&self, ray: &ncollide3d::query::Ray<R>) -> UndistortedPixel<R>;
}

impl<R: RealField + Copy + Default + serde::Serialize> RayCamera<R> for Camera<R> {
    fn project_pixel_to_ray(&self, pt: &UndistortedPixel<R>) -> ncollide3d::query::Ray<R> {
        let dist = na::convert(1.0);
        let p2 = self.project_pixel_to_3d_with_dist(pt, dist);
        let ray_origin = *self.extrinsics().camcenter();
        let ray_dir = p2.coords - ray_origin;
        ncollide3d::query::Ray::new(ray_origin, ray_dir)
    }

    fn project_distorted_pixel_to_ray(
        &self,
        pt2d: &DistortedPixel<R>,
    ) -> ncollide3d::query::Ray<R> {
        let undistorted = self.intrinsics().undistort(&pt2d.into());
        self.project_pixel_to_ray(&undistorted.into())
    }

    fn project_ray_to_distorted_pixel(&self, ray: &ncollide3d::query::Ray<R>) -> DistortedPixel<R> {
        debug_assert!(&ray.origin == self.extrinsics().camcenter());
        let pt3d = PointWorldFrame {
            coords: ray.origin + ray.dir,
        };
        self.project_3d_to_distorted_pixel(&pt3d)
    }

    fn project_ray_to_pixel(&self, ray: &ncollide3d::query::Ray<R>) -> UndistortedPixel<R> {
        debug_assert!(&ray.origin == self.extrinsics().camcenter());
        let pt3d = PointWorldFrame {
            coords: ray.origin + ray.dir,
        };
        self.project_3d_to_pixel(&pt3d)
    }
}

// MultiCamera -------------------------------------------------------

/// A camera which may be looking at water
///
/// Note that we specifically do not have the methods
/// `project_distorted_pixel_to_3d_with_dist` and `project_pixel_to_3d_with_dist`
/// because these are dangerous in the sense that depending on `dist`, the
/// resulting pixel may be subject to refraction. Instead, we have only the
/// ray based methods.
#[derive(Clone, Debug)]
pub struct MultiCamera<R: RealField + Copy + Default + serde::Serialize> {
    water: Option<R>,
    name: String,
    cam: Camera<R>,
}

impl<R: RealField + Copy + Default + serde::Serialize> MultiCamera<R> {
    pub fn to_cam(self) -> Camera<R> {
        self.cam
    }

    #[inline]
    pub fn project_pixel_to_ray(&self, pt: &UndistortedPixel<R>) -> Ray<R> {
        self.cam.project_pixel_to_ray(pt)
    }

    #[inline]
    pub fn project_distorted_pixel_to_ray(&self, pt: &DistortedPixel<R>) -> Ray<R> {
        self.cam.project_distorted_pixel_to_ray(pt)
    }

    #[inline]
    pub fn project_ray_to_pixel(&self, ray: &ncollide3d::query::Ray<R>) -> UndistortedPixel<R> {
        self.cam.project_ray_to_pixel(ray)
    }

    #[inline]
    pub fn project_ray_to_distorted_pixel(
        &self,
        ray: &ncollide3d::query::Ray<R>,
    ) -> DistortedPixel<R> {
        self.cam.project_ray_to_distorted_pixel(ray)
    }

    /// projects a 3D point to a ray
    ///
    /// If the point is under water, the ray is in the direction the camera
    /// sees it (not the straight-line direction).
    pub fn project_3d_to_ray(&self, pt3d: &PointWorldFrame<R>) -> Ray<R> {
        let camcenter = self.extrinsics().camcenter();

        let dir: Vector3<R> = if self.water.is_some() && pt3d.coords[2] < na::convert(0.0) {
            // this is tag "laksdfjasl".
            let n1 = na::convert(AIR_REFRACTION);
            let n2 = self.water.unwrap();

            let camcenter_z0 =
                Point3::from(Vector3::new(camcenter[0], camcenter[1], na::convert(0.0)));
            let shifted_pt = pt3d.coords - camcenter_z0; // origin under cam at surface. cam at (0,0,z).
            let theta = shifted_pt[1].atan2(shifted_pt[0]); // angles to points
            let r = (shifted_pt[0].powi(2) + shifted_pt[1].powi(2)).sqrt(); // horizontal dist
            let depth = -shifted_pt[2];
            let height = camcenter[2];

            let water_roots_eps = na::convert(1e-5);
            let root_params =
                fermats_least_time::RootParams::new(n1, n2, height, r, depth, water_roots_eps);
            let r0 = match fermats_least_time::find_fastest_path_fermat(&root_params) {
                Ok(r0) => r0,
                Err(e) => {
                    log::error!(
                        "find_fastest_path_fermat {} with parameters: {:?}",
                        e,
                        root_params,
                    );
                    panic!(
                        "find_fastest_path_fermat {} with parameters: {:?}",
                        e, root_params,
                    );
                }
            };

            let shifted_water_surface_pt =
                Vector3::new(r0 * theta.cos(), r0 * theta.sin(), na::convert(0.0));

            let water_surface_pt = shifted_water_surface_pt + camcenter_z0.coords;
            water_surface_pt - camcenter.coords
        } else {
            pt3d.coords - camcenter
        };
        debug_assert!(
            na::Matrix::norm(&dir) > R::default_epsilon(),
            "pt3d is at camcenter"
        );
        Ray::new(*camcenter, dir)
    }

    #[allow(non_snake_case)]
    pub fn linearize_numerically_at(
        &self,
        center: &PointWorldFrame<R>,
        delta: R,
    ) -> Result<OMatrix<R, U2, U3>> {
        let zero = na::convert(0.0);

        let dx = Vector3::<R>::new(delta, zero, zero);
        let dy = Vector3::<R>::new(zero, delta, zero);
        let dz = Vector3::<R>::new(zero, zero, delta);

        let center_x = PointWorldFrame {
            coords: center.coords + dx,
        };
        let center_y = PointWorldFrame {
            coords: center.coords + dy,
        };
        let center_z = PointWorldFrame {
            coords: center.coords + dz,
        };

        let F = self.project_3d_to_pixel(center).coords;
        let Fx = self.project_3d_to_pixel(&center_x).coords;
        let Fy = self.project_3d_to_pixel(&center_y).coords;
        let Fz = self.project_3d_to_pixel(&center_z).coords;

        let dF_dx = (Fx - F) / delta;
        let dF_dy = (Fy - F) / delta;
        let dF_dz = (Fz - F) / delta;

        Ok(OMatrix::<R, U2, U3>::new(
            dF_dx[0], dF_dy[0], dF_dz[0], dF_dx[1], dF_dy[1], dF_dz[1],
        ))
    }

    pub fn project_3d_to_pixel(&self, pt3d: &PointWorldFrame<R>) -> UndistortedPixel<R> {
        let ray = self.project_3d_to_ray(pt3d); // This handles water correctly
                                                // (i.e. a 3D point is not necessarily seen with the ray direct from the cam center
                                                // to that 3D point).

        // From here, we use normal camera stuff (no need to know about water).
        let coords = ray.origin + ray.dir;
        let pt_air =
            cam_geom::Points::<cam_geom::WorldFrame, _, _, _>::new(coords.coords.transpose());

        use opencv_ros_camera::CameraExt;
        let pt_undistorted = self.cam.as_ref().world_to_undistorted_pixel(&pt_air);

        pt_undistorted.into()
    }

    pub fn project_3d_to_distorted_pixel(&self, pt3d: &PointWorldFrame<R>) -> DistortedPixel<R>
    where
        DefaultAllocator: Allocator<R, U1, U2>,
    {
        let undistorted = self.project_3d_to_pixel(pt3d);
        let u2: opencv_ros_camera::UndistortedPixels<R, U1, _> = (&undistorted).into();
        self.cam.intrinsics().distort(&u2).into()
    }

    #[inline]
    pub fn extrinsics(&self) -> &ExtrinsicParameters<R> {
        self.cam.extrinsics()
    }

    /// Return the intrinsic parameters, but probably does not do what you want.
    ///
    /// Commenting out for now. Probably does not do what you think it does. In particular,
    /// since we may have a refractive boundary, cannot just simply map coordinates between
    /// 2d pixel coordinate and 3D ray, because after the boundary, the ray will be different.
    pub fn do_not_use_intrinsics(&self) -> &RosOpenCvIntrinsics<R> {
        self.cam.intrinsics()
    }

    pub fn undistort(&self, a: &mvg::DistortedPixel<R>) -> mvg::UndistortedPixel<R> {
        let a2: cam_geom::Pixels<R, U1, _> = a.into();
        let b1: opencv_ros_camera::UndistortedPixels<R, U1, _> =
            self.cam.intrinsics().undistort(&a2);
        b1.into()
    }

    #[inline]
    pub fn width(&self) -> usize {
        self.cam.width()
    }

    #[inline]
    pub fn height(&self) -> usize {
        self.cam.height()
    }

    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }
}

// FlydraMultiCameraSystem ----------------------------------------------------

#[derive(Clone, Debug)]
pub struct FlydraMultiCameraSystem<R: RealField + Copy + serde::Serialize> {
    system: MultiCameraSystem<R>,
    water: Option<R>,
}

impl<R: RealField + Copy + Default + serde::Serialize> FlydraMultiCameraSystem<R> {
    pub fn from_system(system: MultiCameraSystem<R>, water: Option<R>) -> Self {
        FlydraMultiCameraSystem { system, water }
    }

    pub fn to_system(self) -> MultiCameraSystem<R> {
        self.system
    }

    pub fn new(cams_by_name: BTreeMap<String, Camera<R>>, water: Option<R>) -> Self {
        let system = MultiCameraSystem::new(cams_by_name);

        FlydraMultiCameraSystem { system, water }
    }

    pub fn len(&self) -> usize {
        self.system.cams().len()
    }

    pub fn is_empty(&self) -> bool {
        self.system.cams().is_empty()
    }

    pub fn cam_by_name(&self, name: &str) -> Option<MultiCamera<R>> {
        self.system.cam_by_name(name).map(|cam| MultiCamera {
            water: self.water,
            name: name.to_string(),
            cam: cam.clone(),
        })
    }

    pub fn cam_names(&self) -> CamNameIter<'_, R> {
        CamNameIter(self.system.cams().keys())
    }

    pub fn cameras(&self) -> MultiCameraIter<R> {
        let name_iter = self.cam_names();
        MultiCameraIter {
            name_iter,
            flydra_system: self,
        }
    }

    pub fn find3d_and_cum_reproj_dist_distorted(
        &self,
        points: &[(String, DistortedPixel<R>)],
    ) -> Result<PointWorldFrameWithSumReprojError<R>> {
        use crate::PointWorldFrameMaybeWithSumReprojError::*;

        let x = self.find3d_distorted(points)?;
        let (pt, upoints) = x.wc_and_upoints();
        match pt {
            WithSumReprojError(wsre) => Ok(wsre),
            Point(point) => {
                let reproj_dists = self.get_reprojection_undistorted_dists(&upoints, &point)?;
                Ok(PointWorldFrameWithSumReprojError::new(point, reproj_dists))
            }
        }
    }

    /// Find 3D coordinate using pixel coordinates from cameras
    ///
    /// If the system has water, two evaluations are done: one
    /// for the case of the 3D point being under water, the other
    /// for the case of the 3D point being in air. The evaluation
    /// with the lowest mean reprojection error is selected.
    pub fn find3d(
        &self,
        points: &[(String, UndistortedPixel<R>)],
    ) -> Result<PointWorldFrameMaybeWithSumReprojError<R>> {
        if points.len() < 2 {
            return Err(MvgError::NotEnoughPoints);
        }

        use crate::PointWorldFrameMaybeWithSumReprojError::*;

        match self.water {
            Some(n2) => {
                // TODO: would it be possible to have a 3d reconstruction with
                // lower reprojection error when it was z<0 but with the air
                // based calculation? This would seem problematic...
                let opt_water_3d_pt = match self.find3d_water(points, n2) {
                    Ok(water_3d_pt) => Some(water_3d_pt),
                    Err(MvgError::CamGeomError { .. }) => None,
                    Err(e) => {
                        return Err(e);
                    }
                };
                let air_3d_pt = self.find3d_air(points)?;

                let air_dists = self.get_reprojection_undistorted_dists(points, &air_3d_pt)?;
                let air_dist_sum = vec_sum(&air_dists);

                if let Some(water_3d_pt) = opt_water_3d_pt {
                    let water_dists =
                        self.get_reprojection_undistorted_dists(points, &water_3d_pt)?;
                    let water_dist_sum = vec_sum(&water_dists);
                    if water_dist_sum < air_dist_sum {
                        Ok(WithSumReprojError(PointWorldFrameWithSumReprojError::new(
                            water_3d_pt,
                            water_dists,
                        )))
                    } else {
                        Ok(WithSumReprojError(PointWorldFrameWithSumReprojError::new(
                            air_3d_pt, air_dists,
                        )))
                    }
                } else {
                    Ok(WithSumReprojError(PointWorldFrameWithSumReprojError::new(
                        air_3d_pt, air_dists,
                    )))
                }
            }
            None => Ok(Point(self.system.find3d(points)?)),
        }
    }

    pub fn find3d_distorted(
        &self,
        points: &[(String, DistortedPixel<R>)],
    ) -> Result<WorldCoordAndUndistorted2D<R>> {
        let upoints: Vec<(String, UndistortedPixel<R>)> = points
            .iter()
            .filter_map(|&(ref name, ref pt)| {
                self.cam_by_name(name)
                    .map(|cam| (name.clone(), cam.undistort(pt)))
            })
            .collect();
        if upoints.len() != points.len() {
            return Err(MvgError::UnknownCamera);
        }
        Ok(WorldCoordAndUndistorted2D::new(
            self.find3d(&upoints)?,
            upoints,
        ))
    }

    fn find3d_water(
        &self,
        points: &[(String, UndistortedPixel<R>)],
        n2: R,
    ) -> Result<PointWorldFrame<R>> {
        use cam_geom::{Ray, WorldFrame};
        let z0 = Plane::new(Vector3::z_axis());
        let eye = na::Isometry3::identity();

        let mut rays: Vec<Ray<WorldFrame, _>> = Vec::with_capacity(points.len());

        for &(ref name, ref xy) in points.iter() {
            let cam = self.cam_by_name(name).ok_or(MvgError::UnknownCamera)?;
            let air_ray = cam.project_pixel_to_ray(xy);
            let solid = false; // will intersect either side of plane

            use ncollide::query::RayCast;
            let opt_surface_pt_toi: Option<R> =
                z0.toi_with_ray(&eye, &air_ray, R::max_value(), solid);

            if let Some(toi) = opt_surface_pt_toi {
                let surface_pt = air_ray.origin + air_ray.dir * toi;

                // closest point to camera on water surface, assumes water at z==0
                let camcenter = &air_ray.origin;
                let camcenter_z0 =
                    Point3::from(Vector3::new(camcenter[0], camcenter[1], na::convert(0.0)));

                let surface_pt_cam = surface_pt - camcenter_z0;

                // Get underwater line from water surface (using Snell's Law).
                let y = surface_pt_cam[1];
                let x = surface_pt_cam[0];
                let pt_angle = y.atan2(x);
                let pt_horiz_dist = (x * x + y * y).sqrt(); // horizontal distance from camera to water surface
                let theta_air = pt_horiz_dist.atan2(camcenter[2]);

                // sin(theta_water)/sin(theta_air) = sin(n_air)/sin(n_water)
                let n_air = na::convert(AIR_REFRACTION);
                let sin_theta_water = theta_air.sin() * n_air / n2;
                let theta_water = sin_theta_water.asin();
                let horiz_dist_at_depth_1 = theta_water.tan();
                let horiz_dist_cam_depth_1 = horiz_dist_at_depth_1 + pt_horiz_dist; // total horizontal distance
                let deep_pt_cam = Vector3::new(
                    horiz_dist_cam_depth_1 * pt_angle.cos(),
                    horiz_dist_cam_depth_1 * pt_angle.sin(),
                    na::convert(-1.0),
                );
                let deep_pt = deep_pt_cam + camcenter_z0.coords;
                let water_ray_dir = deep_pt - surface_pt.coords;
                rays.push(Ray::new(
                    surface_pt.coords.transpose(),
                    water_ray_dir.transpose(),
                ));
            }
        }
        let pt = cam_geom::best_intersection_of_rays(&rays)?;
        Ok(pt.into())
    }

    /// Find 3D coordinate using pixel coordinates from cameras
    fn find3d_air(&self, points: &[(String, UndistortedPixel<R>)]) -> Result<PointWorldFrame<R>> {
        self.system.find3d(points)
    }

    /// Find reprojection error of 3D coordinate into pixel coordinates
    pub fn get_reprojection_undistorted_dists(
        &self,
        points: &[(String, UndistortedPixel<R>)],
        this_3d_pt: &PointWorldFrame<R>,
    ) -> Result<Vec<R>> {
        let this_dists = points
            .iter()
            .map(|&(ref cam_name, ref orig)| {
                Ok(na::distance(
                    &self
                        .cam_by_name(cam_name)
                        .ok_or(MvgError::UnknownCamera)?
                        .project_3d_to_pixel(this_3d_pt)
                        .coords,
                    &orig.coords,
                ))
            })
            .collect::<Result<Vec<R>>>()?;
        Ok(this_dists)
    }

    pub fn from_flydra_reconstructor(
        recon: &flydra_xml_support::FlydraReconstructor<R>,
    ) -> Result<Self> {
        let water = recon.water;
        let mut cams = BTreeMap::new();
        for flydra_cam in recon.cameras.iter() {
            let (name, cam) = Camera::from_flydra(flydra_cam)?;
            cams.insert(name, cam);
        }
        let _ = recon.minimum_eccentricity;
        Ok(Self::new(cams, water))
    }

    pub fn to_flydra_reconstructor(&self) -> Result<flydra_xml_support::FlydraReconstructor<R>> {
        let cameras: Result<Vec<flydra_xml_support::SingleCameraCalibration<R>>> = self
            .system
            .cams_by_name()
            .iter()
            .map(|(name, cam)| {
                let flydra_cam: flydra_xml_support::SingleCameraCalibration<R> =
                    cam.to_flydra(name)?;
                Ok(flydra_cam)
            })
            .collect();
        let cameras = cameras?;
        let water = self.water;

        Ok(flydra_xml_support::FlydraReconstructor {
            cameras,
            comment: self.system.comment().cloned(),
            water,
            minimum_eccentricity: na::convert(0.0),
        })
    }
}

impl<R> FlydraMultiCameraSystem<R>
where
    R: RealField + Copy + serde::Serialize + DeserializeOwned + Default,
{
    pub fn from_flydra_xml<Rd: Read>(reader: Rd) -> Result<Self> {
        let recon: flydra_xml_support::FlydraReconstructor<R> =
            serde_xml_rs::from_reader(reader).map_err(|_e| MvgError::FailedFlydraXmlConversion)?;
        FlydraMultiCameraSystem::from_flydra_reconstructor(&recon)
    }

    pub fn to_flydra_xml<W: Write>(&self, mut writer: W) -> Result<()> {
        let recon = self.to_flydra_reconstructor()?;
        let buf = flydra_xml_support::serialize_recon(&recon).map_err(|_e| MvgError::Io {
            source: std::io::ErrorKind::Other.into(),
            #[cfg(feature = "backtrace")]
            backtrace: std::backtrace::Backtrace::capture(),
        })?;
        writer.write_all(buf.as_bytes())?;
        Ok(())
    }
}

// FlydraCamera ----------------------------------------------

/// A helper trait to implement conversions to and from `mvg::Camera`
pub trait FlydraCamera<R: RealField + Copy + serde::Serialize> {
    fn to_flydra(&self, name: &str) -> Result<SingleCameraCalibration<R>>;
    fn from_flydra(cam: &SingleCameraCalibration<R>) -> Result<(String, Camera<R>)>;
}

impl<R: RealField + Copy + serde::Serialize> FlydraCamera<R> for Camera<R> {
    fn to_flydra(&self, name: &str) -> Result<SingleCameraCalibration<R>> {
        let cam_id = name.to_string();
        if self.intrinsics().distortion.radial3() != na::convert(0.0) {
            return Err(MvgError::FailedFlydraXmlConversion);
        }
        let k = self.intrinsics().k;
        let distortion = &self.intrinsics().distortion;
        let alpha_c = k[(0, 1)] / k[(0, 0)];

        let non_linear_parameters = FlydraDistortionModel {
            fc1: k[(0, 0)],
            fc2: k[(1, 1)],
            cc1: k[(0, 2)],
            cc2: k[(1, 2)],
            alpha_c,
            k1: distortion.radial1(),
            k2: distortion.radial2(),
            p1: distortion.tangential1(),
            p2: distortion.tangential2(),
            fc1p: None,
            fc2p: None,
            cc1p: None,
            cc2p: None,
        };
        let calibration_matrix = *self.linear_part_as_pmat();
        Ok(SingleCameraCalibration {
            cam_id,
            calibration_matrix,
            resolution: (self.width(), self.height()),
            scale_factor: None,
            non_linear_parameters,
        })
    }

    #[allow(non_snake_case)]
    fn from_flydra(cam: &SingleCameraCalibration<R>) -> Result<(String, Camera<R>)> {
        let one: R = One::one();
        let zero: R = Zero::zero();

        let name = cam.cam_id.clone();
        let m = cam.calibration_matrix.remove_column(3);
        let (rquat, k) = rq_decomposition(m)?;

        let k22: R = k[(2, 2)];
        let k = k * (one / k22); // normalize
        let p = OMatrix::<R, U3, U4>::new(
            k[(0, 0)],
            k[(0, 1)],
            k[(0, 2)],
            zero,
            k[(1, 0)],
            k[(1, 1)],
            k[(1, 2)],
            zero,
            k[(2, 0)],
            k[(2, 1)],
            k[(2, 2)],
            zero,
        );

        // (Ab)use PyMVG's rectification to do coordinate transform
        // for MCSC's undistortion.

        // The intrinsic parameters used for 3D -> 2D.
        let ex = p[(0, 0)];
        let bx = p[(0, 2)];
        let Sx = p[(0, 3)];
        let ey = p[(1, 1)];
        let by = p[(1, 2)];
        let Sy = p[(1, 3)];

        // Parameters used to define undistortion coordinates.
        let fx = cam.non_linear_parameters.fc1;
        let fy = cam.non_linear_parameters.fc2;
        let cx = cam.non_linear_parameters.cc1;
        let cy = cam.non_linear_parameters.cc2;

        // TODO: turn all these `unimplemented!()` calls into
        // proper error returns.
        if cam.non_linear_parameters.alpha_c != zero {
            unimplemented!();
        }
        if let Some(fc1p) = cam.non_linear_parameters.fc1p {
            if fc1p != cam.non_linear_parameters.fc1 {
                unimplemented!();
            }
        }
        if let Some(fc2p) = cam.non_linear_parameters.fc2p {
            if fc2p != cam.non_linear_parameters.fc2 {
                unimplemented!();
            }
        }
        if let Some(cc1p) = cam.non_linear_parameters.cc1p {
            if cc1p != cam.non_linear_parameters.cc1 {
                unimplemented!();
            }
        }
        if let Some(cc2p) = cam.non_linear_parameters.cc2p {
            if cc2p != cam.non_linear_parameters.cc2 {
                unimplemented!();
            }
        }
        if let Some(scale_factor) = cam.scale_factor {
            if scale_factor != one {
                unimplemented!();
            }
        }

        #[rustfmt::skip]
        let rect_t = {
            Matrix3::new(
            ex/fx,     zero, (bx+Sx-cx)/fx,
             zero,    ey/fy, (by+Sy-cy)/fy,
             zero,     zero,           one)
        };
        let rect = rect_t.transpose();
        let i = &cam.non_linear_parameters;
        let k3 = zero;
        let distortion = Vector5::new(i.k1, i.k2, i.p1, i.p2, k3);
        #[rustfmt::skip]
        let k = {
            Matrix3::new(
            fx, i.alpha_c*fx, cx,
            zero, fy, cy,
            zero, zero, one)
        };
        let distortion = Distortion::from_opencv_vec(distortion);
        let intrinsics = RosOpenCvIntrinsics::from_components(p, k, distortion, rect)?;
        let camcenter = pmat2cam_center(&cam.calibration_matrix);

        let extrinsics = ExtrinsicParameters::from_rotation_and_camcenter(rquat, camcenter);
        let cam2 = Self::new(cam.resolution.0, cam.resolution.1, extrinsics, intrinsics)?;

        Ok((name, cam2))
    }
}

/// helper function (duplicated from mvg)
#[allow(clippy::many_single_char_names)]
fn pmat2cam_center<R: RealField + Copy>(p: &OMatrix<R, U3, U4>) -> Point3<R> {
    let x = (*p).remove_column(0).determinant();
    let y = -(*p).remove_column(1).determinant();
    let z = (*p).remove_column(2).determinant();
    let w = -(*p).remove_column(3).determinant();
    Point3::from(Vector3::new(x / w, y / w, z / w))
}
