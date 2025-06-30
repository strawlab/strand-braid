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

/// Error types that can occur during multi-view geometry operations.
///
/// This enum encompasses all possible errors that can arise during camera calibration,
/// 3D reconstruction, coordinate transformations, and other geometric computations.
///
/// # Categories
///
/// - **Model Errors**: Issues with distortion models or camera parameters
/// - **Geometric Errors**: Problems with mathematical operations (SVD, matrix operations)
/// - **I/O Errors**: File reading/writing and serialization failures
/// - **Validation Errors**: Invalid input data or insufficient constraints
///
/// # Example
///
/// ```rust
/// use braid_mvg::{MvgError, Result};
///
/// fn triangulate_points() -> Result<()> {
///     // Function that might fail with various MVG errors
///     Err(MvgError::NotEnoughPoints)
/// }
///
/// match triangulate_points() {
///     Ok(_) => println!("Triangulation successful"),
///     Err(MvgError::NotEnoughPoints) => println!("Need at least 2 cameras for triangulation"),
///     Err(e) => println!("Other error: {}", e),
/// }
/// ```
#[derive(Error, Debug)]
pub enum MvgError {
    /// Unknown or unsupported lens distortion model encountered.
    ///
    /// This error occurs when trying to work with a camera distortion model
    /// that is not supported by the current implementation.
    #[error("unknown distortion model")]
    UnknownDistortionModel,
    /// Rectification matrix is not supported by the current implementation.
    ///
    /// Rectification matrices are used in stereo vision but are not fully
    /// supported by all operations in this crate.
    #[error("rectification matrix not supported")]
    RectificationMatrixNotSupported,
    /// Insufficient points provided for the geometric operation.
    ///
    /// Many operations like triangulation require a minimum number of observations.
    /// For example, 3D triangulation needs at least 2 camera views.
    #[error("not enough points")]
    NotEnoughPoints,
    /// Invalid matrix or array dimensions for the operation.
    ///
    /// This occurs when input data has incompatible dimensions for the
    /// requested mathematical operation.
    #[error("invalid shape")]
    InvalidShape,
    /// Camera name not found in the multi-camera system.
    ///
    /// Thrown when referencing a camera by name that doesn't exist in the
    /// current [`MultiCameraSystem`].
    #[error("unknown camera")]
    UnknownCamera,
    /// Singular Value Decomposition failed during matrix operations.
    ///
    /// This can occur during camera calibration or 3D reconstruction when
    /// the input data is degenerate or ill-conditioned.
    #[error("SVD failed")]
    SvdFailed,
    /// Generic parsing error for configuration files or data formats.
    #[error("Parsing error")]
    ParseError,
    /// Invalid rotation matrix (not orthogonal or determinant ≠ 1).
    ///
    /// Rotation matrices must be orthogonal with determinant +1 to represent
    /// valid 3D rotations.
    #[error("invalid rotation matrix")]
    InvalidRotationMatrix,
    /// Unsupported file format or schema version.
    #[error("unsupported version")]
    UnsupportedVersion,
    /// Invalid rectification matrix parameters.
    #[error("invalid rect matrix")]
    InvalidRectMatrix,
    /// Unsupported camera or parameter type.
    #[error("unsupported type")]
    UnsupportedType,
    /// Rerun.io does not support this camera intrinsics model.
    ///
    /// Only available when the `rerun-io` feature is enabled.
    /// Some complex distortion models cannot be exported to rerun.io format.
    #[cfg(feature = "rerun-io")]
    #[error("rerun does not support this model of camera intrinsics")]
    RerunUnsupportedIntrinsics,
    /// Multiple valid mathematical roots found where only one was expected.
    ///
    /// This can occur in polynomial root-finding algorithms used in
    /// geometric computations.
    #[error("multiple valid roots found")]
    MultipleValidRootsFound,
    /// No valid mathematical root found for the equation.
    ///
    /// This indicates that the geometric problem has no solution with
    /// the given constraints.
    #[error("no valid root found")]
    NoValidRootFound,
    /// I/O error during file operations.
    #[error("IO error: {source}")]
    Io {
        /// The underlying I/O error.
        #[from]
        source: std::io::Error,
    },
    /// YAML serialization/deserialization error.
    #[error("serde_yaml error: {source}")]
    SerdeYaml {
        /// The underlying YAML parsing error.
        #[from]
        source: serde_yaml::Error,
    },
    /// JSON serialization/deserialization error.
    #[error("serde_json error: {source}")]
    SerdeJson {
        /// The underlying JSON parsing error.
        #[from]
        source: serde_json::Error,
    },
    /// SVG rendering or processing error.
    #[error("SvgError: {}", error)]
    SvgError {
        /// The SVG error message.
        error: &'static str,
    },
    /// Pseudo-inverse calculation error.
    #[error("PinvError: {}", error)]
    PinvError {
        /// The pseudo-inverse error message.
        error: String,
    },
    /// Error from the [`cam-geom`](https://crates.io/crates/cam-geom) crate.
    #[error("cam_geom::Error: {source}")]
    CamGeomError {
        /// The underlying cam-geom error.
        #[from]
        source: cam_geom::Error,
    },
    /// Error from the [`opencv-ros-camera`](https://crates.io/crates/opencv-ros-camera) crate.
    #[error("opencv_ros_camera::Error: {source}")]
    OpencvRosError {
        /// The underlying opencv-ros-camera error.
        #[from]
        source: opencv_ros_camera::Error,
    },
}

/// Convenience type alias for results in multi-view geometry operations.
///
/// This is a standard Rust `Result<T, E>` type where the error type is fixed
/// to [`MvgError`]. Most functions in this crate return this type.
///
/// # Example
///
/// ```rust
/// use braid_mvg::{Result, MvgError};
///
/// fn some_mvg_operation() -> Result<f64> {
///     // Some operation that might fail
///     Ok(42.0)
/// }
/// ```
pub type Result<M> = std::result::Result<M, MvgError>;

pub mod pymvg_support;

/// Camera intrinsic parameter utilities and operations.
///
/// This module provides utilities for working with camera intrinsic parameters,
/// including lens distortion models, focal length operations, and coordinate
/// transformations within the image plane.
pub mod intrinsics;

/// Camera extrinsic parameter utilities and factory functions.
///
/// This module provides functions for creating and manipulating camera extrinsic
/// parameters, including pose construction from various representations and
/// coordinate frame transformations.
pub mod extrinsics;

/// Point cloud alignment algorithms and utilities.
///
/// This module implements various algorithms for aligning point clouds and
/// coordinate systems, including Kabsch-Umeyama and robust Arun methods.
/// These are commonly used in camera calibration and 3D reconstruction.
pub mod align_points;

/// Integration with [rerun.io](https://rerun.io) for 3D visualization.
///
/// This module provides conversion utilities between braid-mvg types and
/// rerun.io data structures, enabling 3D visualization of camera systems,
/// point clouds, and tracking results.
///
/// **Note**: This module is only available when the `rerun-io` feature is enabled.
#[cfg(feature = "rerun-io")]
#[cfg_attr(docsrs, doc(cfg(feature = "rerun-io")))]
pub mod rerun_io;

mod camera;
pub use crate::camera::{rq_decomposition, Camera};

mod multi_cam_system;
pub use crate::multi_cam_system::MultiCameraSystem;

/// A 2D pixel coordinate in the distorted image space.
///
/// This represents pixel coordinates as they appear in the raw camera image,
/// including the effects of lens distortion. These coordinates correspond
/// to the actual pixel locations in the camera sensor.
///
/// # Coordinate System
///
/// - **Origin**: Top-left corner of the image (0, 0)
/// - **X-axis**: Increases to the right
/// - **Y-axis**: Increases downward
/// - **Units**: Pixels
///
/// # Relationship to Other Coordinate Types
///
/// - [`UndistortedPixel`]: Corrected version with distortion removed
/// - [`PointCameraFrame`]: 3D coordinates in camera reference frame
/// - [`PointWorldFrame`]: 3D coordinates in world reference frame
///
/// # Example
///
/// ```rust
/// use braid_mvg::DistortedPixel;
/// use nalgebra::Point2;
///
/// let pixel = DistortedPixel {
///     coords: Point2::new(320.5, 240.3)
/// };
/// println!("Pixel at ({}, {})", pixel.coords.x, pixel.coords.y);
/// ```
#[derive(Debug, Clone)]
pub struct DistortedPixel<R: RealField + Copy> {
    /// The 2D pixel coordinates (x, y) in the distorted image.
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
    /// Extract a single distorted pixel from a collection of pixels.
    ///
    /// This method allows you to extract one pixel coordinate from a larger
    /// collection of pixel coordinates, such as those returned by batch
    /// projection operations.
    ///
    /// # Arguments
    ///
    /// * `pixels` - A collection of pixel coordinates
    /// * `i` - The index of the pixel to extract (0-based)
    ///
    /// # Example
    ///
    /// ```rust
    /// use braid_mvg::DistortedPixel;
    /// use cam_geom::Pixels;
    /// use nalgebra::{Point2, OMatrix, U2};
    ///
    /// // This example would work with actual cam_geom::Pixels data
    /// // let pixels = /* some cam_geom::Pixels instance */;
    /// // let first_pixel = DistortedPixel::from_pixels(&pixels, 0);
    /// ```
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

/// A 2D pixel coordinate in the undistorted (rectified) image space.
///
/// This represents pixel coordinates after lens distortion has been removed,
/// corresponding to an ideal pinhole camera model. These coordinates are used
/// for geometric computations like triangulation and epipolar geometry.
///
/// # Coordinate System
///
/// - **Origin**: Top-left corner of the undistorted image (0, 0)
/// - **X-axis**: Increases to the right
/// - **Y-axis**: Increases downward
/// - **Units**: Pixels (in the undistorted image space)
///
/// # Mathematical Properties
///
/// Undistorted pixels follow the ideal pinhole camera model:
/// ```text
/// [u]   [fx  s  cx] [X/Z]
/// [v] = [ 0 fy  cy] [Y/Z]
/// [1]   [ 0  0   1] [ 1 ]
/// ```
/// where (X,Y,Z) are 3D camera coordinates and (u,v) are undistorted pixels.
///
/// # Example
///
/// ```rust
/// use braid_mvg::UndistortedPixel;
/// use nalgebra::Point2;
///
/// let undistorted = UndistortedPixel {
///     coords: Point2::new(320.0, 240.0)
/// };
/// // This pixel can be used directly in geometric computations
/// ```
#[derive(Debug, Clone)]
pub struct UndistortedPixel<R: RealField + Copy> {
    /// The 2D pixel coordinates (x, y) in the undistorted image.
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

/// A 3D point in the camera coordinate frame.
///
/// This represents a 3D point in the coordinate system of a specific camera,
/// where the camera is positioned at the origin and oriented according to
/// the standard computer vision convention.
///
/// # Coordinate System Convention
///
/// - **Origin**: Camera center (optical center)
/// - **X-axis**: Points to the right of the camera
/// - **Y-axis**: Points downward from the camera
/// - **Z-axis**: Points forward along the optical axis (into the scene)
/// - **Units**: Typically meters or millimeters
///
/// # Mathematical Relationship
///
/// Camera frame coordinates relate to world coordinates via the extrinsic parameters:
/// ```text
/// [X_cam]   [R | t] [X_world]
/// [Y_cam] = [0 | 1] [Y_world]
/// [Z_cam]           [Z_world]
///                   [   1   ]
/// ```
/// where R is the rotation matrix and t is the translation vector.
///
/// # Example
///
/// ```rust
/// use braid_mvg::PointCameraFrame;
/// use nalgebra::Point3;
///
/// // A point 1 meter in front of the camera, slightly to the right and up
/// let camera_point = PointCameraFrame {
///     coords: Point3::new(0.1, -0.05, 1.0)
/// };
/// ```
#[derive(Debug, Clone)]
pub struct PointCameraFrame<R: RealField + Copy> {
    /// The 3D coordinates (x, y, z) in the camera reference frame.
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

/// A 3D point in the world coordinate frame.
///
/// This represents a 3D point in a global coordinate system that is independent
/// of any specific camera. This is the primary coordinate system for storing
/// and manipulating 3D scene geometry.
///
/// # Coordinate System
///
/// The world coordinate system is arbitrary and defined by the user or
/// calibration process. Common conventions include:
/// - **Origin**: Often at a calibration target or scene reference point
/// - **Axes**: User-defined, but typically aligned with scene geometry
/// - **Units**: Typically meters, millimeters, or other real-world units
///
/// # Usage in Multi-View Geometry
///
/// World frame points are the output of 3D triangulation and the input
/// for projection into camera images. They provide a camera-independent
/// representation of 3D scene structure.
///
/// # Example
///
/// ```rust
/// use braid_mvg::PointWorldFrame;
/// use nalgebra::Point3;
///
/// // A point 2 meters above the origin, 1 meter east, 3 meters north
/// let world_point = PointWorldFrame {
///     coords: Point3::new(1.0, 3.0, 2.0)
/// };
/// ```
#[derive(Debug, Clone)]
pub struct PointWorldFrame<R: RealField + Copy> {
    /// The 3D coordinates (x, y, z) in the world reference frame.
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

/// Compute the sum of elements in a vector.
///
/// This is a utility function that computes the sum of all elements in a slice
/// of numeric values. It's used internally for computing reprojection error
/// statistics and other aggregate measures.
///
/// # Arguments
///
/// * `vec` - A slice of numeric values to sum
///
/// # Returns
///
/// The sum of all elements in the vector
///
/// # Example
///
/// ```rust
/// use braid_mvg::vec_sum;
///
/// let values = vec![1.0, 2.5, 3.2, 0.8];
/// let total = vec_sum(&values);
/// assert_eq!(total, 7.5);
/// ```
pub fn vec_sum<R: RealField + Copy>(vec: &[R]) -> R {
    vec.iter().fold(na::convert(0.0), |acc, i| acc + *i)
}

/// A 3D world point with associated reprojection error statistics.
///
/// This structure extends [`PointWorldFrame`] with additional information about
/// how well the reconstructed 3D point reprojects back to the original 2D
/// observations in each camera. This is useful for quality assessment and
/// outlier detection in 3D reconstruction.
///
/// # Reprojection Error
///
/// Reprojection error measures how far the reconstructed 3D point projects
/// from the original 2D observations when projected back into each camera.
/// Lower values indicate better reconstruction quality.
///
/// # Fields
///
/// - `point`: The reconstructed 3D point in world coordinates
/// - `cum_reproj_dist`: Sum of reprojection distances across all cameras
/// - `mean_reproj_dist`: Average reprojection distance per camera
/// - `reproj_dists`: Individual reprojection distance for each camera
///
/// # Example
///
/// ```rust
/// use braid_mvg::{PointWorldFrame, PointWorldFrameWithSumReprojError};
/// use nalgebra::Point3;
///
/// let point = PointWorldFrame { coords: Point3::new(1.0, 2.0, 3.0) };
/// let errors = vec![0.5, 0.3, 0.8]; // reprojection errors for 3 cameras
///
/// let point_with_error = PointWorldFrameWithSumReprojError::new(point, errors);
/// println!("Mean reprojection error: {:.3}", point_with_error.mean_reproj_dist);
/// ```
#[derive(Debug, Clone)]
pub struct PointWorldFrameWithSumReprojError<R: RealField + Copy> {
    /// The reconstructed 3D point in world coordinates.
    pub point: PointWorldFrame<R>,
    /// Sum of reprojection distances from all cameras.
    pub cum_reproj_dist: R,
    /// Average reprojection distance per camera.
    pub mean_reproj_dist: R,
    /// Individual reprojection distances for each camera.
    pub reproj_dists: Vec<R>,
}

impl<R: RealField + Copy> PointWorldFrameWithSumReprojError<R> {
    /// Create a new point with reprojection error statistics.
    ///
    /// This constructor automatically computes the cumulative and mean
    /// reprojection errors from the individual camera errors.
    ///
    /// # Arguments
    ///
    /// * `point` - The 3D point in world coordinates
    /// * `reproj_dists` - Vector of reprojection distances, one per camera
    ///
    /// # Returns
    ///
    /// A new instance with computed error statistics
    ///
    /// # Example
    ///
    /// ```rust
    /// use braid_mvg::{PointWorldFrame, PointWorldFrameWithSumReprojError};
    /// use nalgebra::Point3;
    ///
    /// let point = PointWorldFrame { coords: Point3::new(0.0, 0.0, 5.0) };
    /// let errors = vec![0.1, 0.2, 0.15]; // errors from 3 cameras
    ///
    /// let result = PointWorldFrameWithSumReprojError::new(point, errors);
    /// assert!((result.mean_reproj_dist - 0.15f64).abs() < 1e-10);
    /// assert!((result.cum_reproj_dist - 0.45f64).abs() < 1e-10);
    /// ```
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

/// A 3D world point that may or may not include reprojection error information.
///
/// This enum allows functions to return either a simple 3D point or a point
/// with additional reprojection error statistics, depending on the operation
/// performed and the level of detail requested.
///
/// # Variants
///
/// * `Point` - Contains just the 3D coordinates
/// * `WithSumReprojError` - Contains 3D coordinates plus reprojection error analysis
///
/// # Example
///
/// ```rust
/// use braid_mvg::{PointWorldFrame, PointWorldFrameMaybeWithSumReprojError};
/// use nalgebra::Point3;
///
/// let simple_point = PointWorldFrame { coords: Point3::new(1.0, 2.0, 3.0) };
/// let maybe_point = PointWorldFrameMaybeWithSumReprojError::Point(simple_point);
///
/// // Extract just the point coordinates regardless of variant
/// let extracted_point = maybe_point.point();
/// ```
#[derive(Debug, Clone)]
pub enum PointWorldFrameMaybeWithSumReprojError<R: RealField + Copy> {
    /// A simple 3D point without error information.
    Point(PointWorldFrame<R>),
    /// A 3D point with reprojection error statistics.
    WithSumReprojError(PointWorldFrameWithSumReprojError<R>),
}

impl<R: RealField + Copy> PointWorldFrameMaybeWithSumReprojError<R> {
    /// Extract the 3D point coordinates regardless of the variant type.
    ///
    /// This method provides a uniform way to get the 3D point coordinates
    /// whether the enum contains a simple point or a point with error statistics.
    ///
    /// # Returns
    ///
    /// The [`PointWorldFrame`] containing the 3D coordinates
    ///
    /// # Example
    ///
    /// ```rust
    /// use braid_mvg::{PointWorldFrame, PointWorldFrameMaybeWithSumReprojError};
    /// use nalgebra::Point3;
    ///
    /// let point = PointWorldFrame { coords: Point3::new(1.0, 2.0, 3.0) };
    /// let maybe_point = PointWorldFrameMaybeWithSumReprojError::Point(point);
    ///
    /// let extracted = maybe_point.point();
    /// assert_eq!(extracted.coords, Point3::new(1.0, 2.0, 3.0));
    /// ```
    pub fn point(self) -> PointWorldFrame<R> {
        use crate::PointWorldFrameMaybeWithSumReprojError::*;
        match self {
            Point(pt) => pt,
            WithSumReprojError(pto) => pto.point,
        }
    }
}

/// A combined data structure containing a 3D world point and its 2D camera observations.
///
/// This structure packages together a 3D point (possibly with reprojection errors)
/// and the corresponding 2D undistorted pixel observations from each camera.
/// This is useful for algorithms that need to work with both the 3D structure
/// and the 2D observations simultaneously.
///
/// # Use Cases
///
/// - Bundle adjustment optimization
/// - Outlier detection and filtering
/// - Tracking and correspondence validation
/// - Quality assessment of triangulation results
///
/// # Example
///
/// ```rust
/// use braid_mvg::{WorldCoordAndUndistorted2D, PointWorldFrame,
///                 PointWorldFrameMaybeWithSumReprojError, UndistortedPixel};
/// use nalgebra::{Point2, Point3};
///
/// let point = PointWorldFrame { coords: Point3::new(1.0, 2.0, 3.0) };
/// let maybe_point = PointWorldFrameMaybeWithSumReprojError::Point(point);
///
/// let observations = vec![
///     ("cam1".to_string(), UndistortedPixel { coords: Point2::new(320.0, 240.0) }),
///     ("cam2".to_string(), UndistortedPixel { coords: Point2::new(340.0, 250.0) }),
/// ];
///
/// let combined = WorldCoordAndUndistorted2D::new(maybe_point, observations);
/// ```
#[derive(Debug, Clone)]
pub struct WorldCoordAndUndistorted2D<R: RealField + Copy> {
    /// The 3D world point (possibly with reprojection error information).
    wc: PointWorldFrameMaybeWithSumReprojError<R>,
    /// The 2D undistorted pixel observations from each camera.
    /// Each entry is a (camera_name, pixel_coordinate) pair.
    upoints: Vec<(String, UndistortedPixel<R>)>,
}

impl<R: RealField + Copy> WorldCoordAndUndistorted2D<R> {
    /// Create a new combined data structure.
    ///
    /// # Arguments
    ///
    /// * `wc` - The 3D world coordinates (possibly with error information)
    /// * `upoints` - Vector of (camera_name, undistorted_pixel) pairs
    ///
    /// # Returns
    ///
    /// A new instance combining the 3D and 2D information
    pub fn new(
        wc: PointWorldFrameMaybeWithSumReprojError<R>,
        upoints: Vec<(String, UndistortedPixel<R>)>,
    ) -> Self {
        Self { wc, upoints }
    }

    /// Extract just the 3D point coordinates.
    ///
    /// # Returns
    ///
    /// The [`PointWorldFrame`] containing the 3D coordinates
    pub fn point(self) -> PointWorldFrame<R> {
        self.wc.point()
    }

    /// Decompose into the constituent 3D and 2D components.
    ///
    /// # Returns
    ///
    /// A tuple containing:
    /// - The 3D world coordinates (possibly with error information)
    /// - The vector of 2D observations from each camera
    pub fn wc_and_upoints(
        self,
    ) -> (
        PointWorldFrameMaybeWithSumReprojError<R>,
        Vec<(String, UndistortedPixel<R>)>,
    ) {
        (self.wc, self.upoints)
    }
}

/// Create default camera intrinsic parameters for testing and prototyping.
///
/// This function generates reasonable default intrinsic parameters for a typical
/// camera. These parameters are suitable for algorithm testing, unit tests, and
/// as starting points for calibration procedures.
///
/// # Default Parameters
///
/// - **Focal length**: fx = fy = 1000 pixels
/// - **Principal point**: (cx, cy) = (320, 240) pixels
/// - **Skew**: 0 (rectangular pixels)
/// - **Distortion**: None (pinhole model)
///
/// These defaults assume a VGA-sized image (640×480) with the principal point
/// at the center and a reasonable focal length.
///
/// # ⚠️ Important Note
///
/// These parameters are **not suitable for real applications** - always perform
/// proper camera calibration for production use. The default values are chosen
/// for testing convenience and may not represent realistic camera behavior.
///
/// # Example
///
/// ```rust
/// use braid_mvg::make_default_intrinsics;
///
/// let intrinsics = make_default_intrinsics::<f64>();
/// println!("Default focal length: {}", intrinsics.fx());
/// println!("Default principal point: ({}, {})", intrinsics.cx(), intrinsics.cy());
/// ```
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
