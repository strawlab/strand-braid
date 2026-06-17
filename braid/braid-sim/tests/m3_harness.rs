// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! M3a: the generated Braid config + calibration are valid and consistent with
//! the scenario.

use braid_sim::Scenario;
use braid_sim::harness::{braid_config_to_toml, build_braid_config, generate_run};
use braid_types::{StartCameraBackend, TriggerType};

fn demo_scenario() -> Scenario {
    Scenario::from_toml_str(include_str!("../example-sim.toml")).unwrap()
}

#[test]
fn generated_config_matches_scenario() {
    let s = demo_scenario();
    let config = build_braid_config(
        &s,
        std::path::Path::new("/tmp/cal.xml"),
        std::path::Path::new("/tmp/out"),
        "127.0.0.1:0",
    );

    // One sim camera per scenario camera, all using the sim backend.
    assert_eq!(config.cameras.len(), s.cameras.count);
    for (k, cam) in config.cameras.iter().enumerate() {
        assert_eq!(cam.name, Scenario::camera_name(k));
        assert_eq!(cam.start_backend, StartCameraBackend::Sim);
    }

    // FakeSync at the scenario frame rate.
    match &config.trigger {
        TriggerType::FakeSync(fs) => assert_eq!(fs.framerate, s.fps),
        other => panic!("expected FakeSync, got {other:?}"),
    }

    assert!(config.mainbrain.cal_fname.is_some());
}

#[test]
fn generated_config_roundtrips_through_braid_parser() {
    // The generated TOML must be accepted by Braid's own config parser.
    let s = demo_scenario();
    let config = build_braid_config(
        &s,
        std::path::Path::new("/tmp/cal.xml"),
        std::path::Path::new("/tmp/out"),
        "127.0.0.1:0",
    );
    let toml_str = braid_config_to_toml(&config).unwrap();
    let parsed: braid_config_data::BraidConfig = toml::from_str(&toml_str).unwrap();
    assert_eq!(parsed.cameras.len(), s.cameras.count);
}

#[test]
fn generate_run_writes_artifacts() {
    let s = demo_scenario();
    let dir = std::env::temp_dir().join(format!("braid-sim-m3a-{}", std::process::id()));
    let run = generate_run(&s, &dir, "127.0.0.1:0").unwrap();

    assert!(run.config_path.exists(), "config not written");
    assert!(run.calibration_path.exists(), "calibration not written");

    // The calibration is a real flydra XML that re-parses.
    let xml = std::fs::read_to_string(&run.calibration_path).unwrap();
    let system =
        flydra_mvg::FlydraMultiCameraSystem::<f64>::from_flydra_xml(xml.as_bytes()).unwrap();
    assert!(system.cam_by_name("simcam0").is_some());

    // Braid's parser accepts the written config file.
    let parsed = braid_config_data::parse_config_file(&run.config_path).unwrap();
    assert_eq!(parsed.cameras.len(), s.cameras.count);

    let _ = std::fs::remove_dir_all(&dir);
}
