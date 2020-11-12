//! On-board user LED

#[cfg(feature = "nucleo64")]
use crate::stm32_hal::gpio::gpioa::PA5;
#[cfg(feature = "nucleo32")]
use stm32_hal::gpio::gpiob::PB3;

use crate::stm32_hal::gpio::{Output, PushPull};

#[cfg(feature = "nucleo64")]
pub type UserLED = PA5<Output<PushPull>>;

#[cfg(feature = "nucleo32")]
pub type UserLED = PB3<Output<PushPull>>;
