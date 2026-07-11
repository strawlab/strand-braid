// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

#[cfg(not(feature = "openh264"))]
use crate::Error;
use crate::Result;

#[cfg(feature = "openh264")]
pub(crate) type DecoderType = openh264::decoder::Decoder;
#[cfg(not(feature = "openh264"))]
pub(crate) type DecoderType = NoH264Decoder;

/// Create a decoder for sequential whole-stream decoding.
///
/// Mid-stream flushing is disabled (`Flush::NoFlush`): for streams with
/// B-frames the decoder buffers pictures to reorder them into display order,
/// and flushing while pictures are pending evicts reference frames from the
/// decoded picture buffer, corrupting the decode of subsequent frames (it
/// fails with `dsOutOfMemory` / `dsNoParamSets`). Instead, a decode call that
/// buffers its input simply returns no picture, and the pictures still
/// buffered at end of stream must be drained with
/// [`flush_remaining`](openh264::decoder::Decoder::flush_remaining).
#[cfg(feature = "openh264")]
pub(crate) fn new_stream_decoder() -> Result<DecoderType> {
    let api = openh264::OpenH264API::from_source();
    let config = openh264::decoder::DecoderConfig::new()
        .flush_after_decode(openh264::decoder::Flush::NoFlush);
    Ok(openh264::decoder::Decoder::with_api_config(api, config)?)
}

#[cfg(not(feature = "openh264"))]
pub(crate) fn new_stream_decoder() -> Result<DecoderType> {
    NoH264Decoder::new()
}

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
    pub(crate) fn flush_remaining(&mut self) -> Result<Vec<()>> {
        Err(Error::H264Error("No H264 decoder support at compile time"))
    }
}
