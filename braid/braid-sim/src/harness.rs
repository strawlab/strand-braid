// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Generate the on-disk artifacts a full `braid-run` needs to run a scenario:
//! the synthetic calibration (flydra XML) and a Braid configuration TOML wiring
//! up the `sim` camera backend.

use std::path::{Path, PathBuf};

use braid_config_data::{BraidConfig, MainbrainConfig};
use braid_types::{BraidCameraConfig, FakeSyncConfig, StartCameraBackend, TriggerType};

use crate::Scenario;
use crate::calibration::{build_calibration, to_flydra_xml_string};

/// Paths to the artifacts generated for a run.
#[derive(Debug, Clone)]
pub struct GeneratedRun {
    /// The Braid configuration TOML (pass to `braid run`).
    pub config_path: PathBuf,
    /// The synthetic calibration (flydra XML), referenced by the config.
    pub calibration_path: PathBuf,
    /// The directory into which Braid writes `.braidz` files.
    pub braidz_output_dir: PathBuf,
}

/// Build the in-memory Braid configuration for a scenario: one `sim`-backed
/// camera per scenario camera, FakeSync at the scenario frame rate, and the
/// given calibration / output directory / control-API address.
pub fn build_braid_config(
    scenario: &Scenario,
    calibration_path: &Path,
    braidz_output_dir: &Path,
    http_api_server_addr: &str,
) -> BraidConfig {
    let mainbrain = MainbrainConfig {
        cal_fname: Some(calibration_path.to_path_buf()),
        output_base_dirname: braidz_output_dir.to_path_buf(),
        http_api_server_addr: http_api_server_addr.to_string(),
        ..Default::default()
    };

    let trigger = TriggerType::FakeSync(FakeSyncConfig {
        framerate: scenario.fps,
    });

    let cameras = (0..scenario.cameras.count)
        .map(|k| {
            let mut cam = BraidCameraConfig::default_absdiff_config(Scenario::camera_name(k));
            cam.start_backend = StartCameraBackend::Sim;
            cam
        })
        .collect();

    BraidConfig {
        mainbrain,
        trigger,
        cameras,
    }
}

/// Serialize a [`BraidConfig`] to TOML.
///
/// Uses the same two-step `toml::Value` dance as `braid default-config` to avoid
/// a `ValueAfterTable` serialization error.
pub fn braid_config_to_toml(config: &BraidConfig) -> eyre::Result<String> {
    let value = toml::Value::try_from(config)?;
    Ok(toml::to_string(&value)?)
}

/// Write the calibration XML and the Braid config TOML for `scenario` into
/// `out_dir`, returning the resulting paths.
pub fn generate_run(
    scenario: &Scenario,
    out_dir: &Path,
    http_api_server_addr: &str,
) -> eyre::Result<GeneratedRun> {
    std::fs::create_dir_all(out_dir)?;

    let calibration_path = out_dir.join("calibration.xml");
    let system = build_calibration(scenario)?;
    std::fs::write(&calibration_path, to_flydra_xml_string(&system)?)?;

    let braidz_output_dir = out_dir.join("braid-data");

    let config = build_braid_config(
        scenario,
        &calibration_path,
        &braidz_output_dir,
        http_api_server_addr,
    );
    let config_path = out_dir.join("braid-config.toml");
    std::fs::write(&config_path, braid_config_to_toml(&config)?)?;

    Ok(GeneratedRun {
        config_path,
        calibration_path,
        braidz_output_dir,
    })
}
