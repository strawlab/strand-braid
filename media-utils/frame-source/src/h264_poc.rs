// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reconstruct the H.264 **picture order count** (POC) from a bitstream.
//!
//! The POC (ITU-T H.264 §8.2.1) is the one signal that cannot lie about a
//! stream's true *display order*: every slice header carries enough information
//! to recover the relative presentation order of samples, independent of any
//! container metadata (`stts`/`ctts`) or of what a (possibly buggy) writer put
//! in a per-frame timestamp SEI. Within a coded video sequence (delimited by
//! IDR pictures), sorting frames by POC yields presentation order.
//!
//! This module is used both to reorder frames into presentation order (see
//! [`crate::FrameDataSource::presentation_order_iter`]) and by
//! `mp4-bframe-doctor` to detect/repair files whose timing disagrees with the
//! bitstream's real display order.

use h264_reader::{
    Context as H264ParsingContext,
    nal::{
        Nal, RefNal, UnitType,
        pps::PicParameterSet,
        slice::{PicOrderCountLsb, SliceHeader},
        sps::{PicOrderCntType, SeqParameterSet},
    },
};

use crate::{
    Error, Result,
    h264_source::{H264Source, SeekableH264Source},
};

/// Reconstructs picture order count (POC) for `pic_order_cnt_type == 0`
/// streams (ITU-T H.264 §8.2.1.1), which covers essentially all cameras and
/// software/hardware H.264 encoders in practice.
pub(crate) struct PocDecoder {
    max_poc_lsb: i64,
    prev_poc_msb: i64,
    prev_poc_lsb: i64,
}

impl PocDecoder {
    fn new(log2_max_pic_order_cnt_lsb_minus4: u8) -> Self {
        Self {
            max_poc_lsb: 1i64 << (log2_max_pic_order_cnt_lsb_minus4 as i64 + 4),
            prev_poc_msb: 0,
            prev_poc_lsb: 0,
        }
    }

    /// Feed the next sample's slice header info, in decode order, and get
    /// back its POC.
    fn next_poc(&mut self, is_idr: bool, nal_ref_idc: u8, poc_lsb: i64) -> i64 {
        if is_idr {
            self.prev_poc_msb = 0;
            self.prev_poc_lsb = 0;
        }

        let half_max = self.max_poc_lsb / 2;
        let poc_msb = if poc_lsb < self.prev_poc_lsb && (self.prev_poc_lsb - poc_lsb) >= half_max {
            self.prev_poc_msb + self.max_poc_lsb
        } else if poc_lsb > self.prev_poc_lsb && (poc_lsb - self.prev_poc_lsb) > half_max {
            self.prev_poc_msb - self.max_poc_lsb
        } else {
            self.prev_poc_msb
        };

        let poc = poc_msb + poc_lsb;

        // Only reference pictures participate in the prevPicOrderCnt chain.
        if nal_ref_idc != 0 {
            self.prev_poc_msb = poc_msb;
            self.prev_poc_lsb = poc_lsb;
        }

        poc
    }
}

/// How each frame's picture order count is obtained, selected from the SPS's
/// `pic_order_cnt_type`.
pub(crate) enum PocStrategy {
    /// `pic_order_cnt_type == 0`: read `pic_order_cnt_lsb` from every slice and
    /// unwrap it (this is the type that can carry B-frame reordering).
    FromSliceLsb(PocDecoder),
    /// `pic_order_cnt_type == 2`: the bitstream guarantees decode order equals
    /// display order (ITU-T H.264 §8.2.1.3 — no reordering is possible), so the
    /// POC simply follows decode order.
    DecodeOrder { next: i64 },
}

/// Determine the [`PocStrategy`] from an SPS's `pic_order_cnt_type`. Types 0 and
/// 2 are supported; type 1 (rare, delta-based) is not.
pub(crate) fn strategy_from_sps(sps: &SeqParameterSet) -> Result<PocStrategy> {
    match sps.pic_order_cnt {
        PicOrderCntType::TypeZero {
            log2_max_pic_order_cnt_lsb_minus4,
        } => Ok(PocStrategy::FromSliceLsb(PocDecoder::new(
            log2_max_pic_order_cnt_lsb_minus4,
        ))),
        PicOrderCntType::TypeTwo => Ok(PocStrategy::DecodeOrder { next: 0 }),
        PicOrderCntType::TypeOne { .. } => Err(Error::H264Poc(
            "uses pic_order_cnt_type 1, which is not supported".to_string(),
        )),
    }
}

/// Parse an SPS NAL and determine its [`PocStrategy`].
pub(crate) fn parse_sps(nal: &RefNal<'_>) -> Result<(SeqParameterSet, PocStrategy)> {
    let sps = SeqParameterSet::from_bits(nal.rbsp_bits())
        .map_err(|e| Error::H264Poc(format!("bad SPS: {e:?}")))?;
    let strategy = strategy_from_sps(&sps)?;
    Ok((sps, strategy))
}

/// Extract `(is_idr, nal_ref_idc, pic_order_cnt_lsb)` from the first slice NAL
/// unit in a decoded sample.
fn read_slice_poc_lsb(ctx: &H264ParsingContext, nals: &[Vec<u8>]) -> Result<(bool, u8, i64)> {
    for nal_bytes in nals {
        let nal = RefNal::new(nal_bytes, &[], true);
        let header = nal
            .header()
            .map_err(|e| Error::H264Poc(format!("bad NAL header: {e:?}")))?;
        let unit_type = header.nal_unit_type();
        if !matches!(
            unit_type,
            UnitType::SliceLayerWithoutPartitioningIdr
                | UnitType::SliceLayerWithoutPartitioningNonIdr
        ) {
            continue;
        }
        let is_idr = unit_type == UnitType::SliceLayerWithoutPartitioningIdr;
        let mut r = nal.rbsp_bits();
        let (slice_header, _sps, _pps) = SliceHeader::from_bits(ctx, &mut r, header)
            .map_err(|e| Error::H264Poc(format!("bad slice header: {e:?}")))?;
        let poc_lsb = match slice_header.pic_order_cnt_lsb {
            Some(PicOrderCountLsb::Frame(lsb)) => lsb as i64,
            Some(_) => {
                return Err(Error::H264Poc(
                    "field pictures are not supported".to_string(),
                ));
            }
            None => {
                return Err(Error::H264Poc(
                    "slice has no pic_order_cnt_lsb (unsupported pic_order_cnt_type)".to_string(),
                ));
            }
        };
        return Ok((is_idr, header.nal_ref_idc(), poc_lsb));
    }
    Err(Error::H264Poc("sample has no slice NAL unit".to_string()))
}

/// Advance a [`PocStrategy`] by one frame (whose NAL units are `nals`, in decode
/// order) and return that frame's POC. `ctx` must already contain the SPS/PPS
/// referenced by the slice.
pub(crate) fn advance_poc(
    strategy: &mut PocStrategy,
    ctx: &H264ParsingContext,
    nals: &[Vec<u8>],
) -> Result<i64> {
    match strategy {
        PocStrategy::FromSliceLsb(decoder) => {
            let (is_idr, nal_ref_idc, poc_lsb) = read_slice_poc_lsb(ctx, nals)?;
            Ok(decoder.next_poc(is_idr, nal_ref_idc, poc_lsb))
        }
        PocStrategy::DecodeOrder { next } => {
            let poc = *next;
            *next += 1;
            Ok(poc)
        }
    }
}

/// Accumulates the H.264 parsing context (SPS/PPS) and the POC strategy as
/// samples are read, so a whole file can be walked (in decode order) and each
/// frame's picture order count reconstructed the same way.
#[derive(Default)]
pub struct PocReader {
    ctx: H264ParsingContext,
    // The POC strategy comes from the SPS's `pic_order_cnt_type`, so it can only
    // be chosen once an SPS has been seen. MP4 keeps SPS/PPS in the container;
    // Annex B streams carry them inline (picked up per-frame).
    strategy: Option<PocStrategy>,
}

impl PocReader {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an SPS: feed it to the parsing context and, on the first one, fix
    /// the POC strategy from its `pic_order_cnt_type`.
    fn put_sps(&mut self, nal: &RefNal<'_>) -> Result<()> {
        let (sps, strategy) = parse_sps(nal)?;
        if self.strategy.is_none() {
            self.strategy = Some(strategy);
        }
        self.ctx.put_seq_param_set(sps);
        Ok(())
    }

    /// Seed SPS/PPS from container-level metadata (MP4). No-op for Annex B,
    /// which carries them inline (handled in [`Self::poc_for_frame`]).
    pub fn seed_from_container<H: SeekableH264Source>(
        &mut self,
        src: &H264Source<H>,
    ) -> Result<()> {
        if let Some(sps_bytes) = src.as_seekable_h264_source().first_sps() {
            let nal = RefNal::new(&sps_bytes, &[], true);
            self.put_sps(&nal)?;
        }
        if let Some(pps_bytes) = src.as_seekable_h264_source().first_pps() {
            let nal = RefNal::new(&pps_bytes, &[], true);
            let pps = PicParameterSet::from_bits(&self.ctx, nal.rbsp_bits())
                .map_err(|e| Error::H264Poc(format!("bad PPS: {e:?}")))?;
            self.ctx.put_pic_param_set(pps);
        }
        Ok(())
    }

    /// Feed any inline SPS/PPS (Annex B) carried with this frame, then return
    /// the frame's POC.
    pub fn poc_for_frame(&mut self, nals: &[Vec<u8>]) -> Result<i64> {
        for nal_bytes in nals {
            let nal = RefNal::new(nal_bytes, &[], true);
            let Ok(header) = nal.header() else { continue };
            match header.nal_unit_type() {
                UnitType::SeqParameterSet => self.put_sps(&nal)?,
                UnitType::PicParameterSet => {
                    let pps = PicParameterSet::from_bits(&self.ctx, nal.rbsp_bits())
                        .map_err(|e| Error::H264Poc(format!("bad PPS: {e:?}")))?;
                    self.ctx.put_pic_param_set(pps);
                }
                _ => {}
            }
        }
        // `self.ctx` and `self.strategy` are disjoint fields, so the immutable
        // borrow of the context and the mutable borrow of the strategy coexist.
        match &mut self.strategy {
            None => Err(Error::H264Poc(
                "slice data appeared before any SPS".to_string(),
            )),
            Some(strategy) => advance_poc(strategy, &self.ctx, nals),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sps_selects_strategy_by_pic_order_cnt_type() {
        // Real SPS NAL units (EBSP, including the NAL header byte) captured from
        // sample recordings. `pic_order_cnt_type == 0` (explicit poc_lsb, can
        // carry B-frame reordering) vs `== 2` (decode order == display order).
        const SPS_TYPE0: &[u8] = &[
            0x67, 0xf4, 0x00, 0x28, 0x91, 0x9b, 0x28, 0x0f, 0x00, 0x44, 0xfc, 0x4c, 0xd9, 0x00,
            0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x03, 0x00, 0x32, 0x0f, 0x18, 0x31, 0x96,
        ];
        const SPS_TYPE2: &[u8] = &[
            0x67, 0x64, 0x44, 0x28, 0xac, 0x4d, 0x00, 0xf0, 0x04, 0x4f, 0xcb, 0x34, 0xb7, 0x00,
            0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x03, 0x00, 0x3c, 0x0f, 0x08, 0x84, 0x6a,
        ];
        let (_, s0) = parse_sps(&RefNal::new(SPS_TYPE0, &[], true)).unwrap();
        assert!(
            matches!(s0, PocStrategy::FromSliceLsb(_)),
            "pic_order_cnt_type 0 should read poc_lsb from slices"
        );
        let (_, s2) = parse_sps(&RefNal::new(SPS_TYPE2, &[], true)).unwrap();
        assert!(
            matches!(s2, PocStrategy::DecodeOrder { .. }),
            "pic_order_cnt_type 2 should fall back to decode order"
        );
    }

    #[test]
    fn poc_decoder_handles_simple_ipbb_gop() {
        let mut dec = PocDecoder::new(4); // MaxPicOrderCntLsb = 256
        // I(ref), P(ref), B(non-ref), B(non-ref), repeating POC pattern
        // typical of an IBBP-style GOP with POC step 2 per displayed frame.
        assert_eq!(dec.next_poc(true, 1, 0), 0); // I, poc 0
        assert_eq!(dec.next_poc(false, 1, 6), 6); // P, poc 6
        assert_eq!(dec.next_poc(false, 0, 2), 2); // B, poc 2
        assert_eq!(dec.next_poc(false, 0, 4), 4); // B, poc 4
    }

    #[test]
    fn poc_decoder_unwraps_lsb_wraparound() {
        let mut dec = PocDecoder::new(0); // MaxPicOrderCntLsb = 16
        // Step by 2 each reference frame, staying well under
        // MaxPicOrderCntLsb/2 (8) per step so no wrap is triggered yet.
        assert_eq!(dec.next_poc(true, 1, 0), 0);
        assert_eq!(dec.next_poc(false, 1, 2), 2);
        assert_eq!(dec.next_poc(false, 1, 4), 4);
        assert_eq!(dec.next_poc(false, 1, 6), 6);
        assert_eq!(dec.next_poc(false, 1, 8), 8);
        assert_eq!(dec.next_poc(false, 1, 10), 10);
        assert_eq!(dec.next_poc(false, 1, 12), 12);
        assert_eq!(dec.next_poc(false, 1, 14), 14);
        // lsb wraps from 14 back down to 0. The raw backward delta (14)
        // meets MaxPicOrderCntLsb/2, so this is really a forward step to
        // poc 16, not a jump back near zero.
        assert_eq!(dec.next_poc(false, 1, 0), 16);
    }
}
