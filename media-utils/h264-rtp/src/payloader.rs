// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

use rtp_rs::{RtpPacketBuilder, Seq};

use crate::{AccessUnit, Error, Result, RtpSessionConfig};

/// FU-A packet-type code (RFC 6184 §5.8), used as the NAL type field of the FU
/// indicator byte.
const FU_A_TYPE: u8 = 28;

/// Strip a leading 3- or 4-byte Annex-B start code, if present.
///
/// [`AccessUnit`] documents that its NALs should already have any start code
/// stripped, but encoders disagree on this, so [`H264Payloader::packetize`]
/// strips one defensively rather than trusting the caller.
fn strip_start_code(nal: &[u8]) -> &[u8] {
    if nal.starts_with(&[0, 0, 0, 1]) {
        &nal[4..]
    } else if nal.starts_with(&[0, 0, 1]) {
        &nal[3..]
    } else {
        nal
    }
}

/// Convert a presentation time into a 32-bit RTP timestamp, wrapping as RTP
/// timestamps do.
fn rtp_ticks(pts: std::time::Duration, clock_rate: u32, initial_timestamp: u32) -> u32 {
    let ticks: u128 = pts.as_nanos() * clock_rate as u128 / 1_000_000_000;
    // Truncating a u128 to u32 keeps only the low 32 bits, which is exactly
    // the wraparound behavior RTP timestamps require.
    initial_timestamp.wrapping_add(ticks as u32)
}

/// Packetizes [`AccessUnit`]s into RTP datagrams per RFC 6184.
///
/// Holds the RTP session state (sequence counter) that must persist across
/// access units — and, in a streaming context, across encoder restarts.
pub struct H264Payloader {
    cfg: RtpSessionConfig,
    seq: Seq,
    /// Reused across FU-A fragments to assemble `[indicator, header, chunk]`
    /// into one contiguous slice for the RTP builder; never allocates once its
    /// capacity has grown to fit one fragment.
    fu_scratch: Vec<u8>,
    /// Reused across every emitted packet as the RTP builder's output buffer.
    packet_scratch: Vec<u8>,
}

impl H264Payloader {
    pub fn new(cfg: RtpSessionConfig) -> Self {
        let seq = Seq::from(cfg.initial_sequence);
        Self {
            cfg,
            seq,
            fu_scratch: Vec::new(),
            packet_scratch: Vec::new(),
        }
    }

    /// Maximum RTP payload size: `mtu - 28 (IPv4+UDP) - 12 (RTP header)`.
    pub fn payload_budget(&self) -> usize {
        self.cfg.mtu - 28 - 12
    }

    /// Packetize one access unit, invoking `emit` once per RTP datagram.
    ///
    /// Sets the marker bit on the final packet of the access unit only (the
    /// last fragment of the last NAL), and never elsewhere: GStreamer calls
    /// `complete_au()` unconditionally on a marked packet, so a spuriously
    /// early marker splits one real frame into two corrupt ones.
    pub fn packetize(
        &mut self,
        au: &AccessUnit,
        emit: &mut dyn FnMut(&[u8]) -> std::io::Result<()>,
    ) -> Result<()> {
        let budget = self.payload_budget();
        let timestamp = rtp_ticks(au.pts, self.cfg.clock_rate, self.cfg.initial_timestamp);
        let num_nals = au.nals.len();

        for (nal_idx, raw_nal) in au.nals.iter().enumerate() {
            let is_last_nal = nal_idx + 1 == num_nals;
            let nal = strip_start_code(raw_nal);

            let header = *nal.first().ok_or(Error::EmptyNal)?;
            if header & 0x80 != 0 {
                return Err(Error::ForbiddenZeroBit);
            }
            let nal_type = header & 0x1F;
            if !(1..=23).contains(&nal_type) {
                return Err(Error::InvalidNalType(nal_type));
            }

            if nal.len() <= budget {
                self.emit_packet(nal, timestamp, is_last_nal, emit)?;
            } else {
                self.fragment_and_emit(
                    nal,
                    header,
                    nal_type,
                    budget,
                    timestamp,
                    is_last_nal,
                    emit,
                )?;
            }
        }
        Ok(())
    }

    #[expect(clippy::too_many_arguments, reason = "internal helper, not public API")]
    fn fragment_and_emit(
        &mut self,
        nal: &[u8],
        header: u8,
        nal_type: u8,
        budget: usize,
        timestamp: u32,
        is_last_nal: bool,
        emit: &mut dyn FnMut(&[u8]) -> std::io::Result<()>,
    ) -> Result<()> {
        // 1 byte FU indicator + 1 byte FU header per fragment.
        const FU_HEADER_OVERHEAD: usize = 2;
        if budget <= FU_HEADER_OVERHEAD {
            return Err(Error::MtuTooSmall {
                budget,
                min: FU_HEADER_OVERHEAD + 1,
            });
        }
        let frag_cap = budget - FU_HEADER_OVERHEAD;
        // The original NAL header byte is not resent verbatim: its F/NRI bits
        // move into the FU indicator and its type moves into the FU header.
        let payload = &nal[1..];
        let num_fragments = payload.len().div_ceil(frag_cap);
        debug_assert!(
            num_fragments >= 2,
            "a NAL requiring fragmentation must never produce a single FU-A fragment"
        );
        // F and NRI are copied together as the top three bits of the original
        // header; the type field is replaced with the FU-A packet-type code.
        let fu_indicator = (header & 0xE0) | FU_A_TYPE;

        for (frag_idx, chunk) in payload.chunks(frag_cap).enumerate() {
            let start = frag_idx == 0;
            let end = frag_idx + 1 == num_fragments;
            // RFC 6184 §5.8: S and E MUST NOT both be set in the same header.
            debug_assert!(!(start && end));
            let fu_header = ((start as u8) << 7) | ((end as u8) << 6) | nal_type;

            self.fu_scratch.clear();
            self.fu_scratch.push(fu_indicator);
            self.fu_scratch.push(fu_header);
            self.fu_scratch.extend_from_slice(chunk);

            let marker = is_last_nal && end;
            // Work around the borrow checker: `emit_packet` needs `&self.fu_scratch`
            // and `&mut self.packet_scratch` simultaneously, which is fine since
            // they are disjoint fields, but a method call borrows all of `self`.
            let fu_scratch = std::mem::take(&mut self.fu_scratch);
            let result = self.emit_packet(&fu_scratch, timestamp, marker, emit);
            self.fu_scratch = fu_scratch;
            result?;
        }
        Ok(())
    }

    fn emit_packet(
        &mut self,
        payload: &[u8],
        timestamp: u32,
        marker: bool,
        emit: &mut dyn FnMut(&[u8]) -> std::io::Result<()>,
    ) -> Result<()> {
        let builder = RtpPacketBuilder::new()
            .payload_type(self.cfg.payload_type)
            .ssrc(self.cfg.ssrc)
            .sequence(self.seq)
            .timestamp(timestamp)
            .marked(marker)
            .payload(payload);

        let len = builder.target_length();
        if self.packet_scratch.len() < len {
            self.packet_scratch.resize(len, 0);
        }
        let written = builder
            .build_into(&mut self.packet_scratch[..len])
            .map_err(Error::Build)?;
        emit(&self.packet_scratch[..written])?;
        self.seq = self.seq.next();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with_mtu(mtu: usize) -> RtpSessionConfig {
        RtpSessionConfig {
            payload_type: 96,
            ssrc: 0xdead_beef,
            initial_sequence: 0,
            initial_timestamp: 0,
            clock_rate: 90_000,
            mtu,
        }
    }

    fn au(nals: Vec<Vec<u8>>, pts_ms: u64) -> AccessUnit {
        AccessUnit {
            nals,
            is_keyframe: false,
            pts: std::time::Duration::from_millis(pts_ms),
        }
    }

    /// Collects the RTP payloads (header stripped) of every emitted packet.
    fn collect_payloads(payloader: &mut H264Payloader, unit: &AccessUnit) -> Vec<Vec<u8>> {
        let mut payloads = Vec::new();
        payloader
            .packetize(unit, &mut |packet| {
                let reader = rtp_rs::RtpReader::new(packet).unwrap();
                payloads.push(reader.payload().to_vec());
                Ok(())
            })
            .unwrap();
        payloads
    }

    /// A slice NAL (type 1, non-IDR) with the given F/NRI bits and total length,
    /// filled with a deterministic, non-constant byte pattern so any
    /// misframing changes the decoded bytes.
    fn make_nal(nal_type: u8, nri: u8, len: usize) -> Vec<u8> {
        let header = (nri << 5) | nal_type;
        let mut nal = vec![header];
        for i in 0..(len - 1) {
            nal.push((i as u8).wrapping_mul(31).wrapping_add(7));
        }
        nal
    }

    // --- single NAL unit packets -------------------------------------------------

    #[test]
    fn nal_at_exactly_budget_stays_single_packet() {
        let mut p = H264Payloader::new(cfg_with_mtu(1400));
        let budget = p.payload_budget();
        assert_eq!(budget, 1360);
        let nal = make_nal(1, 2, budget);
        let unit = au(vec![nal.clone()], 0);
        let payloads = collect_payloads(&mut p, &unit);
        assert_eq!(payloads.len(), 1);
        assert_eq!(
            payloads[0], nal,
            "single-NAL payload must equal the NAL verbatim"
        );
    }

    #[test]
    fn nal_one_byte_over_budget_becomes_two_fua_fragments() {
        let mut p = H264Payloader::new(cfg_with_mtu(1400));
        let budget = p.payload_budget();
        let nal = make_nal(1, 2, budget + 1);
        let unit = au(vec![nal], 0);
        let payloads = collect_payloads(&mut p, &unit);
        assert_eq!(payloads.len(), 2);
    }

    /// Table-driven fragment-count check across both thresholds (1 vs 2
    /// fragments, and 2 vs 3 fragments), computed from the implementation's own
    /// budget/frag_cap rather than hardcoded arithmetic, so it stays correct if
    /// the MTU or header-overhead accounting ever changes.
    #[test]
    fn fragment_count_thresholds() {
        let mut p = H264Payloader::new(cfg_with_mtu(1400));
        let budget = p.payload_budget();
        let frag_cap = budget - 2;

        let cases: [(usize, usize); 6] = [
            (budget - 1, 1),
            (budget, 1),
            (budget + 1, 2),
            (1 + 2 * frag_cap, 2),
            (1 + 2 * frag_cap + 1, 3),
            (1 + 3 * frag_cap, 3),
        ];
        for (nal_len, expected_packets) in cases {
            let nal = make_nal(1, 2, nal_len);
            let unit = au(vec![nal], 0);
            let payloads = collect_payloads(&mut p, &unit);
            assert_eq!(
                payloads.len(),
                expected_packets,
                "NAL length {nal_len} (budget {budget}, frag_cap {frag_cap})"
            );
        }
    }

    #[test]
    fn fua_reassembles_byte_for_byte() {
        let mut p = H264Payloader::new(cfg_with_mtu(1400));
        let nal = make_nal(5, 3, 5000); // IDR slice, NRI=3
        let unit = au(vec![nal.clone()], 0);

        let mut fragments = Vec::new();
        p.packetize(&unit, &mut |packet| {
            let reader = rtp_rs::RtpReader::new(packet).unwrap();
            fragments.push(reader.payload().to_vec());
            Ok(())
        })
        .unwrap();

        let budget = p.payload_budget();
        let frag_cap = budget - 2;
        assert_eq!(fragments.len(), (nal.len() - 1).div_ceil(frag_cap));

        let mut reassembled = Vec::new();
        for (i, frag) in fragments.iter().enumerate() {
            let fu_indicator = frag[0];
            let fu_header = frag[1];
            let s = fu_header & 0x80 != 0;
            let e = fu_header & 0x40 != 0;
            assert_eq!(s, i == 0);
            assert_eq!(e, i + 1 == fragments.len());
            assert!(!(s && e));
            if i == 0 {
                let original_header = (fu_indicator & 0xE0) | (fu_header & 0x1F);
                reassembled.push(original_header);
            }
            reassembled.extend_from_slice(&frag[2..]);
        }
        assert_eq!(reassembled, nal);
    }

    #[test]
    fn fu_indicator_and_header_preserve_f_nri_and_type() {
        let mut p = H264Payloader::new(cfg_with_mtu(1400));
        let budget = p.payload_budget();
        // F=0, NRI=3, type=5 (IDR slice): header = 0b0_11_00101 = 0x65.
        let nal = make_nal(5, 0b11, budget + 100);
        let unit = au(vec![nal], 0);

        let mut first = None;
        p.packetize(&unit, &mut |packet| {
            if first.is_none() {
                let reader = rtp_rs::RtpReader::new(packet).unwrap();
                first = Some(reader.payload().to_vec());
            }
            Ok(())
        })
        .unwrap();
        let frag = first.unwrap();
        assert_eq!(
            frag[0] & 0xE0,
            0b011_00000,
            "F/NRI preserved in FU indicator"
        );
        assert_eq!(frag[0] & 0x1F, FU_A_TYPE, "FU indicator type is 28");
        assert_eq!(frag[1] & 0x1F, 5, "FU header preserves original NAL type");
    }

    // --- sequence numbers ---------------------------------------------------

    #[test]
    fn sequence_numbers_increment_across_u16_wrap() {
        let mut cfg = cfg_with_mtu(1400);
        cfg.initial_sequence = 65534;
        let mut p = H264Payloader::new(cfg);
        let unit = au(
            vec![make_nal(7, 3, 10), make_nal(8, 3, 10), make_nal(1, 2, 10)],
            0,
        );

        let mut seqs = Vec::new();
        p.packetize(&unit, &mut |packet| {
            let reader = rtp_rs::RtpReader::new(packet).unwrap();
            seqs.push(u16::from(reader.sequence_number()));
            Ok(())
        })
        .unwrap();
        assert_eq!(seqs, vec![65534, 65535, 0]);
    }

    // --- marker bit ----------------------------------------------------------

    #[test]
    fn exactly_one_marked_packet_and_it_is_last() {
        let mut p = H264Payloader::new(cfg_with_mtu(1400));
        let budget = p.payload_budget();
        // SPS, PPS, then an oversized slice fragmented into several packets.
        let unit = au(
            vec![
                make_nal(7, 3, 20),
                make_nal(8, 3, 10),
                make_nal(1, 2, budget + 500),
            ],
            0,
        );

        let mut marks = Vec::new();
        p.packetize(&unit, &mut |packet| {
            let reader = rtp_rs::RtpReader::new(packet).unwrap();
            marks.push(reader.mark());
            Ok(())
        })
        .unwrap();

        assert_eq!(marks.iter().filter(|m| **m).count(), 1);
        assert!(*marks.last().unwrap());
    }

    #[test]
    fn sps_pps_are_separate_unmarked_single_nal_packets() {
        let mut p = H264Payloader::new(cfg_with_mtu(1400));
        let unit = au(
            vec![make_nal(7, 3, 20), make_nal(8, 3, 10), make_nal(5, 3, 30)],
            0,
        );

        let mut marks = Vec::new();
        let mut types = Vec::new();
        p.packetize(&unit, &mut |packet| {
            let reader = rtp_rs::RtpReader::new(packet).unwrap();
            marks.push(reader.mark());
            types.push(reader.payload()[0] & 0x1F);
            Ok(())
        })
        .unwrap();

        assert_eq!(types, vec![7, 8, 5], "SPS, then PPS, then slice");
        assert_eq!(marks, vec![false, false, true]);
    }

    // --- timestamps ----------------------------------------------------------

    #[test]
    fn one_timestamp_per_au_and_differs_between_aus() {
        let mut p = H264Payloader::new(cfg_with_mtu(1400));
        let budget = p.payload_budget();
        let unit_a = au(vec![make_nal(7, 3, 10), make_nal(1, 2, budget + 200)], 0);
        let unit_b = au(vec![make_nal(1, 2, 10)], 33);

        let mut ts_a = Vec::new();
        p.packetize(&unit_a, &mut |packet| {
            ts_a.push(rtp_rs::RtpReader::new(packet).unwrap().timestamp());
            Ok(())
        })
        .unwrap();
        assert!(
            ts_a.windows(2).all(|w| w[0] == w[1]),
            "one AU, one timestamp"
        );

        let mut ts_b = Vec::new();
        p.packetize(&unit_b, &mut |packet| {
            ts_b.push(rtp_rs::RtpReader::new(packet).unwrap().timestamp());
            Ok(())
        })
        .unwrap();
        assert_ne!(ts_a[0], ts_b[0], "timestamps must differ between AUs");
    }

    // --- Annex-B start code stripping ----------------------------------------

    #[test]
    fn three_and_four_byte_start_codes_are_both_stripped() {
        let bare = make_nal(1, 2, 20);
        let mut with_3byte = vec![0, 0, 1];
        with_3byte.extend_from_slice(&bare);
        let mut with_4byte = vec![0, 0, 0, 1];
        with_4byte.extend_from_slice(&bare);

        let mut p1 = H264Payloader::new(cfg_with_mtu(1400));
        let payloads_bare = collect_payloads(&mut p1, &au(vec![bare.clone()], 0));

        let mut p2 = H264Payloader::new(cfg_with_mtu(1400));
        let payloads_3 = collect_payloads(&mut p2, &au(vec![with_3byte], 0));

        let mut p3 = H264Payloader::new(cfg_with_mtu(1400));
        let payloads_4 = collect_payloads(&mut p3, &au(vec![with_4byte], 0));

        assert_eq!(payloads_bare, payloads_3);
        assert_eq!(payloads_bare, payloads_4);
    }

    // --- rejected input --------------------------------------------------------

    #[test]
    fn bare_stap_a_and_reserved_types_are_rejected() {
        for bad_type in [0u8, 24, 30, 31] {
            let mut p = H264Payloader::new(cfg_with_mtu(1400));
            let nal = make_nal(bad_type, 0, 10);
            let unit = au(vec![nal], 0);
            let mut called = false;
            let result = p.packetize(&unit, &mut |_packet| {
                called = true;
                Ok(())
            });
            assert!(!called, "must not emit for NAL type {bad_type}");
            assert!(matches!(result, Err(Error::InvalidNalType(t)) if t == bad_type));
        }
    }

    #[test]
    fn zero_length_nal_is_rejected() {
        let mut p = H264Payloader::new(cfg_with_mtu(1400));
        let unit = au(vec![Vec::new()], 0);
        let result = p.packetize(&unit, &mut |_packet| Ok(()));
        assert!(matches!(result, Err(Error::EmptyNal)));
    }

    #[test]
    fn forbidden_zero_bit_is_rejected() {
        let mut p = H264Payloader::new(cfg_with_mtu(1400));
        let mut nal = make_nal(1, 2, 10);
        nal[0] |= 0x80; // set forbidden_zero_bit
        let unit = au(vec![nal], 0);
        let result = p.packetize(&unit, &mut |_packet| Ok(()));
        assert!(matches!(result, Err(Error::ForbiddenZeroBit)));
    }
}
