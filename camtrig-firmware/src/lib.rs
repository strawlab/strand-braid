#![no_std]

use stm32f3xx_hal as stm32_hal;

pub mod led;

mod wrapped_tx;

pub use crate::wrapped_tx::WrappedTx;

// mod frequency {
//     pub const APB1: u32 = 8_000_000;
// }
