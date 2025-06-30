//! Camera geometry and multi-view geometry (MVG) types and algorithms for the
//! [Braid](https://strawlab.org/braid) tracking system.
//!
//! This crate provides camera modeling, geometric transformations, and
//! multi-camera system support for 3D computer vision applications. It's
//! specifically designed for use in the Braid multi-camera tracking system but
//! can be used for general computer vision tasks.
//!
//! ## Features
//!
//! - Camera modeling with intrinsic and extrinsic parameters based on
//!   [`cam-geom`](https://crates.io/crates/cam-geom)
//! - Lens distortion correction using OpenCV-compatible models based on
//!   [`opencv-ros-camera`](https://crates.io/crates/opencv-ros-camera)
//! - Multi-camera system management and calibration
//! - 3D point triangulation from multiple camera views
//! - Point alignment algorithms (Kabsch-Umeyama, robust Arun)
//! - Coordinate frame transformations between world, camera, and pixel spaces
//! - [rerun.io](https://rerun.io) integration for 3D visualization (optional)
//!
//! ## Core Types
//!
//! - [`Camera`]: Individual camera with intrinsics and extrinsics
//! - [`MultiCameraSystem`]: Collection of calibrated cameras
//! - [`DistortedPixel`], [`UndistortedPixel`]: Pixel coordinate types
//! - [`PointWorldFrame`], [`PointCameraFrame`]: 3D point types in different
//!       coordinate systems
//!
//! ## Coordinate Systems
//!
//! The crate uses three main coordinate systems:
//! - **World Frame**: Global 3D coordinate system
//! - **Camera Frame**: 3D coordinates relative to individual cameras
//! - **Pixel Coordinates**: 2D image coordinates (distorted and undistorted)
//!
//! ## Example
//!
//! This example demonstrates a complete round-trip workflow: projecting a 3D point
//! to 2D pixel coordinates in each camera, then reconstructing the 3D point from
//! these 2D observations and comparing it to the original.
//!
//! ```rust
//! use braid_mvg::{Camera, MultiCameraSystem, PointWorldFrame, extrinsics, make_default_intrinsics};
//! use std::collections::BTreeMap;
//! use nalgebra::Point3;
//!
//! // Create a multi-camera system with two cameras for triangulation
//!
//! // Camera 1: use default extrinsics (positioned at (1,2,3))
//! let extrinsics1 = extrinsics::make_default_extrinsics::<f64>();
//! let camera1 = Camera::new(640, 480, extrinsics1, make_default_intrinsics()).unwrap();
//!
//! // Camera 2: positioned with sufficient baseline for triangulation
//! let translation2 = nalgebra::Point3::new(3.0, 2.0, 3.0);
//! let rotation2 = nalgebra::UnitQuaternion::identity();
//! let extrinsics2 = extrinsics::from_rquat_translation(rotation2, translation2);
//! let camera2 = Camera::new(640, 480, extrinsics2, make_default_intrinsics()).unwrap();
//!
//! // Build the multi-camera system
//! let mut cameras = BTreeMap::new();
//! cameras.insert("cam1".to_string(), camera1);
//! cameras.insert("cam2".to_string(), camera2);
//! let system = MultiCameraSystem::new(cameras);
//!
//! // Define an original 3D point in world coordinates
//! // Place it in front of both cameras at a reasonable distance
//! let original_point = PointWorldFrame {
//!     coords: Point3::new(2.0, 2.0, 8.0)
//! };
//! println!("Original 3D point: {:?}", original_point.coords);
//!
//! // Step 1: Project the 3D point to 2D pixels in each camera
//! let mut observations = Vec::new();
//! println!("\nProjecting to 2D pixels:");
//! for (cam_name, camera) in system.cams_by_name() {
//!     let pixel = camera.project_3d_to_pixel(&original_point);
//!     println!("  {}: pixel ({:.2}, {:.2})", cam_name, pixel.coords.x, pixel.coords.y);
//!     observations.push((cam_name.clone(), pixel));
//! }
//!
//! // Step 2: Reconstruct the 3D point from the 2D observations
//! println!("\nReconstructing 3D point from 2D observations...");
//! let reconstructed_point = system.find3d(&observations)
//!     .expect("Triangulation should succeed with good observations");
//!
//! // Step 3: Compare original and reconstructed points
//! println!("Reconstructed 3D point: {:?}", reconstructed_point.coords);
//!
//! let error = (original_point.coords - reconstructed_point.coords).norm();
//! println!("3D reconstruction error: {error:.2e}");
//!
//! // With perfect cameras and no noise, reconstruction should be very accurate
//! assert!(error < 1e-6, "Reconstruction error too large: {error:.2e}");
//!
//! // Verify that reprojection works correctly
//! println!("\nVerifying reprojection accuracy:");
//! for (cam_name, camera) in system.cams_by_name() {
//!     let reprojected_pixel = camera.project_3d_to_pixel(&reconstructed_point);
//!     let original_pixel = camera.project_3d_to_pixel(&original_point);
//!     let pixel_error = (reprojected_pixel.coords - original_pixel.coords).norm();
//!     println!("  {cam_name}: reprojection error {pixel_error:.2e} pixels");
//!     assert!(pixel_error < 1e-6, "Reprojection error too large for {cam_name}");
//! }
//!
//! println!("✓ Round-trip 3D→2D→3D reconstruction successful!");
//! ```
#![deny(rust_2018_idioms)]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
use thiserror::Error;

use nalgebra as na;
use nalgebra::geometry::{Point2, Point3};
use nalgebra::{Dim, RealField, U1, U2, U3};

use cam_geom::ExtrinsicParameters;
use opencv_ros_camera::{Distortion, RosOpenCvIntrinsics};

#[derive(Error, Debug)]
pub enum MvgError {
    #[error("unknown distortion model")]
    UnknownDistortionModel,
    #[error("rectification matrix not supported")]
    RectificationMatrixNotSupported,
    #[error("not enough points")]
    NotEnoughPoints,
    #[error("invalid shape")]
    InvalidShape,
    #[error("unknown camera")]
    UnknownCamera,
    #[error("SVD failed")]
    SvdFailed,
    #[error("Parsing error")]
    ParseError,
    #[error("invalid rotation matrix")]
    InvalidRotationMatrix,
    #[error("unsupported version")]
    UnsupportedVersion,
    #[error("invalid rect matrix")]
    InvalidRectMatrix,
    #[error("unsupported type")]
    UnsupportedType,
    #[cfg(feature = "rerun-io")]
    #[error("rerun does not support this model of camera intrinsics")]
    RerunUnsupportedIntrinsics,
    #[error("multiple valid roots found")]
    MultipleValidRootsFound,
    #[error("no valid root found")]
    NoValidRootFound,
    #[error("IO error: {source}")]
    Io {
        #[from]
        source: std::io::Error,
    },
    #[error("serde_yaml error: {source}")]
    SerdeYaml {
        #[from]
        source: serde_yaml::Error,
    },
    #[error("serde_json error: {source}")]
    SerdeJson {
        #[from]
        source: serde_json::Error,
    },
    #[error("SvgError: {}", error)]
    SvgError { error: &'static str },
    #[error("PinvError: {}", error)]
    PinvError { error: String },
    #[error("cam_geom::Error: {source}")]
    CamGeomError {
        #[from]
        source: cam_geom::Error,
    },
    #[error("opencv_ros_camera::Error: {source}")]
    OpencvRosError {
        #[from]
        source: opencv_ros_camera::Error,
    },
}

pub type Result<M> = std::result::Result<M, MvgError>;

mod pymvg_support;

pub mod intrinsics;

pub mod extrinsics;

pub mod align_points;

#[cfg(feature = "rerun-io")]
pub mod rerun_io;

mod camera;
pub use crate::camera::{rq_decomposition, Camera};

mod multi_cam_system;
pub use crate::multi_cam_system::MultiCameraSystem;

#[derive(Debug, Clone)]
pub struct DistortedPixel<R: RealField + Copy> {
    pub coords: Point2<R>,
}

impl<R, IN> From<&cam_geom::Pixels<R, U1, IN>> for DistortedPixel<R>
where
    R: RealField + Copy,
    IN: nalgebra::storage::Storage<R, U1, U2>,
{
    fn from(orig: &cam_geom::Pixels<R, U1, IN>) -> Self {
        DistortedPixel {
            coords: Point2::new(orig.data[(0, 0)], orig.data[(0, 1)]),
        }
    }
}

impl<R, IN> From<cam_geom::Pixels<R, U1, IN>> for DistortedPixel<R>
where
    R: RealField + Copy,
    IN: nalgebra::storage::Storage<R, U1, U2>,
{
    fn from(orig: cam_geom::Pixels<R, U1, IN>) -> Self {
        let orig_ref = &orig;
        orig_ref.into()
    }
}

impl<R> From<&DistortedPixel<R>> for cam_geom::Pixels<R, U1, na::storage::Owned<R, U1, U2>>
where
    R: RealField + Copy,
    na::DefaultAllocator: na::allocator::Allocator<U1, U2>,
{
    fn from(orig: &DistortedPixel<R>) -> Self {
        Self {
            data: na::OMatrix::<R, U1, U2>::from_row_slice(&[orig.coords[0], orig.coords[1]]),
        }
    }
}

impl<R: RealField + Copy> DistortedPixel<R> {
    pub fn from_pixels<NPTS, IN>(pixels: &cam_geom::Pixels<R, NPTS, IN>, i: usize) -> Self
    where
        NPTS: Dim,
        IN: nalgebra::storage::Storage<R, NPTS, U2>,
    {
        DistortedPixel {
            coords: Point2::new(pixels.data[(i, 0)], pixels.data[(i, 1)]),
        }
    }
}

#[derive(Debug, Clone)]
pub struct UndistortedPixel<R: RealField + Copy> {
    pub coords: Point2<R>,
}

impl<R, IN> From<&opencv_ros_camera::UndistortedPixels<R, U1, IN>> for UndistortedPixel<R>
where
    R: RealField + Copy,
    IN: nalgebra::storage::Storage<R, U1, U2>,
{
    fn from(orig: &opencv_ros_camera::UndistortedPixels<R, U1, IN>) -> Self {
        UndistortedPixel {
            coords: Point2::new(orig.data[(0, 0)], orig.data[(0, 1)]),
        }
    }
}

impl<R, IN> From<opencv_ros_camera::UndistortedPixels<R, U1, IN>> for UndistortedPixel<R>
where
    R: RealField + Copy,
    IN: nalgebra::storage::Storage<R, U1, U2>,
{
    fn from(orig: opencv_ros_camera::UndistortedPixels<R, U1, IN>) -> Self {
        let orig_ref = &orig;
        orig_ref.into()
    }
}

impl<R> From<&UndistortedPixel<R>>
    for opencv_ros_camera::UndistortedPixels<R, U1, na::storage::Owned<R, U1, U2>>
where
    R: RealField + Copy,
    na::DefaultAllocator: na::allocator::Allocator<U1, U2>,
{
    fn from(orig: &UndistortedPixel<R>) -> Self {
        Self {
            data: na::OMatrix::<R, U1, U2>::from_row_slice(&[orig.coords[0], orig.coords[1]]),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PointCameraFrame<R: RealField + Copy> {
    pub coords: Point3<R>,
}

impl<R, IN> From<&cam_geom::Points<cam_geom::coordinate_system::CameraFrame, R, U1, IN>>
    for PointCameraFrame<R>
where
    R: RealField + Copy,
    IN: nalgebra::storage::Storage<R, U1, U3>,
{
    fn from(orig: &cam_geom::Points<cam_geom::coordinate_system::CameraFrame, R, U1, IN>) -> Self {
        PointCameraFrame {
            coords: Point3::new(orig.data[(0, 0)], orig.data[(0, 1)], orig.data[(0, 2)]),
        }
    }
}

impl<R, IN> From<cam_geom::Points<cam_geom::coordinate_system::CameraFrame, R, U1, IN>>
    for PointCameraFrame<R>
where
    R: RealField + Copy,
    IN: nalgebra::storage::Storage<R, U1, U3>,
{
    fn from(orig: cam_geom::Points<cam_geom::coordinate_system::CameraFrame, R, U1, IN>) -> Self {
        let orig_ref = &orig;
        orig_ref.into()
    }
}

impl<R> From<&PointCameraFrame<R>>
    for cam_geom::Points<
        cam_geom::coordinate_system::CameraFrame,
        R,
        U1,
        na::storage::Owned<R, U1, U3>,
    >
where
    R: RealField + Copy,
    na::DefaultAllocator: na::allocator::Allocator<U1, U2>,
{
    fn from(orig: &PointCameraFrame<R>) -> Self {
        Self::new(na::OMatrix::<R, U1, U3>::new(
            orig.coords[0],
            orig.coords[1],
            orig.coords[2],
        ))
    }
}

#[derive(Debug, Clone)]
pub struct PointWorldFrame<R: RealField + Copy> {
    pub coords: Point3<R>,
}

impl<R, IN> From<&cam_geom::Points<cam_geom::coordinate_system::WorldFrame, R, U1, IN>>
    for PointWorldFrame<R>
where
    R: RealField + Copy,
    IN: nalgebra::storage::Storage<R, U1, U3>,
{
    fn from(orig: &cam_geom::Points<cam_geom::coordinate_system::WorldFrame, R, U1, IN>) -> Self {
        PointWorldFrame {
            coords: Point3::new(orig.data[(0, 0)], orig.data[(0, 1)], orig.data[(0, 2)]),
        }
    }
}

impl<R, IN> From<cam_geom::Points<cam_geom::coordinate_system::WorldFrame, R, U1, IN>>
    for PointWorldFrame<R>
where
    R: RealField + Copy,
    IN: nalgebra::storage::Storage<R, U1, U3>,
{
    fn from(orig: cam_geom::Points<cam_geom::coordinate_system::WorldFrame, R, U1, IN>) -> Self {
        let orig_ref = &orig;
        orig_ref.into()
    }
}

impl<R> From<&PointWorldFrame<R>>
    for cam_geom::Points<
        cam_geom::coordinate_system::WorldFrame,
        R,
        U1,
        na::storage::Owned<R, U1, U3>,
    >
where
    R: RealField + Copy,
    na::DefaultAllocator: na::allocator::Allocator<U1, U2>,
{
    fn from(orig: &PointWorldFrame<R>) -> Self {
        Self::new(na::OMatrix::<R, U1, U3>::new(
            orig.coords[0],
            orig.coords[1],
            orig.coords[2],
        ))
    }
}

pub fn vec_sum<R: RealField + Copy>(vec: &[R]) -> R {
    vec.iter().fold(na::convert(0.0), |acc, i| acc + *i)
}

#[derive(Debug, Clone)]
pub struct PointWorldFrameWithSumReprojError<R: RealField + Copy> {
    pub point: PointWorldFrame<R>,
    pub cum_reproj_dist: R,
    pub mean_reproj_dist: R,
    pub reproj_dists: Vec<R>,
}

impl<R: RealField + Copy> PointWorldFrameWithSumReprojError<R> {
    pub fn new(point: PointWorldFrame<R>, reproj_dists: Vec<R>) -> Self {
        let cum_reproj_dist = vec_sum(&reproj_dists);
        let n_cams: R = na::convert(reproj_dists.len() as f64);
        let mean_reproj_dist = cum_reproj_dist / n_cams;
        Self {
            point,
            cum_reproj_dist,
            mean_reproj_dist,
            reproj_dists,
        }
    }
}

#[derive(Debug, Clone)]
pub enum PointWorldFrameMaybeWithSumReprojError<R: RealField + Copy> {
    Point(PointWorldFrame<R>),
    WithSumReprojError(PointWorldFrameWithSumReprojError<R>),
}

impl<R: RealField + Copy> PointWorldFrameMaybeWithSumReprojError<R> {
    pub fn point(self) -> PointWorldFrame<R> {
        use crate::PointWorldFrameMaybeWithSumReprojError::*;
        match self {
            Point(pt) => pt,
            WithSumReprojError(pto) => pto.point,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WorldCoordAndUndistorted2D<R: RealField + Copy> {
    wc: PointWorldFrameMaybeWithSumReprojError<R>,
    upoints: Vec<(String, UndistortedPixel<R>)>,
}

impl<R: RealField + Copy> WorldCoordAndUndistorted2D<R> {
    pub fn new(
        wc: PointWorldFrameMaybeWithSumReprojError<R>,
        upoints: Vec<(String, UndistortedPixel<R>)>,
    ) -> Self {
        Self { wc, upoints }
    }
    pub fn point(self) -> PointWorldFrame<R> {
        self.wc.point()
    }
    pub fn wc_and_upoints(
        self,
    ) -> (
        PointWorldFrameMaybeWithSumReprojError<R>,
        Vec<(String, UndistortedPixel<R>)>,
    ) {
        (self.wc, self.upoints)
    }
}

pub fn make_default_intrinsics<R: RealField + Copy>() -> RosOpenCvIntrinsics<R> {
    let cx = na::convert(320.0);
    let cy = na::convert(240.0);
    let fx = na::convert(1000.0);
    let skew = na::convert(0.0);
    let fy = fx;
    RosOpenCvIntrinsics::from_params(fx, skew, fy, cx, cy)
}

#[cfg(test)]
mod tests {
    use crate::*;

    fn get_test_intrinsics() -> Vec<(String, RosOpenCvIntrinsics<f64>)> {
        use na::Vector5;
        let mut result = Vec::new();

        for (name, dist) in &[
            (
                "linear",
                Distortion::from_opencv_vec(Vector5::new(0.0, 0.0, 0.0, 0.0, 0.0)),
            ),
            (
                "d1",
                Distortion::from_opencv_vec(Vector5::new(0.1001, 0.2002, 0.3003, 0.4004, 0.5005)),
            ),
        ] {
            for skew in &[0, 10] {
                let fx = 100.0;
                let fy = 100.0;
                let cx = 320.0;
                let cy = 240.0;

                let cam = RosOpenCvIntrinsics::from_params_with_distortion(
                    fx,
                    *skew as f64,
                    fy,
                    cx,
                    cy,
                    dist.clone(),
                );
                result.push((format!("dist-{name}_skew{skew}"), cam));
            }
        }

        result.push(("default".to_string(), make_default_intrinsics()));

        result
    }

    pub(crate) fn get_test_cameras() -> Vec<(String, Camera<f64>)> {
        let mut result = Vec::new();

        use na::core::dimension::U4;
        use na::core::OMatrix;

        #[rustfmt::skip]
        let pmat = OMatrix::<f64,U3,U4>::new(100.0, 0.0, 0.0, 0.01,
            0.0, 100.0, 0.0, 0.01,
            320.0, 240.0, 1.0, 0.01);
        let cam = crate::Camera::from_pmat(640, 480, &pmat).expect("generate test cam from pmat");
        result.insert(0, ("from-pmat-1".to_string(), cam));

        let extrinsics = crate::extrinsics::make_default_extrinsics();
        for (int_name, intrinsics) in get_test_intrinsics().into_iter() {
            let name = format!("cam-{int_name}");
            let cam = Camera::new(640, 480, extrinsics.clone(), intrinsics).unwrap();
            result.push((name, cam));
        }
        result.push(("default-cam".to_string(), Camera::default()));

        let mut result2 = vec![];
        for (name, cam) in result {
            if &name == "cam-dist-linear_skew0" {
                result2.push((name, cam));
            }
        }
        result2
    }
}
