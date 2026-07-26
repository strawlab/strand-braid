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

/// In-process encoder using the local `openh264-rs` fork's `set_bitrate` and
/// `force_intra_frame`. One [`openh264::encoder::EncodedBitStream`] is exactly
/// one access unit, so AU boundaries and the marker bit are exact with no
/// stream re-parsing.
pub(crate) struct OpenH264StreamEncoder {
    encoder: Encoder,
    au_tx: SyncSender<AccessUnit>,
}

impl OpenH264StreamEncoder {
    pub(crate) fn new(
        _cfg: OpenH264EncoderConfig,
        bitrate_bps: u32,
        idr_interval_frames: u32,
        payload_budget: usize,
        au_tx: SyncSender<AccessUnit>,
    ) -> Result<Self> {
        let max_slice_len = payload_budget.saturating_sub(SLICE_LEN_MARGIN);
        let enc_cfg = EncoderConfig::new()
            .usage_type(UsageType::CameraVideoRealTime)
            .rate_control_mode(RateControlMode::Bitrate)
            .bitrate(BitRate::from_bps(bitrate_bps))
            .intra_frame_period(IntraFramePeriod::from_num_frames(idr_interval_frames))
            .max_slice_len(max_slice_len as u32)
            .num_threads(1);
        let encoder = Encoder::with_api_config(OpenH264API::from_source(), enc_cfg)?;
        Ok(Self { encoder, au_tx })
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

    fn set_bitrate(&mut self, bps: u32) -> Result<()> {
        self.encoder.set_bitrate(BitRate::from_bps(bps))?;
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
