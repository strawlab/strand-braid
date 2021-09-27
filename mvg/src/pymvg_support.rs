#![allow(non_snake_case)]

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use nalgebra::allocator::Allocator;
use nalgebra::core::dimension::{U3, U4};
use nalgebra::core::{Matrix3, OMatrix, Vector5};
use nalgebra::dimension::DimName;
use nalgebra::geometry::Point3;
use nalgebra::DefaultAllocator;
use nalgebra::RealField;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PymvgMultiCameraSystemV1<R: RealField> {
    pub(crate) __pymvg_file_version__: String,
    pub(crate) camera_system: Vec<PymvgCamera<R>>,
}

// Serialize is not (yet) implemented.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PymvgCamera<R: RealField> {
    pub(crate) name: String,
    pub(crate) width: usize,
    pub(crate) height: usize,
    #[serde(with = "array_of_arrays")]
    pub(crate) P: OMatrix<R, U3, U4>,
    #[serde(with = "array_of_arrays")]
    pub(crate) K: Matrix3<R>,
    pub(crate) D: Vector5<R>,
    #[serde(with = "array_of_arrays")]
    pub(crate) R: Matrix3<R>,
    #[serde(with = "array_of_arrays")]
    pub(crate) Q: Matrix3<R>,
    pub(crate) translation: Point3<R>,
}

pub mod array_of_arrays {
    use super::*;

    /// Serialize an nalgebra::OMatrix to an array of arrays of floats
    ///
    /// The nalgebra serialization does not work exactly like this, so here we
    /// roll our own.
    pub fn serialize<S, R, ROWS, COLS>(
        arr: &OMatrix<R, ROWS, COLS>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        R: RealField,
        S: Serializer,
        DefaultAllocator: Allocator<R, ROWS, COLS>,
        ROWS: DimName,
        COLS: DimName,
    {
        use serde::ser::SerializeSeq;

        let nrows = arr.nrows();
        let mut outer_seq = serializer.serialize_seq(Some(nrows))?;
        for row in arr.row_iter() {
            let inner_seq: Vec<f64> = row
                .iter()
                .map(|el| nalgebra::try_convert(el.clone()).unwrap())
                .collect();
            outer_seq.serialize_element(&inner_seq)?;
        }
        outer_seq.end()
    }

    /// Deserialize an array of arrays of floats to nalgebra::OMatrix
    ///
    /// The nalgebra deserialization does not work exactly like this, so here we
    /// roll our own.
    pub fn deserialize<'de, D, R: RealField, ROWS, COLS>(
        deserializer: D,
    ) -> Result<OMatrix<R, ROWS, COLS>, D::Error>
    where
        D: Deserializer<'de>,
        DefaultAllocator: Allocator<R, ROWS, COLS>,
        ROWS: DimName,
        COLS: DimName,
    {
        // deserialize to JSON value and then extract the array.
        let v = serde_json::Value::deserialize(deserializer)?;
        let rows = v
            .as_array()
            .ok_or_else(|| serde::de::Error::custom("expected array"))?;

        if rows.len() != ROWS::USIZE {
            return Err(serde::de::Error::custom(format!(
                "expected {} rows, found {}",
                ROWS::USIZE,
                rows.len()
            )));
        }

        let mut values = Vec::<R>::with_capacity(3 * COLS::dim());
        for (i, row_value) in rows.iter().enumerate() {
            let row = row_value
                .as_array()
                .ok_or_else(|| serde::de::Error::custom("expected array"))?;

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
                    .ok_or_else(|| serde::de::Error::custom("expected float"))?;
                values.push(nalgebra::convert(el));
            }
        }

        Ok(nalgebra::OMatrix::<R, ROWS, COLS>::from_row_slice(&values))
    }
}

#[test]
fn matrix3x4_roundtrip() {
    #[derive(Debug, Serialize, Deserialize)]
    pub struct Outer<R: RealField> {
        #[serde(with = "array_of_arrays")]
        pub(crate) inner: OMatrix<R, U3, U4>,
    }

    let orig: Outer<f64> = Outer {
        inner: OMatrix::<f64, U3, U4>::from_row_slice(&[
            1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0,
        ]),
    };

    let buf = serde_json::to_vec(&orig).unwrap();
    println!("buf: {}", std::str::from_utf8(&buf).unwrap());
    let loaded: Outer<f64> = serde_json::from_slice(&buf).unwrap();

    approx::assert_abs_diff_eq!(orig.inner, loaded.inner, epsilon = 1e-32);
}

#[cfg(test)]
use nalgebra::U1;

#[test]
fn matrix4x1_roundtrip() {
    #[derive(Debug, Serialize, Deserialize)]
    pub struct Outer<R: RealField> {
        #[serde(with = "array_of_arrays")]
        pub(crate) inner: OMatrix<R, U4, U1>,
    }

    let orig: Outer<f64> = Outer {
        inner: OMatrix::<f64, U4, U1>::from_row_slice(&[1.0, 2.0, 3.0, 4.0]),
    };

    let buf = serde_json::to_vec(&orig).unwrap();
    println!("buf: {}", std::str::from_utf8(&buf).unwrap());
    let loaded: Outer<f64> = serde_json::from_slice(&buf).unwrap();

    approx::assert_abs_diff_eq!(orig.inner, loaded.inner, epsilon = 1e-32);
}
