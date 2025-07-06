#![allow(non_snake_case)]

use serde::Deserialize;

use na::core::dimension::{U1, U2, U3, U4};
use na::core::{Matrix3, Matrix4, OMatrix, Vector3, Vector5};
use na::geometry::{Point2, Point3, Rotation3, UnitQuaternion};
use na::{allocator::Allocator, DefaultAllocator, RealField};
use nalgebra as na;
use num_traits::{One, Zero};

use opencv_ros_camera::UndistortedPixels;

use crate::pymvg_support::PymvgCamera;
use crate::{
    DistortedPixel, Distortion, ExtrinsicParameters, MvgError, PointWorldFrame, Result,
    RosOpenCvIntrinsics, UndistortedPixel,
};

#[derive(Clone, PartialEq)]
/// A calibrated camera with both intrinsic and extrinsic parameters.
///
/// This structure represents a complete camera model including:
/// - **Intrinsic parameters**: focal length, principal point, distortion
///   coefficients
/// - **Extrinsic parameters**: position and orientation in 3D space
/// - **Image dimensions**: width and height in pixels
///
/// The camera follows the standard computer vision coordinate conventions:
/// - Camera frame: X→right, Y→down, Z→forward (optical axis)
/// - Image coordinates: origin at top-left, X→right, Y→down
///
/// # Mathematical Model
///
/// The camera implements the projective camera model:
/// ```text
/// s[u v 1]ᵀ = K[R|t][X Y Z 1]ᵀ
/// ```
/// where:
/// - `(X,Y,Z)` are 3D world coordinates
/// - `(u,v)` are 2D undistorted image coordinates
/// - `K` is the intrinsic matrix
/// - `[R|t]` represents rotation and translation (extrinsics)
/// - `s` is a scaling factor
///
/// Lens distortion is supported via the
/// [`opencv-ros-camera`](https://docs.rs/opencv-ros-camera) crate.
///
/// The parameters for the intrinsic matrix (focal length, principal point, and
/// skew) in addition to the distortion parameters together comprise the
/// intrinsic parameters, or "intrinsics".
///
/// # Example
///
/// ```rust
/// use braid_mvg::{Camera, extrinsics, make_default_intrinsics};
///
/// // Create a camera with default parameters
/// let extrinsics = extrinsics::make_default_extrinsics::<f64>();
/// let intrinsics = make_default_intrinsics::<f64>();
/// let camera = Camera::new(640, 480, extrinsics, intrinsics).unwrap();
///
/// // Project a 3D point to 2D
/// use braid_mvg::PointWorldFrame;
/// use nalgebra::Point3;
/// let point_3d = PointWorldFrame { coords: Point3::new(0.0, 0.0, 5.0) };
/// let pixel = camera.project_3d_to_pixel(&point_3d);
/// ```
pub struct Camera<R: RealField> {
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) inner: cam_geom::Camera<R, RosOpenCvIntrinsics<R>>,
    pub(crate) cache: CameraCache<R>,
}

impl<R: RealField + Copy> Camera<R> {
    /// Create a new camera from intrinsic and extrinsic parameters.
    ///
    /// This constructor creates a complete camera model by combining:
    /// - Image dimensions (width, height)
    /// - Extrinsic parameters (camera pose in world coordinates)
    /// - Intrinsic parameters (focal length, principal point, distortion)
    ///
    /// # Arguments
    ///
    /// * `width` - Image width in pixels
    /// * `height` - Image height in pixels
    /// * `extrinsics` - Camera position and orientation in world coordinates
    /// * `intrinsics` - Camera intrinsic parameters including distortion model
    ///
    /// # Returns
    ///
    /// A new [`Camera`] instance, or [`MvgError`] if the parameters are invalid.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The projection matrix cannot be computed
    /// - The camera parameters are mathematically inconsistent
    /// - SVD decomposition fails during initialization
    ///
    /// # Example
    ///
    /// ```rust
    /// use braid_mvg::{Camera, extrinsics, make_default_intrinsics};
    ///
    /// let extrinsics = extrinsics::make_default_extrinsics::<f64>();
    /// let intrinsics = make_default_intrinsics::<f64>();
    /// let camera = Camera::new(640, 480, extrinsics, intrinsics)?;
    /// # Ok::<(), braid_mvg::MvgError>(())
    /// ```
    pub fn new(
        width: usize,
        height: usize,
        extrinsics: ExtrinsicParameters<R>,
        intrinsics: RosOpenCvIntrinsics<R>,
    ) -> Result<Self> {
        let inner = cam_geom::Camera::new(intrinsics, extrinsics);
        Self::new_from_cam_geom(width, height, inner)
    }

    /// Create a new camera from a cam-geom Camera instance.
    ///
    /// This constructor wraps an existing cam-geom Camera with additional
    /// image dimension information and caching for performance.
    ///
    /// # Arguments
    ///
    /// * `width` - Image width in pixels
    /// * `height` - Image height in pixels
    /// * `inner` - A pre-constructed cam-geom Camera instance
    ///
    /// # Returns
    ///
    /// A new [`Camera`] instance, or [`MvgError`] if the camera cannot be constructed.
    ///
    /// # Example
    ///
    /// ```rust
    /// use braid_mvg::{Camera, extrinsics, make_default_intrinsics};
    /// use cam_geom;
    ///
    /// let extrinsics = extrinsics::make_default_extrinsics::<f64>();
    /// let intrinsics = make_default_intrinsics::<f64>();
    /// let cam_geom_camera = cam_geom::Camera::new(intrinsics, extrinsics);
    /// let camera = Camera::new_from_cam_geom(640, 480, cam_geom_camera)?;
    /// # Ok::<(), braid_mvg::MvgError>(())
    /// ```
    pub fn new_from_cam_geom(
        width: usize,
        height: usize,
        inner: cam_geom::Camera<R, RosOpenCvIntrinsics<R>>,
    ) -> Result<Self> {
        let intrinsics = inner.intrinsics();
        let extrinsics = inner.extrinsics();
        let m = {
            let p33 = intrinsics.p.fixed_view::<3, 3>(0, 0);
            p33 * extrinsics.matrix()
        };

        // flip sign if focal length < 0
        let m = if m[(0, 0)] < na::convert(0.0) { -m } else { m };

        let m = m / m[(2, 3)]; // normalize

        let pinv = my_pinv(&m)?;
        let cache = CameraCache { m, pinv };
        Ok(Self {
            width,
            height,
            inner,
            cache,
        })
    }

    /// Create a camera from a 3×4 projection matrix.
    ///
    /// This method decomposes a camera projection matrix into intrinsic and extrinsic
    /// parameters using QR decomposition. It assumes no lens distortion (pinhole model).
    ///
    /// # Mathematical Background
    ///
    /// The projection matrix P has the form:
    /// ```text
    /// P = K[R|t]
    /// ```
    /// where K is the 3×3 intrinsic matrix and [R|t] is the 3×4 extrinsic matrix.
    ///
    /// # Arguments
    ///
    /// * `width` - Image width in pixels
    /// * `height` - Image height in pixels
    /// * `pmat` - 3×4 projection matrix
    ///
    /// # Returns
    ///
    /// A new [`Camera`] instance with no distortion, or [`MvgError`] if decomposition fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The projection matrix is singular or ill-conditioned
    /// - QR decomposition fails
    /// - The resulting parameters are invalid
    ///
    /// # Example
    ///
    /// ```rust
    /// use braid_mvg::Camera;
    /// use nalgebra::{OMatrix, U3, U4};
    ///
    /// // Create a simple projection matrix
    /// let pmat = OMatrix::<f64, U3, U4>::new(
    ///     1000.0, 0.0, 320.0, 100.0,
    ///     0.0, 1000.0, 240.0, 200.0,
    ///     0.0, 0.0, 1.0, 0.01
    /// );
    /// let camera = Camera::from_pmat(640, 480, &pmat)?;
    /// # Ok::<(), braid_mvg::MvgError>(())
    /// ```
    pub fn from_pmat(width: usize, height: usize, pmat: &OMatrix<R, U3, U4>) -> Result<Self> {
        let distortion = Distortion::zero();
        Self::from_pmat_with_distortion(width, height, pmat, distortion)
    }

    fn from_pmat_with_distortion(
        width: usize,
        height: usize,
        pmat: &OMatrix<R, U3, U4>,
        distortion: Distortion<R>,
    ) -> Result<Self> {
        let m = (*pmat).remove_column(3);
        let (rquat, k) = rq_decomposition(m)?;

        let k22: R = k[(2, 2)];

        let one: R = One::one();

        let k = k * (one / k22); // normalize
        let fx = k[(0, 0)];
        let skew = k[(0, 1)];
        let fy = k[(1, 1)];
        let cx = k[(0, 2)];
        let cy = k[(1, 2)];

        let intrinsics =
            RosOpenCvIntrinsics::from_params_with_distortion(fx, skew, fy, cx, cy, distortion);
        let camcenter = pmat2cam_center(pmat);
        let extrinsics = ExtrinsicParameters::from_rotation_and_camcenter(rquat, camcenter);

        Camera::new(width, height, extrinsics, intrinsics)
    }

    /// convert, if possible, into a 3x4 matrix
    pub fn as_pmat(&self) -> Option<&OMatrix<R, U3, U4>> {
        let d = &self.intrinsics().distortion;
        if d.is_linear() {
            Some(&self.cache.m)
        } else {
            None
        }
    }

    /// Get the linear projection matrix (3×4) for this camera.
    ///
    /// This returns the cached projection matrix that represents a linearized
    /// version of the camera model (without lens distortion). The matrix has
    /// the form P = K[R|t] where K is the intrinsic matrix and [R|t] are
    /// the extrinsic parameters.
    ///
    /// # Returns
    ///
    /// Reference to the 3×4 projection matrix
    ///
    /// # Example
    ///
    /// ```rust
    /// use braid_mvg::{Camera, extrinsics, make_default_intrinsics};
    ///
    /// let camera = Camera::new(640, 480,
    ///     extrinsics::make_default_extrinsics::<f64>(),
    ///     make_default_intrinsics::<f64>())?;
    /// let pmat = camera.linear_part_as_pmat();
    /// println!("Projection matrix shape: {}×{}", pmat.nrows(), pmat.ncols());
    /// # Ok::<(), braid_mvg::MvgError>(())
    /// ```
    pub fn linear_part_as_pmat(&self) -> &OMatrix<R, U3, U4> {
        &self.cache.m
    }

    /// Return a linearized copy of self.
    ///
    /// The returned camera will not have distortion. In other words, the raw
    /// projected ("distorted") pixels are identical with the "undistorted"
    /// variant. The camera model is a perfect linear pinhole.
    pub fn linearize_to_cam_geom(
        &self,
    ) -> cam_geom::Camera<R, cam_geom::IntrinsicParametersPerspective<R>> {
        let fx = self.intrinsics().k[(0, 0)];
        let skew = self.intrinsics().k[(0, 1)];
        let fy = self.intrinsics().k[(1, 1)];
        let cx = self.intrinsics().k[(0, 2)];
        let cy = self.intrinsics().k[(1, 2)];

        let intrinsics =
            cam_geom::IntrinsicParametersPerspective::from(cam_geom::PerspectiveParams {
                fx,
                fy,
                skew,
                cx,
                cy,
            });

        let pose = self.extrinsics().clone();
        cam_geom::Camera::new(intrinsics, pose)
    }

    /// Transform this camera using a similarity transformation.
    ///
    /// This method applies a similarity transformation (scale, rotation, translation)
    /// to align the camera coordinate system. This is commonly used in:
    /// - Multi-camera system calibration
    /// - Coordinate system alignment
    /// - Scale recovery in structure-from-motion
    ///
    /// # Mathematical Details
    ///
    /// The transformation applies: `X' = s*R*X + t` where:
    /// - `s` is the uniform scale factor
    /// - `R` is the 3×3 rotation matrix
    /// - `t` is the 3×1 translation vector
    /// - `X` are the original 3D points
    ///
    /// # Arguments
    ///
    /// * `s` - Uniform scale factor (positive)
    /// * `rot` - 3×3 rotation matrix (must be orthogonal with det=1)
    /// * `t` - 3×1 translation vector
    ///
    /// # Returns
    ///
    /// A new aligned [`Camera`] instance, or [`MvgError`] if transformation fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The rotation matrix is invalid (not orthogonal or det≠1)
    /// - The scale factor is non-positive
    /// - Camera reconstruction fails after transformation
    ///
    /// # Example
    ///
    /// ```rust
    /// use braid_mvg::{Camera, extrinsics, make_default_intrinsics};
    /// use nalgebra::{Matrix3, Vector3};
    ///
    /// let camera = Camera::new(640, 480,
    ///     extrinsics::make_default_extrinsics::<f64>(),
    ///     make_default_intrinsics::<f64>())?;
    ///
    /// let scale = 2.0;
    /// let rotation = Matrix3::identity();
    /// let translation = Vector3::new(1.0, 0.0, 0.0);
    ///
    /// let aligned_camera = camera.align(scale, rotation, translation)?;
    /// # Ok::<(), braid_mvg::MvgError>(())
    /// ```
    pub fn align(&self, s: R, rot: Matrix3<R>, t: Vector3<R>) -> Result<Self> {
        let m = build_xform(s, rot, t);
        let mi = my_pinv_4x4(&m)?;

        let pmat = &self.cache.m;
        let aligned_pmat = pmat * mi;

        Self::from_pmat_with_distortion(
            self.width,
            self.height,
            &aligned_pmat,
            self.intrinsics().distortion.clone(),
        )
    }

    /// return a copy of this camera looking in the opposite direction
    ///
    /// The returned camera has the same 3D->2D projection. (The 2D->3D
    /// projection results in a vector in the opposite direction.)
    pub fn flip(&self) -> Option<Camera<R>> {
        use crate::intrinsics::{mirror, MirrorAxis::LeftRight};
        if !self.intrinsics().rect.is_identity(na::convert(1.0e-7)) {
            return None;
        }

        let cc = self.extrinsics().camcenter();

        let lv = self.extrinsics().forward();
        let lv2 = -lv;
        let la2 = cc.coords + lv2.as_ref();

        let up = self.extrinsics().up();
        let up2 = -up;

        let extrinsics2 = crate::ExtrinsicParameters::from_view(&cc.coords, &la2, &up2);
        let mut intinsics2 = mirror(self.intrinsics(), LeftRight)?;

        intinsics2.p[(0, 1)] = -intinsics2.p[(0, 1)];
        intinsics2.k[(0, 1)] = -intinsics2.k[(0, 1)];

        let mut d = intinsics2.distortion.clone();
        *d.tangential2_mut() = -d.tangential2();

        Some(Camera::new(self.width(), self.height(), extrinsics2, intinsics2).unwrap())
    }

    /// Get the camera's intrinsic parameters.
    #[inline]
    pub fn intrinsics(&self) -> &RosOpenCvIntrinsics<R> {
        self.inner.intrinsics()
    }

    /// Get the camera's extrinsic parameters.
    #[inline]
    pub fn extrinsics(&self) -> &ExtrinsicParameters<R> {
        self.inner.extrinsics()
    }

    /// Convert this camera to PyMVG format.
    ///
    /// PyMVG is a Python library for multiple view geometry. This method converts
    /// the camera parameters to the PyMVG JSON schema format for interoperability.
    ///
    /// # Arguments
    ///
    /// * `name` - Name to assign to the camera in the PyMVG format
    ///
    /// # Returns
    ///
    /// A [`PymvgCamera`] struct containing the camera parameters in PyMVG format
    ///
    /// # Example
    ///
    /// ```rust
    /// use braid_mvg::{Camera, extrinsics, make_default_intrinsics};
    ///
    /// let camera = Camera::new(640, 480,
    ///     extrinsics::make_default_extrinsics::<f64>(),
    ///     make_default_intrinsics::<f64>())?;
    /// let pymvg_camera = camera.to_pymvg("camera1");
    /// # Ok::<(), braid_mvg::MvgError>(())
    /// ```
    pub fn to_pymvg(&self, name: &str) -> PymvgCamera<R> {
        let d = &self.intrinsics().distortion;
        let dvec = Vector5::new(
            d.radial1(),
            d.radial2(),
            d.tangential1(),
            d.tangential2(),
            d.radial3(),
        );
        PymvgCamera {
            name: name.to_string(),
            width: self.width,
            height: self.height,
            P: self.intrinsics().p,
            K: self.intrinsics().k,
            D: dvec,
            R: self.intrinsics().rect,
            Q: *self.extrinsics().rotation().matrix(),
            translation: *self.extrinsics().translation(),
        }
    }

    pub(crate) fn from_pymvg(cam: &PymvgCamera<R>) -> Result<(String, Self)> {
        let name = cam.name.clone();

        let rquat = right_handed_rotation_quat_new(&cam.Q)?;
        let extrinsics = crate::extrinsics::from_rquat_translation(rquat, cam.translation);
        let distortion = Distortion::from_opencv_vec(cam.D);
        let intrinsics = RosOpenCvIntrinsics::from_components(cam.P, cam.K, distortion, cam.R)?;
        let cam = Self::new(cam.width, cam.height, extrinsics, intrinsics)?;
        Ok((name, cam))
    }

    /// Get the image width in pixels.
    #[inline]
    pub fn width(&self) -> usize {
        self.width
    }

    /// Get the image height in pixels.
    #[inline]
    pub fn height(&self) -> usize {
        self.height
    }

    /// Project a 3D world point to undistorted 2D image coordinates.
    ///
    /// This method performs the core camera projection operation, transforming
    /// a 3D point in world coordinates to 2D pixel coordinates. The result
    /// represents the undistorted pixel coordinates (as if using a perfect
    /// pinhole camera model).
    pub fn project_3d_to_pixel(&self, pt3d: &PointWorldFrame<R>) -> UndistortedPixel<R> {
        let coords: Point3<R> = pt3d.coords;

        let cc = self.cache.m * coords.to_homogeneous();
        UndistortedPixel {
            coords: Point2::new(cc[0] / cc[2], cc[1] / cc[2]),
        }
    }

    /// Project a 3D world point to distorted 2D image coordinates.
    ///
    /// This method projects a 3D point to 2D image coordinates and then applies
    /// lens distortion to get the actual pixel coordinates as they would appear
    /// in the raw camera image.
    ///
    /// # Arguments
    ///
    /// * `pt3d` - 3D point in world coordinates
    ///
    /// # Returns
    ///
    /// [`DistortedPixel`] containing the 2D image coordinates with distortion applied
    ///
    /// # Example
    ///
    /// ```rust
    /// use braid_mvg::{Camera, PointWorldFrame, extrinsics, make_default_intrinsics};
    /// use nalgebra::Point3;
    ///
    /// let camera = Camera::new(640, 480,
    ///     extrinsics::make_default_extrinsics::<f64>(),
    ///     make_default_intrinsics::<f64>())?;
    /// let point_3d = PointWorldFrame { coords: Point3::new(0.0, 0.0, 5.0) };
    /// let distorted_pixel = camera.project_3d_to_distorted_pixel(&point_3d);
    /// # Ok::<(), braid_mvg::MvgError>(())
    /// ```
    pub fn project_3d_to_distorted_pixel(&self, pt3d: &PointWorldFrame<R>) -> DistortedPixel<R> {
        let undistorted = self.project_3d_to_pixel(pt3d);
        let ud = UndistortedPixels {
            data: OMatrix::<R, U1, U2>::new(undistorted.coords[0], undistorted.coords[1]),
        };
        self.intrinsics().distort(&ud).into()
    }

    /// Back-project a 2D undistorted pixel to a 3D point at a given distance.
    ///
    /// This method performs the inverse camera projection, taking a 2D pixel
    /// coordinate and a distance to compute the corresponding 3D world point.
    /// This is useful for depth-based reconstruction and ray casting.
    ///
    /// # Arguments
    ///
    /// * `pt2d` - 2D pixel coordinates (undistorted)
    /// * `dist` - Distance from camera center to the 3D point
    ///
    /// # Returns
    ///
    /// [`PointWorldFrame`] containing the 3D world coordinates
    ///
    /// # Example
    ///
    /// ```rust
    /// use braid_mvg::{Camera, UndistortedPixel, extrinsics, make_default_intrinsics};
    /// use nalgebra::Point2;
    ///
    /// let camera = Camera::new(640, 480,
    ///     extrinsics::make_default_extrinsics::<f64>(),
    ///     make_default_intrinsics::<f64>())?;
    /// let pixel = UndistortedPixel { coords: Point2::new(320.0, 240.0) };
    /// let point_3d = camera.project_pixel_to_3d_with_dist(&pixel, 5.0);
    /// # Ok::<(), braid_mvg::MvgError>(())
    /// ```
    pub fn project_pixel_to_3d_with_dist(
        &self,
        pt2d: &UndistortedPixel<R>,
        dist: R,
    ) -> PointWorldFrame<R>
    where
        DefaultAllocator: Allocator<U1, U2>,
        DefaultAllocator: Allocator<U1, U3>,
    {
        let ray_cam = self.intrinsics().undistorted_pixel_to_camera(&pt2d.into());
        let pt_cam = ray_cam.point_on_ray_at_distance(dist);
        self.extrinsics().camera_to_world(&pt_cam).into()
    }

    /// Back-project a 2D distorted pixel to a 3D point at a given distance.
    ///
    /// This method first removes lens distortion from the pixel coordinates,
    /// then back-projects to 3D space at the specified distance from the camera.
    ///
    /// # Arguments
    ///
    /// * `pt2d` - 2D pixel coordinates (with distortion)
    /// * `dist` - Distance from camera center to the 3D point
    ///
    /// # Returns
    ///
    /// [`PointWorldFrame`] containing the 3D world coordinates
    ///
    /// # Example
    ///
    /// ```rust
    /// use braid_mvg::{Camera, DistortedPixel, extrinsics, make_default_intrinsics};
    /// use nalgebra::Point2;
    ///
    /// let camera = Camera::new(640, 480,
    ///     extrinsics::make_default_extrinsics::<f64>(),
    ///     make_default_intrinsics::<f64>())?;
    /// let pixel = DistortedPixel { coords: Point2::new(320.0, 240.0) };
    /// let point_3d = camera.project_distorted_pixel_to_3d_with_dist(&pixel, 5.0);
    /// # Ok::<(), braid_mvg::MvgError>(())
    /// ```
    pub fn project_distorted_pixel_to_3d_with_dist(
        &self,
        pt2d: &DistortedPixel<R>,
        dist: R,
    ) -> PointWorldFrame<R> {
        use cam_geom::IntrinsicParameters;
        let ray_cam = self.intrinsics().pixel_to_camera(&pt2d.into());
        let pt_cam = ray_cam.point_on_ray_at_distance(dist);
        self.extrinsics().camera_to_world(&pt_cam).into()
    }
}

impl<R: RealField + Copy> std::default::Default for Camera<R> {
    fn default() -> Camera<R> {
        let extrinsics = crate::extrinsics::make_default_extrinsics();
        let intrinsics = crate::make_default_intrinsics();
        Camera::new(640, 480, extrinsics, intrinsics).unwrap()
    }
}

impl<R: RealField + Copy> std::fmt::Debug for Camera<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Camera")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("inner", &self.inner)
            .finish()
    }
}

impl<R: RealField + Copy> AsRef<cam_geom::Camera<R, RosOpenCvIntrinsics<R>>> for Camera<R> {
    #[inline]
    fn as_ref(&self) -> &cam_geom::Camera<R, RosOpenCvIntrinsics<R>> {
        &self.inner
    }
}

impl<R: RealField + serde::Serialize + Copy> serde::Serialize for Camera<R> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;

        // 5 is the number of fields we serialize from the struct.
        let mut state = serializer.serialize_struct("Camera", 5)?;
        state.serialize_field("width", &self.width)?;
        state.serialize_field("height", &self.height)?;
        state.serialize_field("extrinsics", &self.extrinsics())?;
        state.serialize_field("intrinsics", &self.intrinsics())?;
        state.end()
    }
}

impl<'de, R: RealField + serde::Deserialize<'de> + Copy> serde::Deserialize<'de> for Camera<R> {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de;
        use std::fmt;

        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "lowercase")]
        enum Field {
            Width,
            Height,
            Extrinsics,
            Intrinsics,
        }

        struct CameraVisitor<'de, R2: RealField + serde::Deserialize<'de>>(
            std::marker::PhantomData<&'de R2>,
        );

        impl<'de, R2: RealField + serde::Deserialize<'de> + Copy> serde::de::Visitor<'de>
            for CameraVisitor<'de, R2>
        {
            type Value = Camera<R2>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("struct Camera")
            }

            fn visit_seq<V>(self, mut seq: V) -> std::result::Result<Camera<R2>, V::Error>
            where
                V: serde::de::SeqAccess<'de>,
            {
                let width = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(0, &self))?;
                let height = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(1, &self))?;
                let extrinsics = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(0, &self))?;
                let intrinsics = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(1, &self))?;
                Camera::new(width, height, extrinsics, intrinsics)
                    .map_err(|e| de::Error::custom(format!("failed creating Camera: {e}")))
            }

            fn visit_map<V>(self, mut map: V) -> std::result::Result<Camera<R2>, V::Error>
            where
                V: serde::de::MapAccess<'de>,
            {
                let mut width = None;
                let mut height = None;
                let mut extrinsics = None;
                let mut intrinsics = None;
                while let Some(key) = map.next_key()? {
                    match key {
                        Field::Width => {
                            if width.is_some() {
                                return Err(de::Error::duplicate_field("width"));
                            }
                            width = Some(map.next_value()?);
                        }
                        Field::Height => {
                            if height.is_some() {
                                return Err(de::Error::duplicate_field("height"));
                            }
                            height = Some(map.next_value()?);
                        }
                        Field::Extrinsics => {
                            if extrinsics.is_some() {
                                return Err(de::Error::duplicate_field("extrinsics"));
                            }
                            extrinsics = Some(map.next_value()?);
                        }
                        Field::Intrinsics => {
                            if intrinsics.is_some() {
                                return Err(de::Error::duplicate_field("intrinsics"));
                            }
                            intrinsics = Some(map.next_value()?);
                        }
                    }
                }
                let width = width.ok_or_else(|| de::Error::missing_field("width"))?;
                let height = height.ok_or_else(|| de::Error::missing_field("height"))?;
                let extrinsics =
                    extrinsics.ok_or_else(|| de::Error::missing_field("extrinsics"))?;
                let intrinsics =
                    intrinsics.ok_or_else(|| de::Error::missing_field("intrinsics"))?;
                Camera::new(width, height, extrinsics, intrinsics)
                    .map_err(|e| de::Error::custom(format!("failed creating Camera: {e}")))
            }
        }

        const FIELDS: &[&str] = &["width", "height", "extrinsics", "intrinsics"];
        deserializer.deserialize_struct("Camera", FIELDS, CameraVisitor(std::marker::PhantomData))
    }
}

fn _test_camera_is_serialize() {
    // Compile-time test to ensure Camera implements Serialize trait.
    fn implements<T: serde::Serialize>() {}
    implements::<Camera<f64>>();
}

fn _test_camera_is_deserialize() {
    // Compile-time test to ensure Camera implements Deserialize trait.
    fn implements<'de, T: serde::Deserialize<'de>>() {}
    implements::<Camera<f64>>();
}

#[derive(Clone, PartialEq)]
pub(crate) struct CameraCache<R: RealField> {
    pub(crate) m: OMatrix<R, U3, U4>,
    pub(crate) pinv: OMatrix<R, U4, U3>,
}

const SVD_MAX_ITERATIONS: usize = 1_000_000;

fn my_pinv<R: RealField + Copy>(m: &OMatrix<R, U3, U4>) -> Result<OMatrix<R, U4, U3>> {
    na::linalg::SVD::try_new(*m, true, true, na::convert(1e-7), SVD_MAX_ITERATIONS)
        .ok_or(MvgError::SvdFailed)?
        .pseudo_inverse(na::convert(1.0e-7))
        .map_err(|e| MvgError::PinvError {
            error: format!("inverse failed {e}"),
        })
}

fn my_pinv_4x4<R: RealField + Copy>(m: &OMatrix<R, U4, U4>) -> Result<OMatrix<R, U4, U4>> {
    na::linalg::SVD::try_new(*m, true, true, na::convert(1e-7), SVD_MAX_ITERATIONS)
        .ok_or(MvgError::SvdFailed)?
        .pseudo_inverse(na::convert(1.0e-7))
        .map_err(|e| MvgError::PinvError {
            error: format!("inverse failed {e}"),
        })
}

fn build_xform<R: RealField + Copy>(s: R, rot: Matrix3<R>, t: Vector3<R>) -> Matrix4<R> {
    let mut m1 = Matrix4::zero();
    for i in 0..3 {
        for j in 0..3 {
            m1[(i, j)] = rot[(i, j)];
        }
    }
    let mut m2 = m1 * s;
    for i in 0..3 {
        m2[(i, 3)] = t[i];
    }
    m2[(3, 3)] = R::one();
    m2
}

#[allow(clippy::many_single_char_names)]
fn pmat2cam_center<R: RealField + Copy>(p: &OMatrix<R, U3, U4>) -> Point3<R> {
    let x = (*p).remove_column(0).determinant();
    let y = -(*p).remove_column(1).determinant();
    let z = (*p).remove_column(2).determinant();
    let w = -(*p).remove_column(3).determinant();
    Point3::from(Vector3::new(x / w, y / w, z / w))
}

/// Calculate angle of quaternion
///
/// This is the implementation from prior to
/// https://github.com/rustsim/nalgebra/commit/74aefd9c23dadd12ee654c7d0206b0a96d22040c
fn my_quat_angle<R: RealField + Copy>(quat: &na::UnitQuaternion<R>) -> R {
    let w = quat.quaternion().scalar().abs();

    // Handle inaccuracies that make break `.acos`.
    if w >= R::one() {
        R::zero()
    } else {
        w.acos() * na::convert(2.0f64)
    }
}

/// convert a 3x3 matrix into a valid right-handed rotation
fn right_handed_rotation_quat_new<R: RealField + Copy>(
    orig: &Matrix3<R>,
) -> Result<UnitQuaternion<R>> {
    let r1 = *orig;
    let rotmat = Rotation3::from_matrix_unchecked(r1);
    let rquat = UnitQuaternion::from_rotation_matrix(&rotmat);
    {
        // Check for valid rotation matrix by converting back to rotation
        // matrix and back again to quat then comparing quats. Probably
        // there is a much faster and better way.
        let rotmat2 = rquat.to_rotation_matrix();
        let rquat2 = UnitQuaternion::from_rotation_matrix(&rotmat2);
        let delta = rquat.rotation_to(&rquat2);
        let angle = my_quat_angle(&delta);
        let epsilon = na::convert(1.0e-7);
        if angle.abs() > epsilon {
            return Err(MvgError::InvalidRotationMatrix);
        }
    }
    Ok(rquat)
}

fn rq<R: RealField + Copy>(A: Matrix3<R>) -> (Matrix3<R>, Matrix3<R>) {
    let zero: R = Zero::zero();
    let one: R = One::one();

    // see https://math.stackexchange.com/a/1640762
    let P = Matrix3::<R>::new(zero, zero, one, zero, one, zero, one, zero, zero);
    let Atilde = P * A;

    let (Qtilde, Rtilde) = {
        let qrm = na::linalg::QR::new(Atilde.transpose());
        (qrm.q(), qrm.r())
    };
    let Q = P * Qtilde.transpose();
    let R = P * Rtilde.transpose() * P;
    (R, Q)
}

/// perform RQ decomposition and return results as right-handed quaternion and intrinsics matrix
pub fn rq_decomposition<R: RealField + Copy>(
    orig: Matrix3<R>,
) -> Result<(UnitQuaternion<R>, Matrix3<R>)> {
    let (mut intrin, mut q) = rq(orig);
    let zero: R = Zero::zero();
    for i in 0..3 {
        if intrin[(i, i)] < zero {
            for j in 0..3 {
                intrin[(j, i)] = -intrin[(j, i)];
                q[(i, j)] = -q[(i, j)];
            }
        }
    }

    match right_handed_rotation_quat_new(&q) {
        Ok(rquat) => Ok((rquat, intrin)),
        Err(error) => {
            match error {
                MvgError::InvalidRotationMatrix => {
                    // convert left-handed rotation to right-handed rotation
                    let q = -q;
                    let intrin = -intrin;
                    let rquat = right_handed_rotation_quat_new(&q)?;
                    Ok((rquat, intrin))
                }
                e => Err(e),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{DistortedPixel, PointWorldFrame};
    use na::core::dimension::{U3, U4};
    use na::core::{OMatrix, Vector4};
    use na::geometry::{Point2, Point3};
    use nalgebra as na;

    fn is_pmat_same(cam: &crate::Camera<f64>, pmat: &OMatrix<f64, U3, U4>) -> bool {
        let world_pts = [
            PointWorldFrame {
                coords: Point3::new(1.23, 4.56, 7.89),
            },
            PointWorldFrame {
                coords: Point3::new(1.0, 2.0, 3.0),
            },
        ];

        let pts1: Vec<DistortedPixel<_>> = world_pts
            .iter()
            .map(|world| cam.project_3d_to_distorted_pixel(world))
            .collect();

        let pts2: Vec<DistortedPixel<_>> = world_pts
            .iter()
            .map(|world| {
                let world_h = Vector4::new(world.coords.x, world.coords.y, world.coords.z, 1.0);
                let rst = pmat * world_h;
                DistortedPixel {
                    coords: Point2::new(rst[0] / rst[2], rst[1] / rst[2]),
                }
            })
            .collect();

        let epsilon = 1e-10;

        for (im1, im2) in pts1.iter().zip(pts2) {
            println!("im1: {im1:?}");
            println!("im2: {im2:?}");
            let diff = im1.coords - im2.coords;
            let dist_squared = diff.dot(&diff);
            if dist_squared.is_nan() {
                continue;
            }
            println!("dist_squared: {dist_squared:?}");
            if dist_squared > epsilon {
                return false;
            }
        }
        true
    }

    fn is_similar(cam1: &crate::Camera<f64>, cam2: &crate::Camera<f64>) -> bool {
        let world_pts = [
            PointWorldFrame {
                coords: Point3::new(1.23, 4.56, 7.89),
            },
            PointWorldFrame {
                coords: Point3::new(1.0, 2.0, 3.0),
            },
        ];

        let pts1: Vec<DistortedPixel<_>> = world_pts
            .iter()
            .map(|world| cam1.project_3d_to_distorted_pixel(world))
            .collect();

        let pts2: Vec<DistortedPixel<_>> = world_pts
            .iter()
            .map(|world| cam2.project_3d_to_distorted_pixel(world))
            .collect();

        let epsilon = 1e-10;

        for (im1, im2) in pts1.iter().zip(pts2) {
            let diff = im1.coords - im2.coords;
            let dist_squared = diff.dot(&diff);
            if dist_squared.is_nan() {
                continue;
            }
            if dist_squared > epsilon {
                return false;
            }
        }
        true
    }

    #[test]
    fn test_to_from_pmat() {
        for (name, cam1) in crate::tests::get_test_cameras().iter() {
            println!("\n\n\ntesting camera {name}");
            let pmat = match cam1.as_pmat() {
                Some(pmat) => pmat,
                None => {
                    println!("skipping camera {name}: no pmat");
                    continue;
                }
            };
            assert!(is_pmat_same(cam1, pmat));
            let cam2 = crate::Camera::from_pmat(cam1.width(), cam1.height(), pmat).unwrap();
            assert!(is_similar(cam1, &cam2));
        }
    }

    #[test]
    fn test_flipped_camera() {
        for (name, cam1) in crate::tests::get_test_cameras().iter() {
            println!("testing camera {name}");
            let cam2 = cam1.flip().expect("flip cam");
            if !is_similar(cam1, &cam2) {
                panic!("results not similar for cam {name}");
            }
        }
    }

    #[test]
    fn test_rq() {
        let a = na::Matrix3::new(1.2, 3.4, 5.6, 7.8, 9.8, 7.6, 5.4, 3.2, 1.0);
        let (r, q) = crate::camera::rq(a);
        println!("r {r:?}");
        println!("q {q:?}");

        // check it is a real decomposition
        let a2 = r * q;
        println!("a {a:?}");
        println!("a2 {a2:?}");

        approx::assert_abs_diff_eq!(a, a2, epsilon = 1e-10);

        // check that q is orthonormal
        let actual = q * q.transpose();
        let expected = na::Matrix3::identity();
        approx::assert_abs_diff_eq!(actual, expected, epsilon = 1e-10);

        // check that r is upper triangular
        approx::assert_abs_diff_eq!(r[(1, 0)], 0.0, epsilon = 1e-10);
        approx::assert_abs_diff_eq!(r[(2, 0)], 0.0, epsilon = 1e-10);
        approx::assert_abs_diff_eq!(r[(2, 1)], 0.0, epsilon = 1e-10);
    }

    #[test]
    fn test_rotation_matrices_and_quaternions() {
        use na::geometry::{Rotation3, UnitQuaternion};

        #[rustfmt::skip]
        let r1 = na::Matrix3::from_column_slice(
            &[-0.9999999999999998, -0.00000000000000042632564145606005, -0.0000000000000002220446049250313,
            0.0000000000000004263256414560601, -1.0, 0.0,
            -0.0000000000000002220446049250313, -0.00000000000000000000000000000004930380657631324, -0.9999999999999998]);

        let rotmat = Rotation3::from_matrix_unchecked(r1);

        let rquat = UnitQuaternion::from_rotation_matrix(&rotmat);

        let rotmat2 = rquat.to_rotation_matrix();

        let rquat2 = UnitQuaternion::from_rotation_matrix(&rotmat2);

        let angle = rquat.angle_to(&rquat2);
        let delta = rquat.rotation_to(&rquat2);
        let my_angle = crate::camera::my_quat_angle(&delta);

        println!("r1 {r1:?}");
        println!("rotmat {rotmat:?}");
        println!("rquat {rquat:?}");
        println!("rotmat2 {rotmat2:?}");
        println!("rquat2 {rquat2:?}");
        println!("angle: {angle:?}");
        println!("delta {delta:?}");
        println!("my_angle: {my_angle:?}");

        let q = na::Quaternion::new(
            -0.000000000000000000000000000000002756166576353432,
            0.000000000000000024825341532472726,
            -0.00000000000000004766465574234759,
            0.5590169943749475,
        );
        let uq = UnitQuaternion::from_quaternion(q); // hmm, this conversion doesn't give me the delta from above :(
        println!("q: {q:?}");
        println!("uq: {uq:?}");
        println!("uq.angle(): {:?}", uq.angle());
    }

    #[test]
    fn test_serde() {
        let expected = crate::Camera::<f64>::default();
        let buf = serde_json::to_string(&expected).unwrap();
        let actual: crate::Camera<f64> = serde_json::from_str(&buf).unwrap();
        assert!(expected == actual);
    }
}
