#![allow(non_snake_case)]

use std::collections::BTreeMap;
use std::io::Read;

use na::{Matrix3, Vector3};
use nalgebra as na;

use na::RealField;
#[allow(unused_imports)]
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use cam_geom::{coordinate_system::WorldFrame, Ray};

use crate::pymvg_support::PymvgMultiCameraSystemV1;
use crate::{
    Camera, MvgError, PointWorldFrame, PointWorldFrameWithSumReprojError, Result, UndistortedPixel,
};

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde-serialize", derive(Serialize, Deserialize))]
pub struct MultiCameraSystem<R: RealField + Serialize + Copy> {
    cams_by_name: BTreeMap<String, Camera<R>>,
    comment: Option<String>,
}

impl<R> MultiCameraSystem<R>
where
    R: RealField + Serialize + DeserializeOwned + Default + Copy,
{
    pub fn to_pymvg_writer<W: std::io::Write>(&self, writer: &mut W) -> Result<()> {
        let sys = self.to_pymvg()?;
        serde_json::to_writer(writer, &sys)?;
        Ok(())
    }
    pub fn from_pymvg_json<Rd: Read>(reader: Rd) -> Result<Self> {
        let pymvg_system: PymvgMultiCameraSystemV1<R> = serde_json::from_reader(reader)?;
        MultiCameraSystem::from_pymvg(&pymvg_system)
    }
}

impl<R: RealField + Default + Serialize + Copy> MultiCameraSystem<R> {
    pub fn new(cams_by_name: BTreeMap<String, Camera<R>>) -> Self {
        Self::new_inner(cams_by_name, None)
    }

    #[inline]
    pub fn comment(&self) -> Option<&String> {
        self.comment.as_ref()
    }

    #[inline]
    pub fn cams_by_name(&self) -> &BTreeMap<String, Camera<R>> {
        &self.cams_by_name
    }

    pub fn new_with_comment(cams_by_name: BTreeMap<String, Camera<R>>, comment: String) -> Self {
        Self::new_inner(cams_by_name, Some(comment))
    }

    pub fn new_inner(cams_by_name: BTreeMap<String, Camera<R>>, comment: Option<String>) -> Self {
        Self {
            cams_by_name,
            comment,
        }
    }

    #[inline]
    pub fn cams(&self) -> &BTreeMap<String, Camera<R>> {
        &self.cams_by_name
    }

    #[inline]
    pub fn cam_by_name(&self, name: &str) -> Option<&Camera<R>> {
        self.cams_by_name.get(name)
    }

    pub fn from_pymvg(pymvg_system: &PymvgMultiCameraSystemV1<R>) -> Result<Self> {
        let mut cams = BTreeMap::new();
        if pymvg_system.__pymvg_file_version__ != "1.0" {
            return Err(MvgError::UnsupportedVersion);
        }
        for pymvg_cam in pymvg_system.camera_system.iter() {
            let (name, cam) = Camera::from_pymvg(pymvg_cam)?;
            cams.insert(name, cam);
        }
        Ok(Self::new(cams))
    }

    pub fn to_pymvg(&self) -> Result<PymvgMultiCameraSystemV1<R>> {
        Ok(PymvgMultiCameraSystemV1 {
            __pymvg_file_version__: "1.0".to_string(),
            camera_system: self
                .cams_by_name
                .iter()
                .map(|(name, cam)| cam.to_pymvg(name))
                .collect(),
        })
    }

    /// Find reprojection error of 3D coordinate into pixel coordinates.
    ///
    /// Note that this returns the reprojection distance of the *undistorted*
    /// pixels.
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
                        .cams_by_name
                        .get(cam_name)
                        .ok_or(MvgError::UnknownCamera)?
                        .project_3d_to_pixel(this_3d_pt)
                        .coords,
                    &orig.coords,
                ))
            })
            .collect::<Result<Vec<R>>>()?;
        Ok(this_dists)
    }

    /// Find 3D coordinate and cumulative reprojection distance using pixel coordinates from cameras
    pub fn find3d_and_cum_reproj_dist(
        &self,
        points: &[(String, UndistortedPixel<R>)],
    ) -> Result<PointWorldFrameWithSumReprojError<R>> {
        let point = self.find3d(points)?;
        let reproj_dists = self.get_reprojection_undistorted_dists(points, &point)?;
        Ok(PointWorldFrameWithSumReprojError::new(point, reproj_dists))
    }

    /// Find 3D coordinate using pixel coordinates from cameras
    pub fn find3d(&self, points: &[(String, UndistortedPixel<R>)]) -> Result<PointWorldFrame<R>> {
        if points.len() < 2 {
            return Err(MvgError::NotEnoughPoints);
        }

        self.find3d_air(points)
    }

    fn find3d_air(&self, points: &[(String, UndistortedPixel<R>)]) -> Result<PointWorldFrame<R>> {
        let mut rays: Vec<Ray<WorldFrame, R>> = Vec::with_capacity(points.len());
        for (name, xy) in points.iter() {
            // Get camera.
            let cam = self.cams_by_name.get(name).ok_or(MvgError::UnknownCamera)?;
            // Get ray from point `xy` in camera coords.
            let ray_cam = cam.intrinsics().undistorted_pixel_to_camera(&xy.into());
            // Convert to world coords.
            let ray = cam
                .extrinsics()
                .ray_camera_to_world(&ray_cam)
                .to_single_ray();
            rays.push(ray);
        }

        let coords = cam_geom::best_intersection_of_rays(&rays)?;
        Ok(coords.into())
    }

    pub fn align(&self, s: R, rot: Matrix3<R>, t: Vector3<R>) -> Result<Self> {
        let comment = self.comment.clone();

        let mut aligned = BTreeMap::new();

        for (name, orig_cam) in self.cams_by_name.iter() {
            let cam = orig_cam.align(s, rot, t)?;
            aligned.insert(name.clone(), cam);
        }

        Ok(Self {
            cams_by_name: aligned,
            comment,
        })
    }
}
