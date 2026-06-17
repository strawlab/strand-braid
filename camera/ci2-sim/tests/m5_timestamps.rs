// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Verify the timestamp behavior the frame-rate-estimation fix relies on: the
//! sim camera emits a hardware (device) timestamp at the *true* frame cadence,
//! and the `reported_fps` knob *bunches* the host timestamps independently. A
//! fps estimator using the hardware timestamp therefore stays correct even when
//! the host clock is bunched (the cause of the live-vs-retrack fragmentation
//! bug).

use ci2::{Camera, CameraModule};

// True cadence 200 fps, but host timestamps report 600 fps (bunched). High
// rates keep the test fast (acquisition is paced to the true fps).
const SIM_TOML: &str = r#"
seed = 1
fps = 200.0
reported_fps = 600.0
[arena]
min = [-0.15, -0.15, 0.0]
max = [0.15, 0.15, 0.30]
[cameras]
count = 1
radius_m = 0.6
height_m = 0.7
focal_length_px = 450.0
image_width = 320
image_height = 256
[[insects]]
id = 1
[insects.motion]
freq_hz = [0.4, 0.33, 0.0]
phase = [0.0, 1.0, 2.0]
fill = 0.5
"#;

fn device_timestamp(frame: &ci2::DynamicFrameWithInfo) -> u64 {
    let any = ci2::AsAny::as_any(&**frame.backend_data.as_ref().expect("sim must emit backend_data"));
    any.downcast_ref::<ci2_pylon_types::PylonExtra>()
        .expect("sim emits a PylonExtra hardware timestamp")
        .device_timestamp
}

#[test]
fn hardware_timestamp_is_true_cadence_while_host_clock_is_bunched() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("ci2-sim-m5-{}.toml", std::process::id()));
    std::fs::write(&path, SIM_TOML).unwrap();
    // SAFETY: single-threaded test; the env var is read by ci2-sim here only.
    unsafe { std::env::set_var(ci2_sim::SIM_SPEC_ENV, &path) };

    let module = ci2_sim::new_module().unwrap();
    let mut module_ref: &ci2_sim::WrappedModule = &module;
    let mut cam = module_ref.camera("simcam0").unwrap();
    cam.acquisition_start().unwrap();

    // Grab two frames 60 apart and measure both clocks.
    let f0 = cam.next_frame().unwrap();
    let mut last = cam.next_frame().unwrap();
    for _ in 0..59 {
        last = cam.next_frame().unwrap();
    }

    let d_frames = (last.host_timing.fno - f0.host_timing.fno) as f64;

    // Hardware timestamp: should reflect the TRUE 200 fps.
    let dev_dt_s = (device_timestamp(&last) - device_timestamp(&f0)) as f64 / 1e9;
    let hw_fps = d_frames / dev_dt_s;
    assert!(
        (hw_fps - 200.0).abs() < 2.0,
        "hardware-timestamp fps {hw_fps} should be ~200 (true cadence)"
    );

    // Host clock: bunched, reports the (wrong) 600 fps.
    let host_dt_s = last
        .host_timing
        .datetime
        .signed_duration_since(f0.host_timing.datetime)
        .num_nanoseconds()
        .unwrap() as f64
        / 1e9;
    let host_fps = d_frames / host_dt_s;
    assert!(
        (host_fps - 600.0).abs() < 5.0,
        "host-clock fps {host_fps} should be the bunched ~600"
    );

    let _ = std::fs::remove_file(&path);
}
