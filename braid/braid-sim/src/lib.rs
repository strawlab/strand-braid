// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Simulation core for end-to-end testing of Braid's live 3D tracking.
//!
//! This crate is the shared core described in
//! `scratch/2026-06-17_braid-live-3d-sim-test-plan.md` (milestone M1). It
//! provides:
//!
//! - [`scenario::Scenario`]: a `sim.toml`-deserializable description of the
//!   simulated world (arena, cameras, insects, blob rendering, frame rate).
//! - [`world::World`]: a deterministic ground-truth model — insect 3D positions
//!   are a pure function of time, so independent fake-camera processes can each
//!   reconstruct the same world for a given synchronized frame.
//! - [`calibration::build_calibration`]: generate a synthetic multi-camera
//!   [`flydra_mvg::FlydraMultiCameraSystem`] (and serialize it to the flydra XML
//!   that Braid loads) so the *same* calibration is used to project ground truth
//!   and to reconstruct it.
//! - [`projection`]: project a 3D point into each camera's distorted pixel, with
//!   field-of-view culling.

pub mod calibration;
pub mod projection;
pub mod scenario;
pub mod world;

pub use scenario::Scenario;
pub use world::{InsectState, World};
