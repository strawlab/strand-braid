// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The deterministic ground-truth world: insect 3D positions as a pure function
//! of time.

use std::f64::consts::PI;

use braid_mvg::PointWorldFrame;
use nalgebra::Point3;

use crate::scenario::Scenario;

/// Ground-truth state of one insect at one instant.
#[derive(Debug, Clone)]
pub struct InsectState {
    /// Ground-truth identity.
    pub id: u32,
    /// 3D position in the world (arena) frame, meters.
    pub pos: PointWorldFrame<f64>,
}

/// The simulated world. [`World::state_at`] is a pure function of time, so any
/// number of independent fake-camera processes can reconstruct the same world
/// for a given synchronized frame without any communication.
#[derive(Debug, Clone)]
pub struct World {
    scenario: Scenario,
}

impl World {
    /// Create a world from a scenario.
    pub fn new(scenario: Scenario) -> Self {
        World { scenario }
    }

    /// The scenario backing this world.
    pub fn scenario(&self) -> &Scenario {
        &self.scenario
    }

    /// Ground-truth positions of all insects present at time `t` (seconds).
    ///
    /// An insect is present for `enter_t <= t < exit_t` (with no upper bound if
    /// `exit_t` is `None`). The result is ordered by the order of insects in the
    /// scenario.
    pub fn state_at(&self, t: f64) -> Vec<InsectState> {
        let center = self.scenario.arena.center();
        let half = self.scenario.arena.half_extent();
        self.scenario
            .insects
            .iter()
            .filter(|spec| t >= spec.enter_t && spec.exit_t.is_none_or(|exit| t < exit))
            .map(|spec| {
                let m = &spec.motion;
                let mut p = [0.0f64; 3];
                for k in 0..3 {
                    let amp = half[k] * m.fill;
                    p[k] = center[k] + amp * (2.0 * PI * m.freq_hz[k] * t + m.phase[k]).sin();
                }
                InsectState {
                    id: spec.id,
                    pos: PointWorldFrame {
                        coords: Point3::new(p[0], p[1], p[2]),
                    },
                }
            })
            .collect()
    }
}
