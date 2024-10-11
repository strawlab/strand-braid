#![no_std]
extern crate core as std;

#[macro_use]
extern crate serde_derive;
extern crate enum_iter;

use enum_iter::EnumIter;

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
    fn variants() -> &'static [Self] {
        &[AutoMode::Off, AutoMode::Once, AutoMode::Continuous]
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
