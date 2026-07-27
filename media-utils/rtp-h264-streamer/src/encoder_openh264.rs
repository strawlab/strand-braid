// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::sync::mpsc::SyncSender;

use h264_rtp::AccessUnit;
use openh264::{
    OpenH264API,
    encoder::{
        BitRate, Encoder, EncoderConfig, FrameType, IntraFramePeriod, RateControlMode, UsageType,
    },
};

use crate::{Error, Result, encoder::H264StreamEncoder};

/// Headroom subtracted from the RTP payload budget before it is handed to the
/// encoder as `max_slice_len`, so the encoder's own slice-size rounding cannot
/// push a slice NAL just over the budget and force it into FU-A fragmentation
/// (which single-NAL packets otherwise avoid entirely on this path).
const SLICE_LEN_MARGIN: usize = 32;

/// Configuration specific to the openh264 in-process encoder.
///
/// Currently carries no fields of its own: bitrate, the intra-frame period and
/// the slice-length budget all come from the shared [`crate::StreamConfig`]
/// fields, since they apply identically to every encoder backend.
#[derive(Debug, Clone, Default)]
pub struct OpenH264EncoderConfig {}

/// In-process encoder using the local `openh264-rs` fork's `force_intra_frame`.
/// One [`openh264::encoder::EncodedBitStream`] is exactly one access unit, so
/// AU boundaries and the marker bit are exact with no stream re-parsing.
///
/// [`Self::set_bitrate`] does *not* use the fork's `Encoder::set_bitrate`.
/// Verified experimentally: OpenH264's native `SetOption(ENCODER_OPTION_BITRATE)`
/// on an already-initialized encoder only succeeds when the new bitrate is
/// less than or equal to the one the encoder currently holds -- any increase
/// fails with a native error (undocumented upstream; not a bug in the Rust
/// binding). Since a live stream must be able to ramp bitrate up as well as
/// down, `set_bitrate` instead reinitializes a fresh `Encoder` at the new
/// bitrate, which works in both directions and, as a bonus, makes the next
/// frame a natural IDR (every fresh encoder's first frame is one) --
/// consistent with the ffmpeg backend's own respawn-on-`set_bitrate` behavior.
pub(crate) struct OpenH264StreamEncoder {
    encoder: Encoder,
    au_tx: SyncSender<AccessUnit>,
    idr_interval_frames: u32,
    max_slice_len: u32,
}

fn build_encoder(
    bitrate_kbps: u32,
    idr_interval_frames: u32,
    max_slice_len: u32,
) -> Result<Encoder> {
    let enc_cfg = EncoderConfig::new()
        .usage_type(UsageType::CameraVideoRealTime)
        .rate_control_mode(RateControlMode::Bitrate)
        .bitrate(BitRate::from_bps(bitrate_kbps.saturating_mul(1000)))
        .intra_frame_period(IntraFramePeriod::from_num_frames(idr_interval_frames))
        .max_slice_len(max_slice_len)
        .num_threads(1);
    Ok(Encoder::with_api_config(
        OpenH264API::from_source(),
        enc_cfg,
    )?)
}

impl OpenH264StreamEncoder {
    pub(crate) fn new(
        _cfg: OpenH264EncoderConfig,
        bitrate_bps: u32,
        idr_interval_frames: u32,
        payload_budget: usize,
        au_tx: SyncSender<AccessUnit>,
    ) -> Result<Self> {
        let max_slice_len = payload_budget.saturating_sub(SLICE_LEN_MARGIN) as u32;
        let encoder = build_encoder(bitrate_bps, idr_interval_frames, max_slice_len)?;
        Ok(Self {
            encoder,
            au_tx,
            idr_interval_frames,
            max_slice_len,
        })
    }
}

/// Strip a leading 3- or 4-byte Annex-B start code, if present.
///
/// OpenH264 emits each NAL's bytes with a start code already prepended; the
/// RTP payloader wants bare NALs.
fn strip_start_code(nal: &[u8]) -> &[u8] {
    if nal.starts_with(&[0, 0, 0, 1]) {
        &nal[4..]
    } else if nal.starts_with(&[0, 0, 1]) {
        &nal[3..]
    } else {
        nal
    }
}

impl H264StreamEncoder for OpenH264StreamEncoder {
    fn submit(
        &mut self,
        frame: &strand_dynamic_frame::DynamicFrame,
        pts: std::time::Duration,
    ) -> Result<()> {
        let y4m_frame =
            y4m_writer::encode_y4m_dynamic_frame(frame, y4m::Colorspace::C420paldv, None)?;
        let timestamp = openh264::Timestamp::from_millis(pts.as_millis() as u64);
        let encoded = self.encoder.encode_at(&y4m_frame, timestamp)?;
        let is_keyframe = matches!(encoded.frame_type(), FrameType::IDR | FrameType::I);

        let mut nals = Vec::new();
        for layer_idx in 0..encoded.num_layers() {
            let layer = encoded.layer(layer_idx).expect("layer_idx < num_layers");
            for nal_idx in 0..layer.nal_count() {
                let nal = layer.nal_unit(nal_idx).expect("nal_idx < nal_count");
                nals.push(strip_start_code(nal).to_vec());
            }
        }

        let au = AccessUnit {
            nals,
            is_keyframe,
            pts,
        };
        self.au_tx.send(au).map_err(|_| Error::SenderDisconnected)?;
        Ok(())
    }

    fn set_bitrate_kbps(&mut self, kbps: u32) -> Result<()> {
        self.encoder = build_encoder(kbps, self.idr_interval_frames, self.max_slice_len)?;
        Ok(())
    }

    fn request_keyframe(&mut self) -> Result<()> {
        self.encoder.force_intra_frame();
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<()> {
        // No child process or file handle to flush; dropping the encoder is
        // enough.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::sync_channel;

    fn new_test_encoder(
        bitrate_bps: u32,
    ) -> (OpenH264StreamEncoder, std::sync::mpsc::Receiver<AccessUnit>) {
        let (au_tx, au_rx) = sync_channel(8);
        let encoder = OpenH264StreamEncoder::new(
            OpenH264EncoderConfig::default(),
            bitrate_bps,
            30,
            1360,
            au_tx,
        )
        .unwrap();
        (encoder, au_rx)
    }

    fn encode_one_frame(encoder: &mut OpenH264StreamEncoder) {
        let (width, height) = (64u32, 48u32);
        let buf = vec![0u8; (width * height * 3) as usize];
        let frame = strand_dynamic_frame::DynamicFrameOwned::from_buf(
            width,
            height,
            (width * 3) as usize,
            buf,
            machine_vision_formats::PixFmt::RGB8,
        )
        .unwrap();
        encoder
            .submit(&frame.borrow(), std::time::Duration::ZERO)
            .unwrap();
    }

    /// Regression test: OpenH264's native `SetOption(ENCODER_OPTION_BITRATE)`
    /// silently fails (returns a native error) when asked to *increase* an
    /// already-initialized encoder's bitrate, even though decreasing it
    /// works fine -- verified experimentally against the openh264-rs fork
    /// directly, not just through this crate. `set_bitrate` must not regress
    /// to calling that API path directly, in either direction.
    #[test]
    fn set_bitrate_succeeds_for_both_increase_and_decrease_after_encoding() {
        let (mut encoder, _au_rx) = new_test_encoder(500_000);
        encode_one_frame(&mut encoder);

        encoder
            .set_bitrate_kbps(1_000)
            .expect("increase must succeed");
        encode_one_frame(&mut encoder);

        encoder.set_bitrate_kbps(200).expect("decrease must succeed");
        encode_one_frame(&mut encoder);
    }
}
