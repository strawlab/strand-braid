use crate::stm32_hal;
use stm32_hal::serial::Tx;
use stm32_hal::stm32::USART2;

pub struct WrappedTx {
    pub tx: Tx<USART2>,
}

impl embedded_hal::serial::Write<u8> for WrappedTx {
    type Error = core::convert::Infallible;
    #[inline]
    fn write(&mut self, byte: u8) -> nb::Result<(), core::convert::Infallible> {
        self.tx.write(byte)
    }
    #[inline]
    fn flush(&mut self) -> nb::Result<(), core::convert::Infallible> {
        self.tx.flush()
    }
}
