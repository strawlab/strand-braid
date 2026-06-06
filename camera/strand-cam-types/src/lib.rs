// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Core types for camera control and configuration in the [Strand
//! Camera](https://strawlab.org/strand-cam) ecosystem.

use serde::{Deserialize, Serialize};
use strand_cam_enum_iter::EnumIter;

/// Automatic control mode for camera features.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AutoMode {
    /// Automatic control is disabled.
    Off,
    /// Automatic control runs once then stops.
    Once,
    /// Automatic control runs continuously.
    #[default]
    Continuous,
}

// use Debug to impl Display
impl std::fmt::Display for AutoMode {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::result::Result<(), std::fmt::Error> {
        std::fmt::Debug::fmt(self, fmt)
    }
}

impl EnumIter for AutoMode {
    fn variants() -> Vec<Self> {
        vec![AutoMode::Off, AutoMode::Once, AutoMode::Continuous]
    }
}

/// Camera trigger enable state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TriggerMode {
    /// Triggering is disabled.
    Off,
    /// Triggering is enabled.
    On,
}

/// Camera trigger type selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum TriggerSelector {
    /// Trigger for starting image acquisition.
    AcquisitionStart,
    /// Trigger for starting frame capture.
    FrameStart,
    /// Trigger for starting a burst of frames.
    FrameBurstStart,
    /// Trigger for exposure timing.
    ExposureActive,
}

/// Camera acquisition mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AcquisitionMode {
    /// Continuous frame acquisition.
    Continuous,
    /// Single frame acquisition.
    SingleFrame,
    /// Multiple frame acquisition.
    MultiFrame,
}
