// Copyright 2016-2025 Andrew D. Straw.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT
// or http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! Communication protocol types for the [Strand Camera](https://strawlab.org/strand-cam) LED Box device.
//!
//! This crate provides the data structures and constants for communicating
//! with the Strand LED Box hardware device over serial communication.
//!
//! ## Features
//!
//! - `std`: Enables standard library support (default)
//! - `print-defmt`: Enables defmt formatting for embedded debugging

#![cfg_attr(not(feature = "std"), no_std)]
#![warn(missing_docs)]

extern crate serde;

#[cfg(not(feature = "std"))]
extern crate core as std;

use serde::{Deserialize, Serialize};

/// Maximum intensity value for LED channels.
pub const MAX_INTENSITY: u16 = 16000;
/// Communication protocol version.
pub const COMM_VERSION: u16 = 3;
/// Serial communication baud rate.
pub const BAUD_RATE: u32 = 230_400;

/// Messages sent to the LED box device.
#[derive(Debug, PartialEq, Serialize, Deserialize, Clone, Copy)]
#[cfg_attr(feature = "print-defmt", derive(defmt::Format))]
pub enum ToDevice {
    /// Set the device state.
    DeviceState(DeviceState),
    /// Send an echo request with 8 bytes.
    EchoRequest8((u8, u8, u8, u8, u8, u8, u8, u8)),
    /// Request the firmware version.
    VersionRequest,
}

/// Messages received from the LED box device.
#[derive(Debug, PartialEq, Serialize, Deserialize, Clone, Copy)]
#[cfg_attr(feature = "print-defmt", derive(defmt::Format))]
pub enum FromDevice {
    /// Current device state.
    DeviceState(DeviceState),
    /// Echo response with 8 bytes.
    EchoResponse8((u8, u8, u8, u8, u8, u8, u8, u8)),
    /// Firmware version response.
    VersionResponse(u16),
    /// Confirmation that state was set.
    StateWasSet,
}

/// Complete state of the LED box device with all four channels.
#[derive(Debug, PartialEq, Serialize, Deserialize, Clone, Copy)]
#[cfg_attr(feature = "print-defmt", derive(defmt::Format))]
pub struct DeviceState {
    /// Channel 1 state.
    pub ch1: ChannelState,
    /// Channel 2 state.
    pub ch2: ChannelState,
    /// Channel 3 state.
    pub ch3: ChannelState,
    /// Channel 4 state.
    pub ch4: ChannelState,
}

impl DeviceState {
    /// Create a default device state with all channels off.
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

/// State of a single LED channel.
#[derive(Debug, PartialEq, Serialize, Deserialize, Clone, Copy)]
#[cfg_attr(feature = "print-defmt", derive(defmt::Format))]
pub struct ChannelState {
    /// Channel number (1-4).
    pub num: u8,
    /// Whether the channel is on or off.
    pub on_state: OnState,
    /// LED intensity level.
    pub intensity: u16,
}

impl ChannelState {
    /// Create a default channel state with the given channel number.
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

/// LED channel on/off state.
#[derive(Debug, PartialEq, Serialize, Deserialize, Clone, Copy)]
#[cfg_attr(feature = "print-defmt", derive(defmt::Format))]
#[derive(Default)]
pub enum OnState {
    #[default]
    /// LED is turned off.
    Off,
    /// LED is constantly on.
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
