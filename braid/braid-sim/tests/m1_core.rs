// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! M1 exit tests for the `braid-sim` core: the synthetic calibration is
//! self-consistent (projection <-> triangulation), survives a flydra-XML
//! round-trip, places the arena in view of multiple cameras, and the world
//! model is deterministic.

use approx::assert_relative_eq;
use braid_mvg::PointWorldFrame;
use braid_sim::calibration::{build_calibration, to_flydra_xml_string};
use braid_sim::projection::project_all;
use braid_sim::scenario::Scenario;
use braid_sim::world::World;
use flydra_mvg::FlydraMultiCameraSystem;
use nalgebra::Point3;

/// The example scenario shipped with the crate is the single source of truth.
fn demo_scenario() -> Scenario {
    Scenario::from_toml_str(include_str!("../example-sim.toml")).unwrap()
}

fn pt(x: f64, y: f64, z: f64) -> PointWorldFrame<f64> {
    PointWorldFrame {
        coords: Point3::new(x, y, z),
    }
}

/// A few representative points inside the arena to probe with.
fn sample_points(s: &Scenario) -> Vec<PointWorldFrame<f64>> {
    let c = s.arena.center();
    let h = s.arena.half_extent();
    vec![
        pt(c[0], c[1], c[2]),
        pt(c[0] + 0.5 * h[0], c[1] - 0.5 * h[1], c[2] + 0.3 * h[2]),
        pt(c[0] - 0.4 * h[0], c[1] + 0.3 * h[1], c[2] - 0.4 * h[2]),
    ]
}

#[test]
fn scenario_parses_and_camera_names_are_stable() {
    let s = demo_scenario();
    assert_eq!(s.cameras.count, 5);
    assert_eq!(Scenario::camera_name(0), "simcam0");
    // The blob defaults match the M0 spike.
    assert!(s.blob.peak >= 40);
}

#[test]
fn arena_center_projects_near_image_center() {
    // Cameras look at the arena center, so it should project near (cx, cy).
    let s = demo_scenario();
    let system = build_calibration(&s).unwrap();
    let c = s.arena.center();
    let obs = project_all(&system, &s, &pt(c[0], c[1], c[2]));
    let cx = s.cameras.image_width as f64 / 2.0;
    let cy = s.cameras.image_height as f64 / 2.0;
    for o in &obs {
        let (x, y) = o
            .pixel
            .expect("arena center must be in view of every camera");
        assert!(
            (x - cx).abs() < 2.0 && (y - cy).abs() < 2.0,
            "{}: center projected to ({x:.2},{y:.2}), expected ~({cx},{cy})",
            o.cam_name
        );
    }
}

#[test]
fn arena_points_are_visible_to_at_least_two_cameras() {
    // Required for Braid to triangulate everywhere along an insect's path.
    let s = demo_scenario();
    let system = build_calibration(&s).unwrap();
    let world = World::new(s.clone());
    let dt = 1.0 / s.fps;
    for frame in 0..1000 {
        let t = frame as f64 * dt;
        for insect in world.state_at(t) {
            let n_visible = project_all(&system, &s, &insect.pos)
                .iter()
                .filter(|o| o.pixel.is_some())
                .count();
            assert!(
                n_visible >= 2,
                "insect {} at t={t:.3} visible to only {n_visible} cameras",
                insect.id
            );
        }
    }
}

#[test]
fn projection_triangulation_roundtrip() {
    // Project a known 3D point into all cameras, then reconstruct it; the
    // synthetic calibration must be self-consistent.
    let s = demo_scenario();
    let system = build_calibration(&s).unwrap();
    for p in sample_points(&s) {
        let distorted: Vec<_> = (0..s.cameras.count)
            .map(|k| {
                let name = Scenario::camera_name(k);
                let dp = system
                    .cam_by_name(&name)
                    .unwrap()
                    .project_3d_to_distorted_pixel(&p);
                (name, dp)
            })
            .collect();
        let recon = system.find3d_distorted(&distorted).unwrap().point();
        assert_relative_eq!(recon.coords.x, p.coords.x, epsilon = 1e-6);
        assert_relative_eq!(recon.coords.y, p.coords.y, epsilon = 1e-6);
        assert_relative_eq!(recon.coords.z, p.coords.z, epsilon = 1e-6);
    }
}

#[test]
fn calibration_survives_flydra_xml_roundtrip() {
    let s = demo_scenario();
    let system = build_calibration(&s).unwrap();
    let xml = to_flydra_xml_string(&system).unwrap();
    let reloaded = FlydraMultiCameraSystem::<f64>::from_flydra_xml(xml.as_bytes()).unwrap();

    // Projections through the original and the reloaded calibration must agree.
    for p in sample_points(&s) {
        for k in 0..s.cameras.count {
            let name = Scenario::camera_name(k);
            let a = system
                .cam_by_name(&name)
                .unwrap()
                .project_3d_to_distorted_pixel(&p);
            let b = reloaded
                .cam_by_name(&name)
                .unwrap()
                .project_3d_to_distorted_pixel(&p);
            assert_relative_eq!(a.coords.x, b.coords.x, epsilon = 1e-6);
            assert_relative_eq!(a.coords.y, b.coords.y, epsilon = 1e-6);
        }
    }
}

#[test]
fn world_is_deterministic_and_respects_enter_exit() {
    let mut s = demo_scenario();
    s.insects[0].enter_t = 1.0;
    s.insects[0].exit_t = Some(3.0);

    let w1 = World::new(s.clone());
    let w2 = World::new(s.clone());

    // Deterministic: identical positions across instances.
    for i in 0..200 {
        let t = i as f64 * 0.05;
        let a = w1.state_at(t);
        let b = w2.state_at(t);
        assert_eq!(a.len(), b.len());
        for (sa, sb) in a.iter().zip(b.iter()) {
            assert_eq!(sa.id, sb.id);
            assert_eq!(sa.pos.coords, sb.pos.coords);
        }
    }

    // Enter/exit window honored.
    assert!(w1.state_at(0.5).is_empty());
    assert_eq!(w1.state_at(1.0).len(), 1);
    assert_eq!(w1.state_at(2.999).len(), 1);
    assert!(w1.state_at(3.0).is_empty());
}

#[test]
fn insects_stay_inside_the_arena() {
    let s = demo_scenario();
    let world = World::new(s.clone());
    for i in 0..2000 {
        let t = i as f64 * 0.01;
        for insect in world.state_at(t) {
            let p = insect.pos.coords;
            for axis in 0..3 {
                assert!(
                    p[axis] >= s.arena.min[axis] - 1e-9 && p[axis] <= s.arena.max[axis] + 1e-9,
                    "insect left arena on axis {axis}: {p:?}"
                );
            }
        }
    }
}
