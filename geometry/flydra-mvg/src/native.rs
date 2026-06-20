// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Native parametric multi-camera calibration format (TOML).
//!
//! This is a multi-camera container around [`braid_mvg::NativeCameraCalibration`]
//! plus optional system-level metadata (a comment and a water-surface refraction
//! parameter). It is the preferred on-disk calibration format because, unlike
//! flydra XML, it stores each camera's intrinsics exactly once, fully broken
//! down (see [`braid_mvg::native_cal`]).
//!
//! A [`FlydraMultiCameraSystem`] can be written to this format only if every
//! camera is representable (identity rectification matrix); otherwise
//! [`FlydraMultiCameraSystem::write_native_toml`] returns an error and the
//! caller should fall back to flydra XML.

use std::collections::BTreeMap;
use std::io::{Read, Write};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use nalgebra::RealField;

use braid_mvg::{Camera, MultiCameraSystem, NativeCameraCalibration};

use crate::{FlydraMultiCameraSystem, FlydraMvgError, Result};

/// Magic string identifying the native calibration format.
pub const NATIVE_CALIBRATION_FORMAT: &str = "braid-calibration";
/// Current native calibration format version.
pub const NATIVE_CALIBRATION_VERSION: u32 = 1;

/// A multi-camera calibration in the native parametric TOML format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NativeCalibration<R: RealField> {
    /// Format magic string; must equal [`NATIVE_CALIBRATION_FORMAT`].
    pub format: String,
    /// Format version; see [`NATIVE_CALIBRATION_VERSION`].
    pub version: u32,
    /// Optional free-form comment describing the calibration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    /// Optional refractive index of the medium below `z = 0` (water). When
    /// present, 3D reconstruction accounts for refraction at the `z = 0` plane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub water: Option<R>,
    /// The cameras, serialized as an array of `[[camera]]` tables.
    #[serde(rename = "camera", default)]
    pub cameras: Vec<NativeCameraCalibration<R>>,
}

impl<R> FlydraMultiCameraSystem<R>
where
    R: RealField + Copy + Default + serde::Serialize + DeserializeOwned,
{
    /// Returns `true` if every camera in the system is representable in the
    /// native parametric format (i.e. has an identity rectification matrix).
    pub fn is_native_representable(&self) -> bool {
        self.system()
            .cams_by_name()
            .values()
            .all(|cam| cam.is_native_representable())
    }

    /// Returns the names of cameras that cannot be represented in the native
    /// parametric format (those with a non-identity rectification matrix).
    pub fn non_representable_cameras(&self) -> Vec<String> {
        self.system()
            .cams_by_name()
            .iter()
            .filter(|(_, cam)| !cam.is_native_representable())
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// Convert this system to the native parametric calibration data structure.
    ///
    /// Returns an error if any camera is not representable; check first with
    /// [`Self::is_native_representable`].
    pub fn to_native_calibration(&self) -> Result<NativeCalibration<R>> {
        let cameras = self
            .system()
            .cams_by_name()
            .iter()
            .map(|(name, cam)| cam.to_native(name).map_err(FlydraMvgError::from))
            .collect::<Result<Vec<_>>>()?;

        Ok(NativeCalibration {
            format: NATIVE_CALIBRATION_FORMAT.to_string(),
            version: NATIVE_CALIBRATION_VERSION,
            comment: self.system().comment().cloned(),
            water: self.water(),
            cameras,
        })
    }

    /// Build a system from the native parametric calibration data structure.
    pub fn from_native_calibration(cal: &NativeCalibration<R>) -> Result<Self> {
        if cal.format != NATIVE_CALIBRATION_FORMAT {
            return Err(FlydraMvgError::FailedFlydraXmlConversion {
                msg: format!(
                    "unexpected calibration format {:?}, expected {:?}",
                    cal.format, NATIVE_CALIBRATION_FORMAT
                ),
            });
        }
        if cal.version != NATIVE_CALIBRATION_VERSION {
            return Err(FlydraMvgError::FailedFlydraXmlConversion {
                msg: format!(
                    "unsupported native calibration version {}, expected {}",
                    cal.version, NATIVE_CALIBRATION_VERSION
                ),
            });
        }

        let mut cams = BTreeMap::new();
        for nc in cal.cameras.iter() {
            let (name, cam) = Camera::from_native(nc)?;
            cams.insert(name, cam);
        }
        let system = match &cal.comment {
            Some(comment) => MultiCameraSystem::new_with_comment(cams, comment.clone()),
            None => MultiCameraSystem::new(cams),
        };
        Ok(FlydraMultiCameraSystem::from_system(system, cal.water))
    }

    /// Write this system as native parametric TOML.
    ///
    /// Returns an error if any camera is not representable in the native format.
    pub fn write_native_toml<W: Write>(&self, mut writer: W) -> Result<()> {
        let native = self.to_native_calibration()?;
        let buf = toml::to_string_pretty(&native).map_err(|e| {
            FlydraMvgError::FailedFlydraXmlConversion {
                msg: format!("serializing native TOML calibration: {e}"),
            }
        })?;
        writer.write_all(buf.as_bytes())?;
        Ok(())
    }

    /// Emit a loud [`tracing::warn!`] explaining that this calibration uses the
    /// legacy "dual-copy" intrinsics representation (non-identity rectification
    /// matrix) and therefore cannot be written in the native parametric format.
    ///
    /// Use this wherever a calibration is being persisted but cannot be
    /// represented natively, so that users get consistent advice about how to
    /// fix the underlying problem.
    pub fn warn_dual_copy_intrinsics(&self) {
        tracing::warn!(
            "Camera calibration for {:?} uses the legacy \"dual-copy\" intrinsics \
             representation: a non-identity rectification matrix is used to \
             reconcile independent linear (projection-matrix) and distortion-model \
             intrinsics, as produced by MultiCamSelfCal (MCSC). This calibration \
             cannot be expressed in the native parametric format and has been \
             written as legacy flydra XML. Writing such calibrations will NOT be \
             supported in the future. To fix this, regenerate the calibration so \
             that each camera's linear intrinsics match its distortion-model \
             intrinsics (fx, fy, cx, cy) — i.e. so the rectification matrix is the \
             identity.",
            self.non_representable_cameras(),
        );
    }

    /// Write calibration files for inclusion in a braidz recording directory.
    ///
    /// Always writes legacy flydra XML to `xml_path` (for backward
    /// compatibility), and additionally writes native parametric TOML to
    /// `toml_path` when every camera is representable. Otherwise emits a loud
    /// warning (see [`Self::warn_dual_copy_intrinsics`]) and writes only the XML.
    pub fn write_calibration_files(
        &self,
        xml_path: &std::path::Path,
        toml_path: &std::path::Path,
    ) -> Result<()> {
        self.to_flydra_xml(std::fs::File::create(xml_path)?)?;
        if self.is_native_representable() {
            self.write_native_toml(std::fs::File::create(toml_path)?)?;
        } else {
            self.warn_dual_copy_intrinsics();
        }
        Ok(())
    }

    /// Read a system from native parametric TOML.
    pub fn from_native_toml<Rd: Read>(mut reader: Rd) -> Result<Self> {
        let mut buf = String::new();
        reader.read_to_string(&mut buf)?;
        let native: NativeCalibration<R> =
            toml::from_str(&buf).map_err(|e| FlydraMvgError::FailedFlydraXmlConversion {
                msg: format!("parsing native TOML calibration: {e}"),
            })?;
        Self::from_native_calibration(&native)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use braid_mvg::{Camera, extrinsics::make_default_extrinsics, make_default_intrinsics};

    /// Round-trip a multi-camera system (with water) through the native TOML
    /// format.
    #[test]
    fn native_toml_roundtrip() {
        let mut cams = BTreeMap::new();
        cams.insert(
            "cam1".to_string(),
            Camera::new(
                640,
                480,
                make_default_extrinsics(),
                make_default_intrinsics(),
            )
            .unwrap(),
        );
        cams.insert(
            "cam2".to_string(),
            Camera::new(
                512,
                512,
                make_default_extrinsics(),
                make_default_intrinsics(),
            )
            .unwrap(),
        );
        let system = MultiCameraSystem::new(cams);
        let orig = FlydraMultiCameraSystem::from_system(system, Some(1.333));
        assert!(orig.is_native_representable());
        assert!(orig.non_representable_cameras().is_empty());

        let mut buf: Vec<u8> = Vec::new();
        orig.write_native_toml(&mut buf).unwrap();

        let loaded = FlydraMultiCameraSystem::<f64>::from_native_toml(buf.as_slice()).unwrap();

        assert_eq!(orig.len(), loaded.len());
        assert_eq!(orig.water(), loaded.water());
        for (name, cam_orig) in orig.system().cams_by_name() {
            let cam_loaded = loaded.system().cam_by_name(name).unwrap();
            approx::assert_relative_eq!(
                cam_orig.intrinsics().k,
                cam_loaded.intrinsics().k,
                epsilon = 1e-9
            );
            approx::assert_relative_eq!(
                cam_orig.extrinsics().matrix(),
                cam_loaded.extrinsics().matrix(),
                epsilon = 1e-9
            );
        }
    }
}
