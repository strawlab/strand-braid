#![allow(non_snake_case)]

use serde::Deserialize;

use nalgebra::core::dimension::{U3, U4};
use nalgebra::core::{Matrix3, MatrixMN, Vector5};
use nalgebra::geometry::Point3;
use nalgebra::RealField;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PymvgMultiCameraSystemV1<R: RealField> {
    pub(crate) __pymvg_file_version__: String,
    pub(crate) camera_system: Vec<PymvgCamera<R>>,
}

// Serialize is not implemented because the default Matrix serializer does not
// behave exactly like pymvg.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PymvgCamera<R: RealField> {
    pub(crate) name: String,
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) P: MatrixMN<R, U3, U4>,
    pub(crate) K: Matrix3<R>,
    pub(crate) D: Vector5<R>,
    pub(crate) R: Matrix3<R>,
    pub(crate) Q: Matrix3<R>,
    pub(crate) translation: Point3<R>,
}
