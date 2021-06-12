#![deny(rust_2018_idioms)]
#![cfg_attr(feature = "backtrace", feature(backtrace))]

use thiserror::Error;

use nalgebra as na;
use nalgebra::geometry::{Point2, Point3};
use nalgebra::{Dim, RealField, U1, U2, U3};

use cam_geom::ExtrinsicParameters;
use opencv_ros_camera::{Distortion, RosOpenCvIntrinsics};

#[derive(Error, Debug)]
pub enum MvgError {
    #[error("bad matrix size")]
    BadMatrixSize,
    #[error("unknown distortion model")]
    UnknownDistortionModel,
    #[error("rectification matrix not supported")]
    RectificationMatrixNotSupported,
    #[error("not enough points")]
    NotEnoughPoints,
    #[error("unknown camera")]
    UnknownCamera,
    #[error("SVD failed")]
    SvdFailed,
    #[error("invalid rotation matrix")]
    InvalidRotationMatrix,
    #[error("unsupported version")]
    UnsupportedVersion,
    #[error("invalid rect matrix")]
    InvalidRectMatrix,
    #[error("unsupported type")]
    UnsupportedType,
    #[error("multiple valid roots found")]
    MultipleValidRootsFound,
    #[error("no valid root found")]
    NoValidRootFound,
    #[error("not implemented operation in mvg")]
    NotImplemented,
    #[error("cannot convert to flydra xml")]
    CannotConvertToFlydraXml,
    #[error("IO error: {}", error)]
    Io {
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        error: std::io::Error,
    },
    #[error("serde_yaml error: {}", error)]
    SerdeYaml {
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        error: serde_yaml::Error,
    },
    #[error("serde_json error: {}", error)]
    SerdeJson {
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        error: serde_json::Error,
    },
    #[error("SvgError: {}", error)]
    SvgError { error: &'static str },
    #[error("PinvError: {}", error)]
    PinvError { error: String },
    #[error("cam_geom::Error: {}", error)]
    CamGeomError {
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        error: cam_geom::Error,
    },
    #[error("opencv_ros_camera::Error: {}", error)]
    OpencvRosError {
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        error: opencv_ros_camera::Error,
    },
}

#[derive(Debug)]
pub struct CubicRootArgs {
    pub p4: f64,
    pub p3: f64,
    pub p2: f64,
    pub p1: f64,
    pub p0: f64,
    pub maxval: f64,
    pub eps: f64,
}

pub type Result<M> = std::result::Result<M, MvgError>;

mod pymvg_support;

pub mod intrinsics;

pub mod extrinsics;

mod camera;
pub use crate::camera::{rq_decomposition, Camera};

mod multi_cam_system;
pub use crate::multi_cam_system::MultiCameraSystem;

#[derive(Debug, Clone)]
pub struct DistortedPixel<R: RealField> {
    pub coords: Point2<R>,
}

impl<R, IN> From<&cam_geom::Pixels<R, U1, IN>> for DistortedPixel<R>
where
    R: RealField,
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
    R: RealField,
    IN: nalgebra::storage::Storage<R, U1, U2>,
{
    fn from(orig: cam_geom::Pixels<R, U1, IN>) -> Self {
        let orig_ref = &orig;
        orig_ref.into()
    }
}

impl<R> Into<cam_geom::Pixels<R, U1, na::storage::Owned<R, U1, U2>>> for &DistortedPixel<R>
where
    R: RealField,
    na::DefaultAllocator: na::allocator::Allocator<R, U1, U2>,
{
    fn into(self) -> cam_geom::Pixels<R, U1, na::storage::Owned<R, U1, U2>> {
        cam_geom::Pixels {
            data: na::OMatrix::<R, U1, U2>::from_row_slice(&[self.coords[0], self.coords[1]]),
        }
    }
}

impl<R: RealField> DistortedPixel<R> {
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
pub struct UndistortedPixel<R: RealField> {
    pub coords: Point2<R>,
}

impl<R, IN> From<&opencv_ros_camera::UndistortedPixels<R, U1, IN>> for UndistortedPixel<R>
where
    R: RealField,
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
    R: RealField,
    IN: nalgebra::storage::Storage<R, U1, U2>,
{
    fn from(orig: opencv_ros_camera::UndistortedPixels<R, U1, IN>) -> Self {
        let orig_ref = &orig;
        orig_ref.into()
    }
}

impl<R> Into<opencv_ros_camera::UndistortedPixels<R, U1, na::storage::Owned<R, U1, U2>>>
    for &UndistortedPixel<R>
where
    R: RealField,
    na::DefaultAllocator: na::allocator::Allocator<R, U1, U2>,
{
    fn into(self) -> opencv_ros_camera::UndistortedPixels<R, U1, na::storage::Owned<R, U1, U2>> {
        opencv_ros_camera::UndistortedPixels {
            data: na::OMatrix::<R, U1, U2>::from_row_slice(&[self.coords[0], self.coords[1]]),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PointCameraFrame<R: RealField> {
    pub coords: Point3<R>,
}

impl<R, IN> From<&cam_geom::Points<cam_geom::coordinate_system::CameraFrame, R, U1, IN>>
    for PointCameraFrame<R>
where
    R: RealField,
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
    R: RealField,
    IN: nalgebra::storage::Storage<R, U1, U3>,
{
    fn from(orig: cam_geom::Points<cam_geom::coordinate_system::CameraFrame, R, U1, IN>) -> Self {
        let orig_ref = &orig;
        orig_ref.into()
    }
}

impl<R>
    Into<
        cam_geom::Points<
            cam_geom::coordinate_system::CameraFrame,
            R,
            U1,
            na::storage::Owned<R, U1, U3>,
        >,
    > for &PointCameraFrame<R>
where
    R: RealField,
    na::DefaultAllocator: na::allocator::Allocator<R, U1, U2>,
{
    fn into(
        self,
    ) -> cam_geom::Points<
        cam_geom::coordinate_system::CameraFrame,
        R,
        U1,
        na::storage::Owned<R, U1, U3>,
    > {
        cam_geom::Points::new(na::OMatrix::<R, U1, U3>::new(
            self.coords[0],
            self.coords[1],
            self.coords[2],
        ))
    }
}

#[derive(Debug, Clone)]
pub struct PointWorldFrame<R: RealField> {
    pub coords: Point3<R>,
}

impl<R, IN> From<&cam_geom::Points<cam_geom::coordinate_system::WorldFrame, R, U1, IN>>
    for PointWorldFrame<R>
where
    R: RealField,
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
    R: RealField,
    IN: nalgebra::storage::Storage<R, U1, U3>,
{
    fn from(orig: cam_geom::Points<cam_geom::coordinate_system::WorldFrame, R, U1, IN>) -> Self {
        let orig_ref = &orig;
        orig_ref.into()
    }
}

impl<R>
    Into<
        cam_geom::Points<
            cam_geom::coordinate_system::WorldFrame,
            R,
            U1,
            na::storage::Owned<R, U1, U3>,
        >,
    > for &PointWorldFrame<R>
where
    R: RealField,
    na::DefaultAllocator: na::allocator::Allocator<R, U1, U2>,
{
    fn into(
        self,
    ) -> cam_geom::Points<
        cam_geom::coordinate_system::WorldFrame,
        R,
        U1,
        na::storage::Owned<R, U1, U3>,
    > {
        cam_geom::Points::new(na::OMatrix::<R, U1, U3>::new(
            self.coords[0],
            self.coords[1],
            self.coords[2],
        ))
    }
}

pub fn vec_sum<R: RealField>(vec: &[R]) -> R {
    vec.iter().fold(na::convert(0.0), |acc, i| acc + *i)
}

#[derive(Debug, Clone)]
pub struct PointWorldFrameWithSumReprojError<R: RealField> {
    pub point: PointWorldFrame<R>,
    pub cum_reproj_dist: R,
    pub mean_reproj_dist: R,
    pub reproj_dists: Vec<R>,
}

impl<R: RealField> PointWorldFrameWithSumReprojError<R> {
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
pub enum PointWorldFrameMaybeWithSumReprojError<R: RealField> {
    Point(PointWorldFrame<R>),
    WithSumReprojError(PointWorldFrameWithSumReprojError<R>),
}

impl<R: RealField> PointWorldFrameMaybeWithSumReprojError<R> {
    pub fn point(self) -> PointWorldFrame<R> {
        use crate::PointWorldFrameMaybeWithSumReprojError::*;
        match self {
            Point(pt) => pt,
            WithSumReprojError(pto) => pto.point,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WorldCoordAndUndistorted2D<R: RealField> {
    wc: PointWorldFrameMaybeWithSumReprojError<R>,
    upoints: Vec<(String, UndistortedPixel<R>)>,
}

impl<R: RealField> WorldCoordAndUndistorted2D<R> {
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

pub fn make_default_intrinsics<R: RealField>() -> RosOpenCvIntrinsics<R> {
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
                result.push((format!("dist-{}_skew{}", name, skew), cam));
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
            let name = format!("cam-{}", int_name);
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
