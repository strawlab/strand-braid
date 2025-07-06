use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::PathBuf;

use serde::de::DeserializeOwned;

use num_traits::{One, Zero};

use nalgebra as na;
use nalgebra::{
    allocator::Allocator, geometry::Point3, DMatrix, DefaultAllocator, Dyn, Matrix3, OMatrix,
    RealField, Vector3, Vector5, U1, U2, U3, U4,
};

use cam_geom::ExtrinsicParameters;
use opencv_ros_camera::{Distortion, RosOpenCvIntrinsics};

use braid_mvg::{
    rq_decomposition, vec_sum, Camera, DistortedPixel, MultiCameraSystem, MvgError,
    PointWorldFrame, PointWorldFrameMaybeWithSumReprojError, PointWorldFrameWithSumReprojError,
    UndistortedPixel, WorldCoordAndUndistorted2D,
};

mod fermats_least_time;

pub mod flydra_xml_support;

use crate::flydra_xml_support::{FlydraDistortionModel, SingleCameraCalibration};

const AIR_REFRACTION: f64 = 1.0003;

#[derive(thiserror::Error, Debug)]
pub enum FlydraMvgError {
    #[error("xml error: {0}")]
    SerdeXmlError(#[from] serde_xml_rs::Error),
    #[error("cannot convert to or from flydra xml: {msg}")]
    FailedFlydraXmlConversion { msg: &'static str },
    #[error("MVG error: {0}")]
    MvgError(#[from] braid_mvg::MvgError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("not implemented operation in braid_mvg")]
    NotImplemented,
    #[error("no valid root found")]
    NoValidRootFound,
    #[error("No non-linear parameter file {0} found")]
    NoNonlinearParameters(PathBuf),
}

pub type Result<T> = std::result::Result<T, FlydraMvgError>;

// MultiCameraIter -------------------------------------------------------

/// implements an `Iterator` which returns cameras as `MultiCamera`s.
pub struct MultiCameraIter<'a, R: RealField + Copy + Default + serde::Serialize> {
    name_iter: CamNameIter<'a, R>,
    flydra_system: &'a FlydraMultiCameraSystem<R>,
}

impl<R: RealField + Copy + Default + serde::Serialize> Iterator for MultiCameraIter<'_, R> {
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

// RealField and f64 inter conversion ----------------------------------------------------

trait Point3ToR<R: RealField> {
    fn to_r(self) -> Point3<R>;
}

impl<R: RealField> Point3ToR<R> for Point3<f64> {
    fn to_r(self) -> Point3<R> {
        Point3::new(
            na::convert(self[0]),
            na::convert(self[1]),
            na::convert(self[2]),
        )
    }
}

trait Vector3ToR<R: RealField> {
    fn to_r(self) -> Vector3<R>;
}

impl<R: RealField> Vector3ToR<R> for Vector3<f64> {
    fn to_r(self) -> Vector3<R> {
        Vector3::new(
            na::convert(self[0]),
            na::convert(self[1]),
            na::convert(self[2]),
        )
    }
}

trait Point3ToF64 {
    fn to_f64(self) -> Point3<f64>;
}

impl<R: RealField> Point3ToF64 for &Point3<R> {
    fn to_f64(self) -> Point3<f64> {
        let x: f64 = self[0].to_subset().unwrap();
        let y: f64 = self[1].to_subset().unwrap();
        let z: f64 = self[2].to_subset().unwrap();
        Point3::new(x, y, z)
    }
}

trait Vector3ToF64<T> {
    fn to_f64(self) -> nalgebra::Vector3<f64>;
}

impl<T> Vector3ToF64<T> for &nalgebra::Vector3<T>
where
    T: RealField,
{
    fn to_f64(self) -> nalgebra::Vector3<f64> {
        nalgebra::Vector3::new(
            self[0].to_subset().unwrap(),
            self[1].to_subset().unwrap(),
            self[2].to_subset().unwrap(),
        )
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
    fn project_pixel_to_ray(&self, pt: &UndistortedPixel<R>) -> parry3d_f64::query::Ray;
    fn project_distorted_pixel_to_ray(&self, pt2d: &DistortedPixel<R>) -> parry3d_f64::query::Ray;
    fn project_ray_to_distorted_pixel(&self, ray: &parry3d_f64::query::Ray) -> DistortedPixel<R>;
    fn project_ray_to_pixel(&self, ray: &parry3d_f64::query::Ray) -> UndistortedPixel<R>;
}

impl<R: RealField + Copy + Default + serde::Serialize> RayCamera<R> for Camera<R> {
    fn project_pixel_to_ray(&self, pt: &UndistortedPixel<R>) -> parry3d_f64::query::Ray {
        let dist = na::convert(1.0);
        let p2 = self.project_pixel_to_3d_with_dist(pt, dist);
        let ray_origin = *self.extrinsics().camcenter();
        let ray_dir = p2.coords - ray_origin;
        let ray_origin = ray_origin.to_f64();
        let ray_dir = ray_dir.to_f64();
        parry3d_f64::query::Ray::new(ray_origin, ray_dir)
    }

    fn project_distorted_pixel_to_ray(&self, pt2d: &DistortedPixel<R>) -> parry3d_f64::query::Ray {
        let undistorted = self.intrinsics().undistort(&pt2d.into());
        self.project_pixel_to_ray(&undistorted.into())
    }

    fn project_ray_to_distorted_pixel(&self, ray: &parry3d_f64::query::Ray) -> DistortedPixel<R> {
        let camcenter = self.extrinsics().camcenter().to_f64();
        debug_assert!(ray.origin == camcenter);
        let pt3d = PointWorldFrame::<R> {
            coords: (ray.origin + ray.dir).to_r(),
        };
        self.project_3d_to_distorted_pixel(&pt3d)
    }

    fn project_ray_to_pixel(&self, ray: &parry3d_f64::query::Ray) -> UndistortedPixel<R> {
        debug_assert!(ray.origin == self.extrinsics().camcenter().to_f64());
        let pt3d = PointWorldFrame::<R> {
            coords: (ray.origin + ray.dir).to_r(),
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
    pub fn project_pixel_to_ray(&self, pt: &UndistortedPixel<R>) -> parry3d_f64::query::Ray {
        self.cam.project_pixel_to_ray(pt)
    }

    #[inline]
    pub fn project_distorted_pixel_to_ray(
        &self,
        pt: &DistortedPixel<R>,
    ) -> parry3d_f64::query::Ray {
        self.cam.project_distorted_pixel_to_ray(pt)
    }

    #[inline]
    pub fn project_ray_to_pixel(&self, ray: &parry3d_f64::query::Ray) -> UndistortedPixel<R> {
        self.cam.project_ray_to_pixel(ray)
    }

    #[inline]
    pub fn project_ray_to_distorted_pixel(
        &self,
        ray: &parry3d_f64::query::Ray,
    ) -> DistortedPixel<R> {
        self.cam.project_ray_to_distorted_pixel(ray)
    }

    /// projects a 3D point to a ray
    ///
    /// If the point is under water, the ray is in the direction the camera
    /// sees it (not the straight-line direction).
    pub fn project_3d_to_ray(&self, pt3d: &PointWorldFrame<R>) -> parry3d_f64::query::Ray {
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
                    tracing::error!(
                        "find_fastest_path_fermat {} with parameters: {:?}",
                        e,
                        root_params,
                    );
                    panic!("find_fastest_path_fermat {e} with parameters: {root_params:?}");
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
        parry3d_f64::query::Ray::new(camcenter.to_f64(), dir.to_f64())
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
        let coords: Point3<R> = (ray.origin + ray.dir).to_r();
        let pt_air =
            cam_geom::Points::<cam_geom::WorldFrame, _, _, _>::new(coords.coords.transpose());

        use opencv_ros_camera::CameraExt;
        let pt_undistorted = self.cam.as_ref().world_to_undistorted_pixel(&pt_air);

        pt_undistorted.into()
    }

    pub fn project_3d_to_distorted_pixel(&self, pt3d: &PointWorldFrame<R>) -> DistortedPixel<R>
    where
        DefaultAllocator: Allocator<U1, U2>,
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

    pub fn undistort(&self, a: &braid_mvg::DistortedPixel<R>) -> braid_mvg::UndistortedPixel<R> {
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

    pub fn has_refractive_boundary(&self) -> bool {
        self.water.is_some()
    }

    pub fn water(&self) -> Option<R> {
        self.water
    }

    pub fn to_system(self) -> MultiCameraSystem<R> {
        self.system
    }

    pub fn system(&self) -> &MultiCameraSystem<R> {
        &self.system
    }

    pub fn new(cams_by_name: BTreeMap<String, Camera<R>>, water: Option<R>) -> Self {
        let system = MultiCameraSystem::new(cams_by_name);

        FlydraMultiCameraSystem { system, water }
    }

    pub fn len(&self) -> usize {
        self.system.cams_by_name().len()
    }

    pub fn is_empty(&self) -> bool {
        self.system.cams_by_name().is_empty()
    }

    pub fn cam_by_name(&self, name: &str) -> Option<MultiCamera<R>> {
        self.system.cam_by_name(name).map(|cam| MultiCamera {
            water: self.water,
            name: name.to_string(),
            cam: cam.clone(),
        })
    }

    pub fn cam_names(&self) -> CamNameIter<'_, R> {
        CamNameIter(self.system.cams_by_name().keys())
    }

    pub fn cameras<'a>(&'a self) -> MultiCameraIter<'a, R> {
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
            return Err(MvgError::NotEnoughPoints.into());
        }

        use crate::PointWorldFrameMaybeWithSumReprojError::*;

        match self.water {
            Some(n2) => {
                // TODO: would it be possible to have a 3d reconstruction with
                // lower reprojection error when it was z<0 but with the air
                // based calculation? This would seem problematic...
                let opt_water_3d_pt = match self.find3d_water(points, n2) {
                    Ok(water_3d_pt) => Some(water_3d_pt),
                    Err(FlydraMvgError::MvgError(MvgError::CamGeomError { .. })) => None,
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
            .filter_map(|(name, pt)| {
                self.cam_by_name(name)
                    .map(|cam| (name.clone(), cam.undistort(pt)))
            })
            .collect();
        if upoints.len() != points.len() {
            return Err(MvgError::UnknownCamera.into());
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
        let z0 = parry3d_f64::shape::HalfSpace::new(Vector3::z_axis());

        let mut rays: Vec<Ray<WorldFrame, _>> = Vec::with_capacity(points.len());

        for (name, xy) in points.iter() {
            let cam = self.cam_by_name(name).ok_or(MvgError::UnknownCamera)?;
            let air_ray = cam.project_pixel_to_ray(xy);
            let solid = false; // will intersect either side of plane

            let opt_surface_pt_toi: Option<f64> = parry3d_f64::query::RayCast::cast_local_ray(
                &z0,
                &air_ray,
                f64::max_value().unwrap(),
                solid,
            );

            let air_ray_origin = air_ray.origin.to_r();
            let air_ray_dir = air_ray.dir.to_r();

            if let Some(toi) = opt_surface_pt_toi {
                let toi: R = na::convert(toi);
                let surface_pt: Point3<R> = air_ray_origin + air_ray_dir * toi;

                // closest point to camera on water surface, assumes water at z==0
                let camcenter = &air_ray_origin;
                let camcenter_z0: Point3<R> =
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
                let deep_pt_cam: Vector3<R> = Vector3::new(
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
        let pt = cam_geom::best_intersection_of_rays(&rays).map_err(braid_mvg::MvgError::from)?;
        Ok(pt.into())
    }

    /// Find 3D coordinate using pixel coordinates from cameras
    fn find3d_air(&self, points: &[(String, UndistortedPixel<R>)]) -> Result<PointWorldFrame<R>> {
        Ok(self.system.find3d(points)?)
    }

    /// Find reprojection error of 3D coordinate into pixel coordinates
    pub fn get_reprojection_undistorted_dists(
        &self,
        points: &[(String, UndistortedPixel<R>)],
        this_3d_pt: &PointWorldFrame<R>,
    ) -> Result<Vec<R>> {
        let this_dists = points
            .iter()
            .map(|(cam_name, orig)| {
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

fn loadtxt_3x4<R>(p: impl AsRef<std::path::Path>) -> Result<OMatrix<R, U3, U4>>
where
    R: RealField + Copy + serde::Serialize + DeserializeOwned + Default,
{
    let mat = loadtxt_dyn::<R>(p)?;
    if mat.nrows() != 3 || mat.ncols() != 4 {
        return Err(MvgError::ParseError.into());
    }
    Ok(OMatrix::<R, U3, U4>::from_column_slice(mat.as_slice()))
}

fn loadtxt_dyn<R>(p: impl AsRef<std::path::Path>) -> Result<OMatrix<R, Dyn, Dyn>>
where
    R: RealField + Copy + serde::Serialize + DeserializeOwned + Default,
{
    let buf = std::fs::read_to_string(p.as_ref()).map_err(braid_mvg::MvgError::from)?;
    let lines: Vec<&str> = buf.trim().split("\n").collect();
    let lines: Vec<&str> = lines
        .into_iter()
        .filter(|line| !line.trim().starts_with('#'))
        .collect();
    let mut result = Vec::new();
    let mut n_cols = None;
    for line in lines.iter() {
        let mut this_line: Vec<R> = Vec::new();
        for val_str in line.split_ascii_whitespace() {
            let val: f64 = val_str.parse().map_err(|_| MvgError::ParseError)?;
            this_line.push(na::convert(val));
        }
        if n_cols.is_none() {
            n_cols = Some(this_line.len());
        }
        if n_cols != Some(this_line.len()) {
            return Err(MvgError::ParseError.into());
        }
        result.push(this_line);
    }
    let n_rows = result.len();
    if n_rows < 1 {
        return Err(MvgError::ParseError.into());
    }
    let n_cols = n_cols.unwrap();

    let mut rmat = OMatrix::<R, Dyn, Dyn>::zeros(n_rows, n_cols);
    for (i, this_row) in result.into_iter().enumerate() {
        for (j, this_el) in this_row.into_iter().enumerate() {
            rmat[(i, j)] = this_el;
        }
    }
    Ok(rmat)
}

fn loadrad<R>(p: impl AsRef<std::path::Path>) -> Result<FlydraDistortionModel<R>>
where
    R: RealField + Copy + serde::Serialize + DeserializeOwned + Default,
{
    let buf = std::fs::read_to_string(p.as_ref())?;
    let lines: Vec<&str> = buf.trim().split("\n").collect();
    let mut vars = BTreeMap::new();
    for line in lines.into_iter() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<_> = line.split("=").collect();
        if parts.len() != 2 {
            return Err(MvgError::ParseError.into());
        }
        let (varname, valstr) = (parts[0], parts[1]);
        let val: f64 = valstr.trim().parse().map_err(|_| MvgError::ParseError)?;
        let val: R = na::convert(val);
        vars.insert(varname.trim().to_string(), val);
    }

    Ok(FlydraDistortionModel {
        fc1: vars["K11"],
        fc2: vars["K22"],
        cc1: vars["K13"],
        cc2: vars["K23"],
        alpha_c: na::convert(0.0),
        k1: vars["kc1"],
        k2: vars["kc2"],
        p1: vars["kc3"],
        p2: vars["kc4"],
        k3: na::convert(0.0),
        fc1p: None,
        fc2p: None,
        cc1p: None,
        cc2p: None,
    })
}

pub struct McscDirData<R>
where
    R: RealField + Copy + serde::Serialize + DeserializeOwned + Default,
{
    pub cameras: Vec<SingleCameraCalibration<R>>,
    pub points4cals: Vec<DMatrix<f64>>,
}

pub fn read_mcsc_dir<R, P: AsRef<std::path::Path>>(mcsc_dir: P) -> Result<McscDirData<R>>
where
    R: RealField + Copy + serde::Serialize + DeserializeOwned + Default,
{
    let mcsc_dir = std::path::PathBuf::from(mcsc_dir.as_ref());
    let cam_order_fname = mcsc_dir.join("camera_order.txt");
    let cam_order = std::fs::read_to_string(cam_order_fname)?;
    let cam_ids: Vec<&str> = cam_order.trim().split("\n").collect();

    let res_dat = mcsc_dir.join("Res.dat");
    let res_dat_buf = std::fs::read_to_string(res_dat)?;
    let res_lines: Vec<&str> = res_dat_buf.trim().split("\n").collect();
    assert_eq!(cam_ids.len(), res_lines.len());
    let mut cameras = Vec::new();
    let mut points4cals = Vec::new();
    for (i, (cam_id, res_row)) in cam_ids.iter().zip(res_lines.iter()).enumerate() {
        let wh: Vec<&str> = res_row.split(" ").collect();
        assert_eq!(wh.len(), 2);
        let w = wh[0].parse().unwrap();
        let h = wh[1].parse().unwrap();

        let pmat_fname = mcsc_dir.join(format!("camera{}.Pmat.cal", (i + 1)));
        let pmat = loadtxt_3x4(&pmat_fname)?; // 3 rows x 4 columns

        let rad_fname = mcsc_dir.join(format!("basename{}.rad", (i + 1)));
        if !rad_fname.exists() {
            return Err(FlydraMvgError::NoNonlinearParameters(rad_fname));
        }
        let non_linear_parameters = loadrad(&rad_fname)?;

        let cam = SingleCameraCalibration {
            cam_id: cam_id.to_string(),
            calibration_matrix: pmat,
            resolution: (w, h),
            scale_factor: None,
            non_linear_parameters,
        };
        cameras.push(cam);

        let points4cal_fname = mcsc_dir.join(format!("cam{}.points4cal.dat", (i + 1)));
        if points4cal_fname.exists() {
            let points4cal = loadtxt_dyn(&points4cal_fname)?;
            points4cals.push(points4cal);
        }
    }

    Ok(McscDirData {
        cameras,
        points4cals,
    })
}

impl<R> FlydraMultiCameraSystem<R>
where
    R: RealField + Copy + serde::Serialize + DeserializeOwned + Default,
{
    pub fn from_mcsc_dir<P>(mcsc_dir: P) -> Result<Self>
    where
        P: AsRef<std::path::Path>,
    {
        let McscDirData { cameras, .. } = read_mcsc_dir(mcsc_dir)?;
        let recon = flydra_xml_support::FlydraReconstructor {
            cameras,
            ..Default::default()
        };
        FlydraMultiCameraSystem::from_flydra_reconstructor(&recon)
    }

    pub fn from_flydra_xml<Rd: Read>(reader: Rd) -> Result<Self> {
        let recon: flydra_xml_support::FlydraReconstructor<R> = serde_xml_rs::from_reader(reader)?;
        FlydraMultiCameraSystem::from_flydra_reconstructor(&recon)
    }

    pub fn to_flydra_xml<W: Write>(&self, mut writer: W) -> Result<()> {
        let recon = self.to_flydra_reconstructor()?;
        let buf = flydra_xml_support::serialize_recon(&recon).map_err(|_e| MvgError::Io {
            source: std::io::ErrorKind::Other.into(),
        })?;
        writer.write_all(buf.as_bytes())?;
        Ok(())
    }

    /// Read a calibration from a path.
    pub fn from_path<P>(cal_fname: P) -> Result<Self>
    where
        P: AsRef<std::path::Path>,
    {
        let cal_fname = cal_fname.as_ref();

        if cal_fname.is_dir() {
            return Self::from_mcsc_dir(cal_fname);
        }

        let cal_file = std::fs::File::open(cal_fname)?;

        if cal_fname.extension() == Some(std::ffi::OsStr::new("json"))
            || cal_fname.extension() == Some(std::ffi::OsStr::new("pymvg"))
        {
            // Assume any .json or .pymvg file is a pymvg file.
            let system = braid_mvg::MultiCameraSystem::from_pymvg_json(cal_file)?;
            Ok(Self::from_system(system, None))
        } else {
            // Otherwise, assume it is a flydra xml file.
            Ok(Self::from_flydra_xml(cal_file)?)
        }
    }
}

// FlydraCamera ----------------------------------------------

/// A helper trait to implement conversions to and from `braid_mvg::Camera`
pub trait FlydraCamera<R: RealField + Copy + serde::Serialize> {
    fn to_flydra(&self, name: &str) -> Result<SingleCameraCalibration<R>>;
    fn from_flydra(cam: &SingleCameraCalibration<R>) -> Result<(String, Camera<R>)>;
}
impl<R: RealField + Copy + serde::Serialize> FlydraCamera<R> for Camera<R> {
    fn to_flydra(&self, name: &str) -> Result<SingleCameraCalibration<R>> {
        let cam_id = name.to_string();
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
            k3: distortion.radial3(),
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

    fn from_flydra(cam: &SingleCameraCalibration<R>) -> Result<(String, Camera<R>)> {
        // We allow a relatively large epsilon here because, due to a bug, we
        // have saved many calibrations with cam.non_linear_parameters.alpha_c
        // set to zero where the skew in k is not quite zero. In theory, this
        // epsilon should be really low.
        let epsilon = 0.03;
        from_flydra_with_limited_skew(cam, epsilon)
    }
}

#[allow(non_snake_case)]
pub fn from_flydra_with_limited_skew<R: RealField + Copy + serde::Serialize>(
    cam: &SingleCameraCalibration<R>,
    epsilon: f64,
) -> Result<(String, Camera<R>)> {
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

    let expected_alpha_c = k[(0, 1)] / k[(0, 0)];

    if (expected_alpha_c - cam.non_linear_parameters.alpha_c).abs() > na::convert(epsilon) {
        return Err(FlydraMvgError::FailedFlydraXmlConversion {
            msg: "skew not supported",
        });
    }

    if let Some(fc1p) = cam.non_linear_parameters.fc1p {
        if fc1p != cam.non_linear_parameters.fc1 {
            return Err(FlydraMvgError::NotImplemented);
        }
    }
    if let Some(fc2p) = cam.non_linear_parameters.fc2p {
        if fc2p != cam.non_linear_parameters.fc2 {
            return Err(FlydraMvgError::NotImplemented);
        }
    }
    if let Some(cc1p) = cam.non_linear_parameters.cc1p {
        if cc1p != cam.non_linear_parameters.cc1 {
            return Err(FlydraMvgError::NotImplemented);
        }
    }
    if let Some(cc2p) = cam.non_linear_parameters.cc2p {
        if cc2p != cam.non_linear_parameters.cc2 {
            return Err(FlydraMvgError::NotImplemented);
        }
    }
    if let Some(scale_factor) = cam.scale_factor {
        if scale_factor != one {
            return Err(FlydraMvgError::NotImplemented);
        }
    }

    // This craziness abuses the rectification matrix of the ROS/OpenCV
    // model to compensate for the issue that the intrinsic parameters used
    // in the MultiCamSelfCal (MCSC) distortion correction are independent
    // from the intrinsic parameters of the linear camera model. With this
    // abuse, we allow storing the MCSC calibration results in a compatible
    // way with ROS/OpenCV.
    //
    // Potential bug warning: it could be that the math used to work out
    // this matrix form had has a bug in which it was assumed that skew was
    // always zero. (This goes especially for entry [0,1].)
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
    let intrinsics = RosOpenCvIntrinsics::from_components(p, k, distortion, rect)
        .map_err(braid_mvg::MvgError::from)?;
    let camcenter = pmat2cam_center(&cam.calibration_matrix);

    let extrinsics = ExtrinsicParameters::from_rotation_and_camcenter(rquat, camcenter);
    let cam2 = Camera::new(cam.resolution.0, cam.resolution.1, extrinsics, intrinsics)?;

    Ok((name, cam2))
}

/// helper function (duplicated from braid_mvg)
#[allow(clippy::many_single_char_names)]
fn pmat2cam_center<R: RealField + Copy>(p: &OMatrix<R, U3, U4>) -> Point3<R> {
    let x = (*p).remove_column(0).determinant();
    let y = -(*p).remove_column(1).determinant();
    let z = (*p).remove_column(2).determinant();
    let w = -(*p).remove_column(3).determinant();
    Point3::from(Vector3::new(x / w, y / w, z / w))
}
