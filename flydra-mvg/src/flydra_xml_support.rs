use nalgebra as na;
use nalgebra::core::dimension::{U3, U4};
use nalgebra::core::OMatrix;
use nalgebra::RealField;

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename = "multi_camera_reconstructor", deny_unknown_fields)]
pub struct FlydraReconstructor<R: RealField + serde::Serialize> {
    #[serde(rename = "single_camera_calibration")]
    pub cameras: Vec<SingleCameraCalibration<R>>,
    /// This is ignored when reading and not written.
    #[serde(default)]
    pub minimum_eccentricity: R,
    #[serde(default)]
    pub water: Option<R>,
    #[serde(default)]
    pub comment: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, rename = "single_camera_calibration")]
pub struct SingleCameraCalibration<R: RealField + serde::Serialize> {
    // changes to this should update BraidMetadataSchemaTag
    pub cam_id: String,
    #[serde(
        serialize_with = "serialize_matrix",
        deserialize_with = "deserialize_matrix"
    )]
    pub calibration_matrix: OMatrix<R, U3, U4>,
    #[serde(
        serialize_with = "serialize_two_ints",
        deserialize_with = "deserialize_two_ints"
    )]
    pub resolution: (usize, usize),
    /// Only values of None or Some(1.0) are supported.
    #[serde(default, skip_serializing)]
    pub scale_factor: Option<R>,
    #[serde(serialize_with = "serialize_non_linear_parameters")]
    pub non_linear_parameters: FlydraDistortionModel<R>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FlydraDistortionModel<R: RealField + serde::Serialize> {
    pub fc1: R,
    pub fc2: R,
    pub cc1: R,
    pub cc2: R,
    pub k1: R,
    pub k2: R,
    pub p1: R,
    pub p2: R,
    pub alpha_c: R,
    #[serde(default, skip_serializing)]
    pub fc1p: Option<R>,
    #[serde(default, skip_serializing)]
    pub fc2p: Option<R>,
    #[serde(default, skip_serializing)]
    pub cc1p: Option<R>,
    #[serde(default, skip_serializing)]
    pub cc2p: Option<R>,
}

pub(crate) fn serialize_recon<R>(
    recon: &FlydraReconstructor<R>,
) -> std::result::Result<String, serde_xml_rs::Error>
where
    R: RealField + Serialize,
{
    // this is a total hack. TODO make it not a hack

    // changes to this should update BraidMetadataSchemaTag

    // TODO: indent nicely within each camera.

    let v: Result<Vec<String>, serde_xml_rs::Error> =
        recon.cameras.iter().map(serde_xml_rs::to_string).collect();
    let v: Vec<String> = v?;
    let v_indented: Vec<String> = v.iter().map(|s| format!("    {}", s)).collect();
    let cams_buf = v_indented.join("\n");

    let mut v = vec!["<multi_camera_reconstructor>".to_string()];
    v.push(cams_buf);
    if let Some(ref w) = recon.water {
        v.push(format!("    <water>{}</water>", w));
    }
    if let Some(ref c) = recon.comment {
        v.push(format!("    <comment>{}</comment>", c));
    }
    v.push("</multi_camera_reconstructor>\n".to_string());
    let buf = v.join("\n");
    Ok(buf)
}

fn serialize_non_linear_parameters<S, R>(
    m: &FlydraDistortionModel<R>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
    R: RealField + Serialize,
{
    let buf = serde_xml_rs::to_string(m).expect("serialize FlydraDistortionModel");

    let strip_start = "<FlydraDistortionModel>";
    let strip_end = "</FlydraDistortionModel>";
    assert!(buf.starts_with(strip_start));
    assert!(buf.ends_with(strip_end));
    let bufptr = &buf[0..(buf.len() - strip_end.len())];
    let bufptr = &bufptr[strip_start.len()..];
    serializer.serialize_str(bufptr)
}

#[rustfmt::skip]
fn serialize_matrix<S, R>(m: &OMatrix<R,U3,U4>, serializer: S) -> Result<S::Ok, S::Error>
    where S: serde::Serializer,
         R: RealField + Serialize,
{
    let buf = format!("{} {} {} {}; {} {} {} {}; {} {} {} {}",
        m[(0,0)], m[(0,1)], m[(0,2)], m[(0,3)],
        m[(1,0)], m[(1,1)], m[(1,2)], m[(1,3)],
        m[(2,0)], m[(2,1)], m[(2,2)], m[(2,3)]);
    serializer.serialize_str(&buf)
}

fn deserialize_matrix<'de, D, R>(deserializer: D) -> Result<OMatrix<R, U3, U4>, D::Error>
where
    D: serde::Deserializer<'de>,
    R: RealField,
{
    use std::str::FromStr;

    let s = String::deserialize(deserializer)?;
    let rows: Vec<&str> = s.split(';').collect();
    if rows.len() != 3 {
        return Err(serde::de::Error::custom("expected exactly 3 rows"));
    }
    let mut elements: Vec<R> = Vec::new();
    for row in rows.iter() {
        let cols: Vec<&str> = row.split_whitespace().collect();
        if cols.len() != 4 {
            return Err(serde::de::Error::custom("expected exactly 4 columns"));
        }
        for col in cols.iter() {
            let element = f64::from_str(col).map_err(serde::de::Error::custom)?;
            elements.push(na::convert(element));
        }
    }
    Ok(OMatrix::<R, U3, U4>::from_row_slice(elements.as_slice()))
}

fn serialize_two_ints<S>(two_ints: &(usize, usize), serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let buf = format!("{} {}", two_ints.0, two_ints.1);
    serializer.serialize_str(&buf)
}

fn deserialize_two_ints<'de, D>(deserializer: D) -> Result<(usize, usize), D::Error>
where
    D: serde::Deserializer<'de>,
{
    use std::str::FromStr;

    let s = String::deserialize(deserializer)?;
    let nums: Vec<&str> = s.split(' ').collect();
    if nums.len() != 2 {
        return Err(serde::de::Error::custom("expected exactly 2 numbers"));
    }
    Ok((
        usize::from_str(nums[0]).map_err(serde::de::Error::custom)?,
        usize::from_str(nums[1]).map_err(serde::de::Error::custom)?,
    ))
}
