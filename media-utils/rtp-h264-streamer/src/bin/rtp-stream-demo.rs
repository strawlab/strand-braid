// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Demo CLI proving `rtp-h264-streamer` end-to-end: streams either a
//! synthetic moving test pattern or a real video file as H.264 over RTP/UDP.
//!
//! Not part of any release bundle (deliberately absent from
//! `_packaging/gen-strand-braid-third-party-licenses.sh` and the Debian
//! `.install` list), since it exists only to exercise and verify the library.

use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use clap::Parser;
use machine_vision_formats::PixFmt;
use rtp_h264_streamer::{
    EncoderKind, FfmpegEncoderConfig, OpenH264EncoderConfig, RtpH264Streamer, RtpSessionConfig,
    StreamConfig,
};
use strand_dynamic_frame::DynamicFrameOwned;

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
enum EncoderChoice {
    Ffmpeg,
    Openh264,
}

/// Stream H.264 over RTP/UDP for a fixed, non-negotiating receiver, e.g.:
///
/// gst-launch-1.0 udpsrc port=5600 ! application/x-rtp,encoding-name=H264,payload=96 \
///   ! rtph264depay ! h264parse ! avdec_h264 ! autovideosink
#[derive(clap::Parser, Debug)]
#[command(verbatim_doc_comment)]
struct Args {
    /// Destination host:port for the RTP/UDP datagrams.
    #[arg(long)]
    dest: SocketAddr,
    /// Encoder backend.
    #[arg(long, value_enum, default_value_t = EncoderChoice::Ffmpeg)]
    encoder: EncoderChoice,
    /// Initial target bitrate in kbps.
    #[arg(long, default_value_t = 4000)]
    bitrate_kbps: u32,
    /// Frame rate in Hz, both for pacing the source and for the encoder's
    /// `-framerate`/timestamp arithmetic.
    #[arg(long, default_value_t = 30.0)]
    fps: f32,
    /// Path MTU in bytes.
    #[arg(long, default_value_t = 1400)]
    mtu: usize,
    /// Interval, in frames, between forced keyframes (IDR).
    #[arg(long, default_value_t = 30)]
    idr_interval: u32,
    /// Synthetic test-pattern size, `WxH`. Ignored when `--input` is given.
    #[arg(long, default_value = "1280x720")]
    size: String,
    /// Read frames from a real video file instead of the synthetic test
    /// pattern (looped once it runs out).
    #[arg(long)]
    input: Option<PathBuf>,
    /// Cycle the bitrate every few seconds, to demonstrate live bitrate
    /// control without restarting the RTP session.
    #[arg(long)]
    ramp_bitrate: bool,
    /// Tee the raw Annex-B elementary stream to this file, for offline
    /// `ffprobe`/decoder verification independent of the RTP framing.
    #[arg(long)]
    dump_annexb: Option<PathBuf>,
}

/// A moving white bar over a position-dependent gradient, so both frame
/// progression (the bar's motion) and any spatial corruption (the gradient)
/// are visible without any input assets.
fn synthetic_frame(width: u32, height: u32, frame_idx: u64) -> Arc<DynamicFrameOwned> {
    let stride = width as usize * 3;
    let mut buf = vec![0u8; stride * height as usize];
    let bar_width = (width / 8).max(1);
    let offset = (frame_idx as u32).wrapping_mul(4) % width.max(1);
    for y in 0..height {
        for x in 0..width {
            let i = y as usize * stride + x as usize * 3;
            let in_bar = (x + offset) % width.max(1) < bar_width;
            let (r, g, b) = if in_bar {
                (255u8, 255u8, 255u8)
            } else {
                (
                    (x * 255 / width.max(1)) as u8,
                    (y * 255 / height.max(1)) as u8,
                    (frame_idx % 256) as u8,
                )
            };
            buf[i] = r;
            buf[i + 1] = g;
            buf[i + 2] = b;
        }
    }
    Arc::new(DynamicFrameOwned::from_buf(width, height, stride, buf, PixFmt::RGB8).unwrap())
}

fn parse_size(s: &str) -> anyhow::Result<(u32, u32)> {
    let (w, h) = s
        .split_once('x')
        .ok_or_else(|| anyhow::anyhow!("--size must be WxH, e.g. 1280x720"))?;
    Ok((w.parse()?, h.parse()?))
}

/// A handful of bitrates to cycle through under `--ramp-bitrate`, chosen to
/// span a visibly different range (250 kbps to 8 Mbps) rather than a subtle one.
const RAMP_BITRATES_KBPS: &[u32] = &[250, 1_000, 4_000, 8_000];
const RAMP_PERIOD: Duration = Duration::from_secs(5);

fn main() -> anyhow::Result<()> {
    let _guard = env_tracing_logger::init();
    let args = Args::parse();

    let encoder = match args.encoder {
        EncoderChoice::Ffmpeg => EncoderKind::Ffmpeg(FfmpegEncoderConfig::default()),
        EncoderChoice::Openh264 => EncoderKind::OpenH264(OpenH264EncoderConfig::default()),
    };

    let cfg = StreamConfig {
        dest: args.dest,
        bitrate_kbps: args.bitrate_kbps,
        fps: args.fps,
        idr_interval_frames: args.idr_interval,
        encoder,
        rtp: RtpSessionConfig {
            mtu: args.mtu,
            ..Default::default()
        },
        queue_size: 8,
        dump_annexb: args.dump_annexb.clone()
    };
    let mut streamer = RtpH264Streamer::new(cfg)?;
    tracing::info!(
        "streaming to {} via {:?} at {} kbps, {} fps",
        args.dest,
        args.encoder,
        args.bitrate_kbps,
        args.fps
    );

    let frame_period = Duration::from_secs_f32(1.0 / args.fps.max(1.0));
    let start = Instant::now();
    let mut next_ramp_at = RAMP_PERIOD;
    let mut ramp_idx = 0usize;

    let mut frame_idx: u64 = 0;
    let mut next_frame_at = Instant::now();

    match args.input {
        None => {
            let (width, height) = parse_size(&args.size)?;
            loop {
                maybe_ramp_bitrate(
                    &mut streamer,
                    args.ramp_bitrate,
                    start,
                    &mut next_ramp_at,
                    &mut ramp_idx,
                )?;
                let frame = synthetic_frame(width, height, frame_idx);
                streamer.send(frame, chrono::Local::now())?;
                frame_idx += 1;
                pace(&mut next_frame_at, frame_period);
            }
        }
        Some(input) => loop {
            let mut source = frame_source::FrameSourceBuilder::new(&input).build_source()?;
            for frame_data in source.presentation_order_iter()? {
                let frame_data = frame_data?;
                let Some(frame) = frame_data.take_decoded() else {
                    continue; // an encoded (non-decoded) frame; skip it.
                };
                maybe_ramp_bitrate(
                    &mut streamer,
                    args.ramp_bitrate,
                    start,
                    &mut next_ramp_at,
                    &mut ramp_idx,
                )?;
                streamer.send(Arc::new(frame), chrono::Local::now())?;
                frame_idx += 1;
                pace(&mut next_frame_at, frame_period);
            }
            tracing::info!("input file exhausted after {frame_idx} frames; looping");
        },
    }
}

fn pace(next_frame_at: &mut Instant, frame_period: Duration) {
    *next_frame_at += frame_period;
    let now = Instant::now();
    if *next_frame_at > now {
        std::thread::sleep(*next_frame_at - now);
    } else {
        // We've fallen behind (e.g. encoder respawn glitch): resync to now
        // rather than trying to burst-catch-up.
        *next_frame_at = now;
    }
}

fn maybe_ramp_bitrate(
    streamer: &mut RtpH264Streamer,
    enabled: bool,
    start: Instant,
    next_ramp_at: &mut Duration,
    ramp_idx: &mut usize,
) -> anyhow::Result<()> {
    if !enabled {
        return Ok(());
    }
    if start.elapsed() < *next_ramp_at {
        return Ok(());
    }
    *ramp_idx = (*ramp_idx + 1) % RAMP_BITRATES_KBPS.len();
    let kbps = RAMP_BITRATES_KBPS[*ramp_idx];
    tracing::info!("ramping bitrate to {} kbps", kbps);
    streamer.set_bitrate_kbps(kbps)?;
    *next_ramp_at += RAMP_PERIOD;
    Ok(())
}
