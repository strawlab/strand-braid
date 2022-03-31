#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "collections")]
extern crate collections;

#[macro_use]
extern crate serde_derive;
extern crate serde;

#[cfg(all(test, feature = "std"))]
#[macro_use]
extern crate quickcheck;

#[cfg(test)]
extern crate rand;
#[cfg(test)]
use rand::Rng;

extern crate enum_iter;

#[cfg(not(feature = "std"))]
extern crate core as std;

use enum_iter::EnumIter;

pub const MAX_INTENSITY: u16 = 16000;

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone, Copy)]
#[cfg_attr(feature = "print-defmt", derive(defmt::Format))]
pub enum ToDevice {
    DeviceState(DeviceState),
    EchoRequest8((u8, u8, u8, u8, u8, u8, u8, u8)),
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone, Copy)]
#[cfg_attr(feature = "print-defmt", derive(defmt::Format))]
pub enum FromDevice {
    DeviceState(DeviceState),
    EchoResponse8((u8, u8, u8, u8, u8, u8, u8, u8)),
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
            num: num,
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
pub enum OnState {
    Off,
    ConstantOn,
}

impl Default for OnState {
    fn default() -> Self {
        OnState::Off
    }
}

impl std::fmt::Display for OnState {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::result::Result<(), std::fmt::Error> {
        match self {
            _ => std::fmt::Debug::fmt(self, fmt),
        }
    }
}

// I found this necessary to avoid lifetime error in camtrig-firmware. Not
// sure why this needs to be allocated as with const to be 'static in this
// case (but not in standard linux target).
const ON_STATE_VARIANTS: [OnState; 2] = [OnState::Off, OnState::ConstantOn];
const ON_STATE_VARIANTS_REF: &[OnState] = &ON_STATE_VARIANTS;

impl EnumIter for OnState {
    fn variants() -> &'static [Self] {
        ON_STATE_VARIANTS_REF
    }
}

// --------------------------------------------------------
// testing
// --------------------------------------------------------

#[cfg(test)]
impl quickcheck::Arbitrary for DeviceState {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> DeviceState {
        DeviceState {
            ch1: ChannelState::arbitrary(g),
            ch2: ChannelState::arbitrary(g),
            ch3: ChannelState::arbitrary(g),
            ch4: ChannelState::arbitrary(g),
        }
    }
}

#[cfg(test)]
impl quickcheck::Arbitrary for ChannelState {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> ChannelState {
        ChannelState {
            num: g.gen(),
            on_state: OnState::arbitrary(g),
            intensity: g.gen(),
        }
    }
}

#[cfg(test)]
impl quickcheck::Arbitrary for OnState {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> OnState {
        match g.gen_range(0, 2) {
            0 => OnState::Off,
            1 => OnState::ConstantOn,
            _ => unreachable!(),
        }
    }
}

#[cfg(feature = "std")]
#[cfg(test)]
mod tests {
    extern crate ssmarshal;

    use self::ssmarshal::{deserialize, serialize};
    use serde::de::DeserializeOwned;
    use serde::Serialize;
    use std;

    use {ChannelState, DeviceState, OnState};

    fn rt_val<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(val: &T) -> bool {
        let mut buf = vec![0; std::mem::size_of::<T>()];
        serialize(&mut buf, val).unwrap();
        let new_val: T = deserialize(&buf).unwrap().0;
        println!("\n\nOld: {:?}\nNew: {:?}", val, new_val);
        val == &new_val
    }

    quickcheck! {
        fn rt_device_state(val: DeviceState) -> bool {
            rt_val(&val)
        }
    }

    quickcheck! {
        fn rt_channel_state(val: ChannelState) -> bool {
            rt_val(&val)
        }
    }

    quickcheck! {
        fn rt_on_state(val: OnState) -> bool {
            rt_val(&val)
        }
    }
}
