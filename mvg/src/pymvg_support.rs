#![allow(non_snake_case)]

use serde::{Deserialize, Deserializer};

use nalgebra::allocator::Allocator;
use nalgebra::core::dimension::{U3, U4};
use nalgebra::core::{Matrix3, OMatrix, Vector5};
use nalgebra::dimension::DimName;
use nalgebra::geometry::Point3;
use nalgebra::DefaultAllocator;
use nalgebra::RealField;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PymvgMultiCameraSystemV1<R: RealField> {
    pub(crate) __pymvg_file_version__: String,
    pub(crate) camera_system: Vec<PymvgCamera<R>>,
}

// Serialize is not (yet) implemented.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PymvgCamera<R: RealField> {
    pub(crate) name: String,
    pub(crate) width: usize,
    pub(crate) height: usize,
    #[serde(deserialize_with = "deserialize_3xN")]
    pub(crate) P: OMatrix<R, U3, U4>,
    #[serde(deserialize_with = "deserialize_3xN")]
    pub(crate) K: Matrix3<R>,
    pub(crate) D: Vector5<R>,
    #[serde(deserialize_with = "deserialize_3xN")]
    pub(crate) R: Matrix3<R>,
    #[serde(deserialize_with = "deserialize_3xN")]
    pub(crate) Q: Matrix3<R>,
    pub(crate) translation: Point3<R>,
}

/// Deserialize an array of arrays of floats to nalgebra::OMatrix
///
/// The nalgebra deserialization does not work exactly like this, so here we
/// roll our own.
fn deserialize_3xN<'de, D, R: RealField, COLS>(
    deserializer: D,
) -> Result<OMatrix<R, U3, COLS>, D::Error>
where
    D: Deserializer<'de>,
    DefaultAllocator: Allocator<R, U3, COLS>,
    COLS: DimName,
{
    // deserialize to JSON value and then extract the array.
    let v = serde_json::Value::deserialize(deserializer)?;
    let rows = v
        .as_array()
        .ok_or(serde::de::Error::custom("expected array"))?;

    if rows.len() != 3 {
        return Err(serde::de::Error::custom(format!(
            "expected 3 rows, found {}",
            rows.len()
        )));
    }

    let mut values = Vec::<R>::with_capacity(3 * COLS::dim());
    for (i, row_value) in rows.iter().enumerate() {
        let row = row_value
            .as_array()
            .ok_or(serde::de::Error::custom("expected array"))?;

        if row.len() != COLS::dim() {
            return Err(serde::de::Error::custom(format!(
                "in row {}, expected {} cols found {}",
                i,
                COLS::dim(),
                row.len()
            )));
        }

        for el_value in row {
            let el = el_value
                .as_f64()
                .ok_or(serde::de::Error::custom("expected float"))?;
            values.push(nalgebra::convert(el));
        }
    }

    Ok(nalgebra::OMatrix::<R, U3, COLS>::from_row_slice(&values))
}
