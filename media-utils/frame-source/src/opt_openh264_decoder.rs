#[cfg(not(feature = "openh264"))]
use crate::{Error, Result};

#[cfg(feature = "openh264")]
pub(crate) type DecoderType = openh264::decoder::Decoder;
#[cfg(not(feature = "openh264"))]
pub(crate) type DecoderType = NoH264Decoder;

#[cfg(not(feature = "openh264"))]
pub(crate) struct NoH264Decoder {}
#[cfg(not(feature = "openh264"))]
impl NoH264Decoder {
    pub(crate) fn new() -> Result<Self> {
        Err(Error::H264Error("No H264 decoder support at compile time"))
    }
    pub(crate) fn decode(&self, _data: &[u8]) -> Result<Option<()>> {
        Err(Error::H264Error("No H264 decoder support at compile time"))
    }
}
