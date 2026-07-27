// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! RFC 6184 H.264-over-RTP packetization.
//!
//! This crate is pure: no I/O, no threads, no encoder. It turns one encoded
//! [`AccessUnit`] into a sequence of RTP datagrams, RFC 6184 §5.6 (single NAL
//! unit packets) and §5.8 (FU-A fragmentation) only — STAP-A is deliberately
//! not implemented (the RFC makes it MAY, not MUST). See [`payloader`] for the
//! packetizer itself and the unit tests that cross-check this implementation
//! against the RFC, GStreamer's `gstrtph264depay.c`, and FFmpeg's
//! `rtpdec_h264.c`.

mod payloader;

pub use payloader::H264Payloader;

/// One encoded access unit: the NALs of a single frame.
///
/// Each entry of `nals` starts at its NAL header byte; any Annex-B start code
/// must already be stripped (though [`H264Payloader::packetize`] also strips
/// one defensively, since encoders disagree on whether they include it).
#[derive(Debug, Clone)]
pub struct AccessUnit {
    /// The NAL units of this access unit, in wire order (SPS, PPS, \[SEI\],
    /// slice(s)).
    pub nals: Vec<Vec<u8>>,
    /// Whether this access unit is a keyframe (IDR).
    pub is_keyframe: bool,
    /// Presentation time since stream start.
    pub pts: std::time::Duration,
}

/// Configuration for one RTP session.
///
/// SSRC, initial sequence number and initial RTP timestamp default to random
/// values per RFC 3550 §5.1 ("This is done to make known-plaintext attacks on
/// SRTP more difficult"). That rationale does not apply to this unencrypted
/// use, but the random default costs nothing and future-proofs an encrypted
/// link, while overriding with a fixed value buys deterministic pcaps and log
/// correlation for tests and debugging.
#[derive(Debug, Clone)]
pub struct RtpSessionConfig {
    /// RTP payload type (PT). Default 96 (the conventional first dynamic PT).
    pub payload_type: u8,
    /// Synchronization source identifier. Default: random.
    pub ssrc: u32,
    /// Sequence number of the first packet emitted. Default: random.
    pub initial_sequence: u16,
    /// RTP timestamp corresponding to `pts == Duration::ZERO`. Default: random.
    pub initial_timestamp: u32,
    /// RTP clock rate in Hz. Default 90_000, the conventional H.264 rate.
    pub clock_rate: u32,
    /// Path MTU in bytes, used to derive [`H264Payloader::payload_budget`].
    /// Default 1400.
    pub mtu: usize,
}

impl Default for RtpSessionConfig {
    fn default() -> Self {
        Self {
            payload_type: 96,
            ssrc: rand::random(),
            initial_sequence: rand::random(),
            initial_timestamp: rand::random(),
            clock_rate: 90_000,
            mtu: 1400,
        }
    }
}

/// Errors from [`H264Payloader::packetize`].
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A NAL in the access unit had zero bytes.
    #[error("empty NAL unit")]
    EmptyNal,
    /// `forbidden_zero_bit` was set on an ingested NAL. No depayloader checks
    /// this, so forwarding it would corrupt silently; from our own encoder it
    /// should be impossible, so seeing it indicates a start-code scanning or
    /// buffer-tearing bug upstream.
    #[error("NAL has forbidden_zero_bit set (start-code scanning or buffer-tearing bug upstream)")]
    ForbiddenZeroBit,
    /// A bare NAL of a type that is not a valid single NAL unit type (RFC 6184
    /// Table 1 restricts these to 1-23; 24-31 are packet-type codes and 0 is
    /// unused). Forwarding one bare would make a depayloader misparse the
    /// payload as STAP/FU headers.
    #[error("NAL type {0} is not a valid single NAL unit type (RFC 6184 Table 1: must be 1-23)")]
    InvalidNalType(u8),
    /// The configured MTU leaves too little payload budget to carry even one
    /// byte of an FU-A fragment (2 bytes of every FU-A packet are the FU
    /// indicator and FU header).
    #[error("MTU too small for FU-A: payload budget is {budget} bytes, need at least {min}")]
    MtuTooSmall {
        /// The payload budget computed from the configured MTU.
        budget: usize,
        /// The minimum budget FU-A fragmentation requires.
        min: usize,
    },
    /// Building the RTP packet itself failed.
    #[error("RTP packet build error: {0:?}")]
    Build(rtp_rs::RtpPacketBuildError),
    /// The `emit` callback returned an error.
    #[error("IO error emitting RTP packet: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
