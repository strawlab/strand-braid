// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Manual/CI-optional integration test: streams synthetic frames over real
//! RTP/UDP loopback and decodes them with a real `ffmpeg` receiver process (an
//! independent RTP depayloader and H.264 decoder), proving the wire format
//! itself is correct rather than only our own parsing of it.
//!
//! Ignored by default -- CI machines may lack `ffmpeg`. Run explicitly with:
//!
//!     cargo test -p rtp-h264-streamer -- --ignored

use std::{
    net::SocketAddr,
    process::{Command, Stdio},
    sync::Arc,
    time::Duration,
};

use machine_vision_formats::PixFmt;
use rtp_h264_streamer::{
    EncoderKind, FfmpegEncoderConfig, RtpH264Streamer, RtpSessionConfig, StreamConfig,
};
use strand_dynamic_frame::DynamicFrameOwned;

/// Find a free UDP port by binding an ephemeral socket and immediately
/// dropping it. There is a small race window (another process could grab the
/// port before ffmpeg binds it), acceptable for a manual/ignored test.
fn free_udp_port() -> u16 {
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    socket.local_addr().unwrap().port()
}

#[test]
#[ignore = "requires a real ffmpeg binary; run with `cargo test -p rtp-h264-streamer -- --ignored`"]
fn loopback_stream_decodes_with_real_ffmpeg() {
    let port = free_udp_port();
    let dest: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();

    // The exact SDP recipe from the plan's Stage-0 verification: no RTSP, no
    // negotiation, just enough for ffmpeg to know the payload type and clock
    // rate of the RTP stream it's about to receive.
    let tmp = tempfile::tempdir().unwrap();
    let sdp_path = tmp.path().join("loopback.sdp");
    std::fs::write(
        &sdp_path,
        format!(
            "v=0\no=- 0 0 IN IP4 127.0.0.1\nc=IN IP4 127.0.0.1\nt=0 0\nm=video {port} RTP/AVP 96\na=rtpmap:96 H264/90000\n"
        ),
    )
    .unwrap();

    let mut receiver = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-nostdin",
            "-protocol_whitelist",
            "file,rtp,udp",
            "-fflags",
            "nobuffer",
            "-flags",
            "low_delay",
            "-analyzeduration",
            "0",
            "-probesize",
            "32",
            "-i",
        ])
        .arg(&sdp_path)
        .args(["-f", "null", "-"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawning ffmpeg receiver (is ffmpeg installed and on PATH?)");

    // Give the receiver a moment to open its socket before we start sending.
    std::thread::sleep(Duration::from_millis(300));

    let cfg = StreamConfig {
        dest,
        bitrate_kbps: 1_000,
        fps: 30.0,
        idr_interval_frames: 30,
        encoder: EncoderKind::Ffmpeg(FfmpegEncoderConfig::default()),
        rtp: RtpSessionConfig::default(),
        queue_size: 8,
        dump_annexb: None,
    };
    let mut streamer = RtpH264Streamer::new(cfg).unwrap();

    let (width, height) = (176u32, 144u32);
    let mut ts = chrono::Local::now();
    for frame_idx in 0..60u32 {
        let mut buf = vec![0u8; (width * height * 3) as usize];
        for b in buf.iter_mut() {
            *b = frame_idx as u8;
        }
        let frame = Arc::new(
            DynamicFrameOwned::from_buf(width, height, (width * 3) as usize, buf, PixFmt::RGB8)
                .unwrap(),
        );
        streamer.send(frame, ts).unwrap();
        ts += chrono::Duration::milliseconds(33);
        std::thread::sleep(Duration::from_millis(33));
    }
    streamer.finish().unwrap();

    // Let the receiver catch up, then stop it: RTP/UDP has no end-of-stream
    // signal, so ffmpeg would otherwise wait forever for more packets.
    std::thread::sleep(Duration::from_millis(500));
    let _ = receiver.kill();
    let output = receiver.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);

    // ffmpeg's default progress reporting prints periodic
    // "frame=   N fps=... " lines to stderr; the largest N observed is the
    // decoded frame count.
    let decoded_frames = stderr
        .split("frame=")
        .skip(1)
        .filter_map(|s| s.split_whitespace().next())
        .filter_map(|n| n.parse::<u32>().ok())
        .max()
        .unwrap_or(0);

    assert!(
        decoded_frames > 0,
        "expected a nonzero decoded frame count from ffmpeg's real RTP/H.264 depayloader and decoder; stderr:\n{stderr}"
    );
}
