# rtp-h264-streamer

Turns a sequence of camera frames into a live H.264 RTP/UDP stream at
lowest-practical latency, for a fixed, non-negotiating receiver such as

```
gst-launch-1.0 udpsrc port=5600 ! application/x-rtp,encoding-name=H264,payload=96 \
  ! rtph264depay ! h264parse ! avdec_h264 ! autovideosink
```

No RTSP, no SDP exchange, no RTCP, no FEC, no retransmission — a one-way
elementary stream in RTP packets (RFC 6184, via the `h264-rtp` crate).

- **Pluggable encoder** ([`H264StreamEncoder`]): an in-process `openh264`
  encoder, or (once added) an `ffmpeg` sidecar. Both feed encoded access units
  to one long-lived sender thread that owns the RTP session state (SSRC,
  sequence counter, timestamp base) and the UDP socket, so the encoder can be
  swapped or respawned — e.g. on [`RtpH264Streamer::set_bitrate`] — without
  disturbing the session a receiver has already synced to.
- **Runtime bitrate control**: `set_bitrate`/`request_keyframe` are lock-free
  and coalescing (an `AtomicU32`/`AtomicBool` pending-change cell checked at
  the top of each feeder iteration), so they are never dropped even though the
  frame channel itself is lossy (a live stream prefers a dropped stale frame
  over back-pressure on the caller).

See `src/bin/rtp-stream-demo.rs` for a runnable example, and
`tests/loopback.rs` for the manual/CI verification recipe (receiving with a
real ffmpeg process over loopback).
