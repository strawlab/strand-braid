#![cfg_attr(not(feature = "std"), no_std)]

extern crate serde;

#[cfg(not(feature = "std"))]
extern crate core as std;

use serde::{Serialize, Deserialize};

pub const MAX_INTENSITY: u16 = 16000;
pub const COMM_VERSION: u16 = 3;
pub const BAUD_RATE: u32 = 230_400;

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone, Copy)]
#[cfg_attr(feature = "print-defmt", derive(defmt::Format))]
pub enum ToDevice {
    DeviceState(DeviceState),
    EchoRequest8((u8, u8, u8, u8, u8, u8, u8, u8)),
    VersionRequest,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone, Copy)]
#[cfg_attr(feature = "print-defmt", derive(defmt::Format))]
pub enum FromDevice {
    DeviceState(DeviceState),
    EchoResponse8((u8, u8, u8, u8, u8, u8, u8, u8)),
    VersionResponse(u16),
    StateWasSet,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone, Copy)]
#[cfg_attr(feature = "print-defmt", derive(defmt::Format))]
pub struct CounterInfo {
    pub cnt: u16,
    pub psc: u16,
    pub arr: u16,
    pub ccr1: u16,
    // pub ccmr1: u16,
    // pub cr1: u16,
    pub cr2_ois1: Option<u8>,
    // pub egr: u16,
    // pub ccer: u16,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone, Copy)]
#[cfg_attr(feature = "print-defmt", derive(defmt::Format))]
pub struct DeviceState {
    pub ch1: ChannelState,
    pub ch2: ChannelState,
    pub ch3: ChannelState,
    pub ch4: ChannelState,
}

impl DeviceState {
    pub const fn default() -> DeviceState {
        DeviceState {
            ch1: ChannelState::default(1),
            ch2: ChannelState::default(2),
            ch3: ChannelState::default(3),
            ch4: ChannelState::default(4),
        }
    }
}

impl Default for DeviceState {
    fn default() -> DeviceState {
        DeviceState::default()
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone, Copy)]
#[cfg_attr(feature = "print-defmt", derive(defmt::Format))]
pub struct ChannelState {
    pub num: u8,
    pub on_state: OnState,
    pub intensity: u16,
}

impl ChannelState {
    pub const fn default(num: u8) -> ChannelState {
        ChannelState {
            num,
            on_state: OnState::Off,
            intensity: MAX_INTENSITY,
        }
    }
}

impl Default for ChannelState {
    fn default() -> ChannelState {
        ChannelState::default(1)
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone, Copy)]
#[cfg_attr(feature = "print-defmt", derive(defmt::Format))]
#[derive(Default)]
pub enum OnState {
    #[default]
    Off,
    ConstantOn,
}

impl std::fmt::Display for OnState {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::result::Result<(), std::fmt::Error> {
        std::fmt::Debug::fmt(self, fmt)
    }
}

#[cfg(feature = "std")]
impl strand_cam_enum_iter::EnumIter for OnState {
    fn variants() -> Vec<Self> {
        vec![OnState::Off, OnState::ConstantOn]
    }
}
