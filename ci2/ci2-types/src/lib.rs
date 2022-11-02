#![no_std]
extern crate core as std;

#[macro_use]
extern crate serde_derive;
extern crate enum_iter;

use enum_iter::EnumIter;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AutoMode {
    Off,
    Once,
    Continuous,
}

// use Debug to impl Display
impl std::fmt::Display for AutoMode {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::result::Result<(), std::fmt::Error> {
        std::fmt::Debug::fmt(self, fmt)
    }
}

impl EnumIter for AutoMode {
    fn variants() -> &'static [Self] {
        &[AutoMode::Off, AutoMode::Once, AutoMode::Continuous]
    }
}

impl Default for AutoMode {
    fn default() -> Self {
        AutoMode::Continuous
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
