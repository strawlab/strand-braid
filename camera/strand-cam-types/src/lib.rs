// Copyright 2020-2025 Andrew D. Straw.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT
// or http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use serde::{Deserialize, Serialize};
use strand_cam_enum_iter::EnumIter;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AutoMode {
    Off,
    Once,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TriggerMode {
    Off,
    On,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum TriggerSelector {
    AcquisitionStart,
    FrameStart,
    FrameBurstStart,
    ExposureActive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AcquisitionMode {
    Continuous,
    SingleFrame,
    MultiFrame,
}
