# h264-rtp

RFC 6184 H.264-over-RTP packetization, pure and dependency-light: no I/O, no
threads, no encoder. Given one encoded access unit (the NAL units of a single
frame), [`H264Payloader::packetize`] emits one or more RTP datagrams via a
caller-supplied callback.

- NALs that fit the configured MTU go out as **single NAL unit packets**
  (RFC 6184 §5.6): the payload is the NAL verbatim, Annex-B start code
  stripped.
- Oversized NALs are split into **FU-A** fragments (RFC 6184 §5.8).
- STAP-A is deliberately not implemented (optional per the RFC).
- The marker bit is set on exactly one packet per access unit: the last
  packet of the last NAL.

See `src/payloader.rs` for the RFC cross-checks this implementation embeds as
unit tests (NAL-type validation, FU-A fragment boundaries, sequence-number
wraparound, marker-bit placement, and Annex-B start-code stripping).

This crate does not open sockets or spawn processes; see the `rtp-h264-streamer`
crate for a threaded UDP sender built on top of this payloader.
