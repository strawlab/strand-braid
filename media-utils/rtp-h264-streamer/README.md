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
  encoder, or an `ffmpeg` sidecar (the default). Both feed encoded access units
  to one long-lived sender thread that owns the RTP session state (SSRC,
  sequence counter, timestamp base) and the UDP socket, so the encoder can be
  swapped or respawned — e.g. on [`RtpH264Streamer::set_bitrate`] — without
  disturbing the session a receiver has already synced to.
- **Runtime bitrate control**: `set_bitrate`/`request_keyframe` are lock-free
  and coalescing (an `AtomicU32`/`AtomicBool` pending-change cell checked at
  the top of each feeder iteration), so they are never dropped even though the
  frame channel itself is lossy (a live stream prefers a dropped stale frame
  over back-pressure on the caller). Both encoder backends respond to a
  bitrate change by respawning/reinitializing rather than mutating the running
  encoder in place — required for the openh264 backend, whose native bitrate
  option only accepts decreases once initialized — which conveniently also
  gives the receiver a fresh IDR on every change.

## `rtp-stream-demo`

A runnable demo/verification binary (not part of any release bundle):

```
cargo run -p rtp-h264-streamer --bin rtp-stream-demo -- \
  --dest 127.0.0.1:5600 --encoder ffmpeg|openh264 \
  --bitrate-kbps 4000 --fps 30 --mtu 1400 --idr-interval 30 \
  [--size 1280x720 | --input FILE] [--ramp-bitrate] [--dump-annexb out.h264]
```

With no `--input`, it streams a synthetic moving test pattern (no assets
needed). Point a receiver such as the `gst-launch-1.0` pipeline above, or
`ffplay`, at `--dest`; verify the elementary stream independently with
`--dump-annexb out.h264` followed by `ffprobe`/`ffplay out.h264`.

See `tests/loopback.rs` for the manual/CI-optional integration test (receives
with a real `ffmpeg` process over loopback): `cargo test -p rtp-h264-streamer
-- --ignored`.
