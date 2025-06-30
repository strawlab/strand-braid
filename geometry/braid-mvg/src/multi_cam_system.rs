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

/// A calibrated multi-camera system for 3D computer vision applications.
///
/// This structure manages a collection of cameras with known intrinsic and extrinsic
/// parameters, providing high-level operations for 3D reconstruction, triangulation,
/// and geometric analysis. It's the primary interface for multi-view geometry
/// operations in the Braid system.
///
/// # Core Capabilities
///
/// - **3D Triangulation**: Reconstruct 3D points from 2D observations across cameras
/// - **Reprojection Analysis**: Compute and analyze reprojection errors for quality assessment
/// - **Geometric Validation**: Verify camera calibration quality and detect issues
/// - **Format Conversion**: Import/export from various camera system formats (PyMVG, etc.)
///
/// # Mathematical Foundation
///
/// The system operates on the principle that multiple cameras observing the same
/// 3D point provide redundant information that can be used to:
///
/// 1. **Triangulate** the 3D position via geometric intersection of viewing rays
/// 2. **Validate** the reconstruction by reprojecting back to all cameras
/// 3. **Optimize** camera parameters through bundle adjustment
///
/// # Camera Naming
///
/// Cameras are identified by string names within the system. This allows for
/// flexible camera management and easy association of observations with specific cameras.
///
/// # Example
///
/// ```rust
/// use braid_mvg::{Camera, MultiCameraSystem, PointWorldFrame, extrinsics, make_default_intrinsics, UndistortedPixel};
/// use std::collections::BTreeMap;
/// use nalgebra::{Point3, Point2};
///
/// // Create cameras
/// let camera1 = Camera::new(640, 480,
///     extrinsics::make_default_extrinsics::<f64>(),
///     make_default_intrinsics::<f64>())?;
/// let camera2 = Camera::new(640, 480,
///     extrinsics::make_default_extrinsics::<f64>(),
///     make_default_intrinsics::<f64>())?;
///
/// // Build multi-camera system
/// let mut cameras = BTreeMap::new();
/// cameras.insert("cam1".to_string(), camera1);
/// cameras.insert("cam2".to_string(), camera2);
/// let system = MultiCameraSystem::new(cameras);
///
/// // Create observations for triangulation as slice of tuples
/// let observations = vec![
///     ("cam1".to_string(), UndistortedPixel { coords: Point2::new(320.0, 240.0) }),
///     ("cam2".to_string(), UndistortedPixel { coords: Point2::new(320.0, 240.0) }),
/// ];
///
/// // Use for 3D triangulation
/// if let Ok(point_3d) = system.find3d(&observations) {
///     println!("Reconstructed 3D point: {:?}", point_3d.coords);
/// }
/// # Ok::<(), braid_mvg::MvgError>(())
/// ```
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
    /// Export the camera system to PyMVG format via a writer.
    ///
    /// This method serializes the multi-camera system to PyMVG JSON format,
    /// writing the result to the provided writer.
    ///
    /// # Arguments
    ///
    /// * `writer` - Writer to output the PyMVG JSON data
    ///
    /// # Returns
    ///
    /// `Ok(())` on success, or [`MvgError`] if serialization fails.
    pub fn to_pymvg_writer<W: std::io::Write>(&self, writer: &mut W) -> Result<()> {
        let sys = self.to_pymvg()?;
        serde_json::to_writer(writer, &sys)?;
        Ok(())
    }
    /// Create a multi-camera system from PyMVG JSON format.
    ///
    /// This constructor deserializes a multi-camera system from PyMVG JSON format,
    /// reading from the provided reader.
    ///
    /// # Arguments
    ///
    /// * `reader` - Reader containing PyMVG JSON data
    ///
    /// # Returns
    ///
    /// A new [`MultiCameraSystem`] instance, or [`MvgError`] if parsing fails.
    pub fn from_pymvg_json<Rd: Read>(reader: Rd) -> Result<Self> {
        let pymvg_system: PymvgMultiCameraSystemV1<R> = serde_json::from_reader(reader)?;
        MultiCameraSystem::from_pymvg(&pymvg_system)
    }
}

impl<R: RealField + Default + Serialize + Copy> MultiCameraSystem<R> {
    /// Create a new multi-camera system from a collection of cameras.
    ///
    /// # Arguments
    ///
    /// * `cams_by_name` - Map of camera names to [`Camera`] instances
    ///
    /// # Returns
    ///
    /// A new [`MultiCameraSystem`] instance
    pub fn new(cams_by_name: BTreeMap<String, Camera<R>>) -> Self {
        Self::new_inner(cams_by_name, None)
    }

    /// Get an optional comment describing this camera system.
    ///
    /// # Returns
    ///
    /// Optional reference to the comment string
    #[inline]
    pub fn comment(&self) -> Option<&String> {
        self.comment.as_ref()
    }

    /// Get the collection of cameras in this system.
    ///
    /// # Returns
    ///
    /// Reference to the map of camera names to [`Camera`] instances
    #[inline]
    pub fn cams_by_name(&self) -> &BTreeMap<String, Camera<R>> {
        &self.cams_by_name
    }

    /// Create a new multi-camera system with an optional comment.
    ///
    /// # Arguments
    ///
    /// * `cams_by_name` - Map of camera names to [`Camera`] instances
    /// * `comment` - Descriptive comment for the camera system
    ///
    /// # Returns
    ///
    /// A new [`MultiCameraSystem`] instance
    pub fn new_with_comment(cams_by_name: BTreeMap<String, Camera<R>>, comment: String) -> Self {
        Self::new_inner(cams_by_name, Some(comment))
    }

    /// Internal constructor for creating a multi-camera system.
    ///
    /// # Arguments
    ///
    /// * `cams_by_name` - Map of camera names to [`Camera`] instances
    /// * `comment` - Optional descriptive comment
    ///
    /// # Returns
    ///
    /// A new [`MultiCameraSystem`] instance
    pub fn new_inner(cams_by_name: BTreeMap<String, Camera<R>>, comment: Option<String>) -> Self {
        Self {
            cams_by_name,
            comment,
        }
    }

    /// Get a camera by name.
    ///
    /// # Arguments
    ///
    /// * `name` - Name of the camera to retrieve
    ///
    /// # Returns
    ///
    /// Optional reference to the [`Camera`], or `None` if not found
    #[inline]
    pub fn cam_by_name(&self, name: &str) -> Option<&Camera<R>> {
        self.cams_by_name.get(name)
    }

    /// Create a multi-camera system from a PyMVG data structure.
    ///
    /// # Arguments
    ///
    /// * `pymvg_system` - PyMVG multi-camera system data structure
    ///
    /// # Returns
    ///
    /// A new [`MultiCameraSystem`] instance, or [`MvgError`] if conversion fails
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

    /// Convert this multi-camera system to PyMVG format.
    ///
    /// This method converts the camera system to PyMVG data structure format
    /// for interoperability with PyMVG library and JSON serialization.
    ///
    /// # Returns
    ///
    /// A [`PymvgMultiCameraSystemV1`] structure, or [`MvgError`] if conversion fails
    ///
    /// # Example
    ///
    /// ```rust
    /// use braid_mvg::{MultiCameraSystem, Camera, extrinsics, make_default_intrinsics};
    /// use std::collections::BTreeMap;
    ///
    /// let mut cameras = BTreeMap::new();
    /// cameras.insert("cam1".to_string(), Camera::new(640, 480,
    ///     extrinsics::make_default_extrinsics::<f64>(),
    ///     make_default_intrinsics::<f64>())?);
    /// let system = MultiCameraSystem::new(cameras);
    /// let pymvg_system = system.to_pymvg()?;
    /// # Ok::<(), braid_mvg::MvgError>(())
    /// ```
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

    /// Apply a similarity transformation to all cameras in the system.
    ///
    /// This method applies the same similarity transformation (scale, rotation, translation)
    /// to all cameras in the multi-camera system. This is commonly used for:
    /// - Coordinate system alignment between different camera systems
    /// - Scale recovery in structure-from-motion pipelines
    /// - Aligning reconstructed coordinates with ground truth
    ///
    /// # Mathematical Details
    ///
    /// The transformation applies: `X' = s*R*X + t` to all camera positions and orientations.
    ///
    /// # Arguments
    ///
    /// * `s` - Uniform scale factor (must be positive)
    /// * `rot` - 3×3 rotation matrix (must be orthogonal with determinant +1)
    /// * `t` - 3×1 translation vector
    ///
    /// # Returns
    ///
    /// A new aligned [`MultiCameraSystem`], or [`MvgError`] if transformation fails
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The rotation matrix is invalid
    /// - The scale factor is non-positive
    /// - Any camera transformation fails
    ///
    /// # Example
    ///
    /// ```rust
    /// use braid_mvg::{MultiCameraSystem, Camera, extrinsics, make_default_intrinsics};
    /// use nalgebra::{Matrix3, Vector3};
    /// use std::collections::BTreeMap;
    ///
    /// let mut cameras = BTreeMap::new();
    /// cameras.insert("cam1".to_string(), Camera::new(640, 480,
    ///     extrinsics::make_default_extrinsics::<f64>(),
    ///     make_default_intrinsics::<f64>())?);
    /// let system = MultiCameraSystem::new(cameras);
    ///
    /// let scale = 2.0;
    /// let rotation = Matrix3::identity();
    /// let translation = Vector3::zeros();
    /// let aligned_system = system.align(scale, rotation, translation)?;
    /// # Ok::<(), braid_mvg::MvgError>(())
    /// ```
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
