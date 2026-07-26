// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Low-latency H.264 RTP/UDP streaming with pluggable encoders and runtime
//! bitrate control.
//!
//! [`RtpH264Streamer`] turns a sequence of frames into an H.264 RTP/UDP
//! stream, mirroring [`bg_movie_writer::BgMovieWriter`]'s threading pattern: a
//! background feeder thread receives frames over a bounded, lossy channel
//! (dropping a frame rather than blocking the caller when the queue is full),
//! encodes them, and hands encoded access units to a separate sender thread
//! that owns the RTP session state (SSRC, sequence counter, 90 kHz timestamp
//! base) and the UDP socket. Keeping the RTP session in its own thread, fed
//! by a channel rather than by the encoder directly, means the encoder can be
//! swapped or respawned (on [`RtpH264Streamer::set_bitrate`]) without
//! disturbing the RTP session the receiver has already synced to.
//!
//! See [`encoder::H264StreamEncoder`] for the pluggable encoder seam.

mod encoder;
mod encoder_ffmpeg;
mod encoder_openh264;
mod sender;

pub use encoder::H264StreamEncoder;
pub use encoder_ffmpeg::FfmpegEncoderConfig;
pub use encoder_openh264::OpenH264EncoderConfig;
pub use h264_rtp::RtpSessionConfig;

use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc::{Receiver, SyncSender},
    },
};

use h264_rtp::AccessUnit;

/// Access units are buffered between the encoder and the sender thread with a
/// small, blocking (not lossy) queue: unlike a dropped raw input frame, a
/// dropped *encoded* access unit corrupts every frame after it until the next
/// keyframe, since P-frames reference their predecessors. A slow network sender
/// should apply backpressure to the encoder instead.
const AU_QUEUE_SIZE: usize = 4;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("h264-rtp error: {0}")]
    H264Rtp(#[from] h264_rtp::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("openh264 error: {0}")]
    OpenH264(#[from] openh264::Error),
    #[error("y4m-writer error: {0}")]
    Y4mWriter(#[from] y4m_writer::Error),
    #[error("ffmpeg-writer error: {0}")]
    FfmpegWriter(#[from] ffmpeg_writer::Error),
    #[error("the encoder's access-unit channel receiver is gone")]
    SenderDisconnected,
    #[error("the streamer's worker thread is gone")]
    WorkerDisconnected,
    #[error("already finished")]
    AlreadyDone,
    #[error("bitrate must be nonzero")]
    ZeroBitrate,
}

pub type Result<T> = std::result::Result<T, Error>;

/// Which H.264 encoder backend to use, and its backend-specific settings.
///
/// Bitrate, frame rate and the intra-frame period are shared across backends
/// (see [`StreamConfig`]) since they mean the same thing regardless of which
/// encoder produces the bitstream.
pub enum EncoderKind {
    /// In-process encoding via the local `openh264-rs` fork.
    OpenH264(OpenH264EncoderConfig),
    /// An `ffmpeg` child process, spawned lazily on the first frame and
    /// respawned on `set_bitrate`/`request_keyframe`.
    Ffmpeg(FfmpegEncoderConfig),
}

/// Configuration for one RTP H.264 stream.
pub struct StreamConfig {
    /// Destination address for the RTP/UDP datagrams.
    pub dest: std::net::SocketAddr,
    /// Initial target bitrate in kilo bits per second.
    pub bitrate_kbps: u32,
    /// Source frame rate. Passed to the ffmpeg backend as `-framerate` and
    /// used to size its VBV `-bufsize`; the openh264 backend does not need it.
    pub fps: f32,
    /// Interval, in frames, between forced keyframes (IDR). `0` lets the
    /// encoder decide.
    pub idr_interval_frames: u32,
    /// Which encoder backend to use.
    pub encoder: EncoderKind,
    /// RTP session parameters (SSRC, sequence base, MTU, ...).
    pub rtp: RtpSessionConfig,
    /// Number of frames buffered in [`RtpH264Streamer::send`]'s channel before
    /// frames are dropped.
    pub queue_size: usize,
    /// If set, tee the raw Annex-B elementary stream (as sent to the encoder's
    /// packetizer) to this file, for offline `ffprobe`/decoder verification
    /// independent of the RTP framing.
    pub dump_annexb: Option<PathBuf>,
}

enum Msg {
    Write(
        Arc<strand_dynamic_frame::DynamicFrameOwned>,
        chrono::DateTime<chrono::Local>,
    ),
    Finish,
}

/// Recover from a poisoned lock rather than panicking: a panic here would
/// propagate out of `send`/`finish` and take down the calling thread.
macro_rules! poll_err {
    ($err_rx:expr) => {{
        if let Some(e) = $err_rx.lock().unwrap_or_else(|e| e.into_inner()).take() {
            return Err(e);
        }
    }};
}

/// Streams frames as an H.264 RTP/UDP stream in a background thread.
///
/// [`Self::new`] spawns the worker threads; [`Self::send`] and
/// [`Self::finish`] return immediately, even though their work is not done yet.
pub struct RtpH264Streamer {
    tx: SyncSender<Msg>,
    pending_bitrate_bps: Arc<AtomicU32>,
    keyframe_requested: Arc<AtomicBool>,
    is_done: bool,
    err_from_worker: Arc<Mutex<Option<Error>>>,
}

impl RtpH264Streamer {
    pub fn new(cfg: StreamConfig) -> Result<Self> {
        let err_to_worker = Arc::new(Mutex::new(None));
        let err_from_worker = err_to_worker.clone();
        let pending_bitrate_bps = Arc::new(AtomicU32::new(0));
        let keyframe_requested = Arc::new(AtomicBool::new(false));
        let (tx, rx) = std::sync::mpsc::sync_channel::<Msg>(cfg.queue_size);

        let feeder_pending_bitrate = pending_bitrate_bps.clone();
        let feeder_keyframe_requested = keyframe_requested.clone();
        std::thread::spawn(move || {
            if let Err(e) = run_feeder(cfg, rx, feeder_pending_bitrate, feeder_keyframe_requested) {
                *err_to_worker.lock().unwrap_or_else(|e| e.into_inner()) = Some(e);
            }
        });

        Ok(Self {
            tx,
            pending_bitrate_bps,
            keyframe_requested,
            is_done: false,
            err_from_worker,
        })
    }

    /// Enqueue a frame for encoding and sending.
    ///
    /// Non-blocking; drops the frame (with a [`tracing::warn!`]) if the queue
    /// is full, since this is a live stream and a stale frame is worse than a
    /// skipped one.
    pub fn send<TS>(
        &mut self,
        frame: Arc<strand_dynamic_frame::DynamicFrameOwned>,
        timestamp: TS,
    ) -> Result<()>
    where
        TS: Into<chrono::DateTime<chrono::Local>>,
    {
        poll_err!(self.err_from_worker);
        if self.is_done {
            return Err(Error::AlreadyDone);
        }
        let msg = Msg::Write(frame, timestamp.into());
        match self.tx.try_send(msg) {
            Ok(()) => {}
            Err(std::sync::mpsc::TrySendError::Full(_msg)) => {
                tracing::warn!("dropping frame to stream: queue full");
            }
            Err(std::sync::mpsc::TrySendError::Disconnected(_msg)) => {
                return Err(Error::WorkerDisconnected);
            }
        }
        Ok(())
    }

    /// Request a new target bitrate. Coalescing: if called again before the
    /// feeder thread has applied a pending change, the newer value wins and
    /// the older one is never applied. Never dropped, unlike `send`.
    pub fn set_bitrate(&mut self, bps: u32) -> Result<()> {
        poll_err!(self.err_from_worker);
        // 0 is reserved to mean "no pending change"; reject it rather than
        // silently ignoring a real request.
        if bps == 0 {
            return Err(Error::ZeroBitrate);
        }
        self.pending_bitrate_bps.store(bps, Ordering::Release);
        Ok(())
    }

    /// Request that the next encoded frame be a keyframe (IDR).
    pub fn request_keyframe(&mut self) -> Result<()> {
        poll_err!(self.err_from_worker);
        self.keyframe_requested.store(true, Ordering::Release);
        Ok(())
    }

    /// Flush and shut down the stream.
    pub fn finish(&mut self) -> Result<()> {
        poll_err!(self.err_from_worker);
        self.is_done = true;
        let tx = self.tx.clone();
        // Send the finish message from a new thread so a full queue can't
        // drop it and so this call never blocks the caller.
        std::thread::spawn(move || {
            if tx.send(Msg::Finish).is_err() {
                tracing::debug!("feeder thread already gone; nothing to finish");
            }
        });
        Ok(())
    }
}

fn make_encoder(
    encoder_kind: EncoderKind,
    bitrate_bps: u32,
    fps: f32,
    idr_interval_frames: u32,
    payload_budget: usize,
    au_tx: SyncSender<AccessUnit>,
) -> Result<Box<dyn H264StreamEncoder>> {
    match encoder_kind {
        EncoderKind::OpenH264(cfg) => Ok(Box::new(encoder_openh264::OpenH264StreamEncoder::new(
            cfg,
            bitrate_bps,
            idr_interval_frames,
            payload_budget,
            au_tx,
        )?)),
        EncoderKind::Ffmpeg(cfg) => Ok(Box::new(encoder_ffmpeg::FfmpegStreamEncoder::new(
            cfg,
            bitrate_bps,
            fps,
            idr_interval_frames,
            au_tx,
        )?)),
    }
}

fn run_feeder(
    cfg: StreamConfig,
    rx: Receiver<Msg>,
    pending_bitrate_bps: Arc<AtomicU32>,
    keyframe_requested: Arc<AtomicBool>,
) -> Result<()> {
    let (au_tx, au_rx) = std::sync::mpsc::sync_channel::<AccessUnit>(AU_QUEUE_SIZE);

    let dest = cfg.dest;
    let rtp_cfg = cfg.rtp.clone();
    let dump_annexb = cfg.dump_annexb;
    let sender_handle =
        std::thread::spawn(move || sender::run_sender(dest, rtp_cfg, au_rx, dump_annexb));

    // Mirror h264_rtp::H264Payloader::payload_budget so the encoder's own
    // slice-length budget matches what the payloader will actually emit as a
    // single NAL unit packet.
    let payload_budget = cfg.rtp.mtu - 28 - 12;

    let mut encoder = make_encoder(
        cfg.encoder,
        cfg.bitrate_kbps,
        cfg.fps,
        cfg.idr_interval_frames,
        payload_budget,
        au_tx,
    )?;

    let mut first_timestamp: Option<chrono::DateTime<chrono::Local>> = None;
    for msg in rx {
        match msg {
            Msg::Write(frame, timestamp) => {
                let first_timestamp = *first_timestamp.get_or_insert(timestamp);
                let pts = (timestamp - first_timestamp).to_std().unwrap_or_default();

                let pending = pending_bitrate_bps.swap(0, Ordering::AcqRel);
                if pending != 0 {
                    encoder.set_bitrate(pending)?;
                }
                if keyframe_requested.swap(false, Ordering::AcqRel) {
                    encoder.request_keyframe()?;
                }
                encoder.submit(&frame.borrow(), pts)?;
            }
            Msg::Finish => break,
        }
    }

    encoder.finish()?;
    // `encoder` (and its clone of `au_tx`) was just dropped, so `au_rx`'s
    // for-loop in the sender thread ends and it returns on its own.
    match sender_handle.join() {
        Ok(result) => result?,
        Err(_) => tracing::warn!("RTP sender thread panicked"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use machine_vision_formats::PixFmt;
    use strand_dynamic_frame::DynamicFrameOwned;

    /// End-to-end smoke test of the whole crate (feeder thread, openh264
    /// encoder, sender thread, `h264-rtp` payloader, real UDP socket) with no
    /// external process: a handful of synthetic frames must arrive at a
    /// loopback socket as well-formed RTP packets starting at the configured
    /// initial sequence number. This does not prove the H.264 bitstream itself
    /// decodes (see `tests/loopback.rs` for that, via a real ffmpeg receiver).
    fn assert_stream_produces_rtp_packets(encoder: EncoderKind) {
        let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        socket
            .set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .unwrap();
        let dest = socket.local_addr().unwrap();

        let cfg = StreamConfig {
            dest,
            bitrate_kbps: 500,
            fps: 30.0,
            idr_interval_frames: 30,
            encoder,
            rtp: RtpSessionConfig {
                initial_sequence: 1000,
                ..Default::default()
            },
            queue_size: 8,
            dump_annexb: None,
        };
        let mut streamer = RtpH264Streamer::new(cfg).unwrap();

        let (width, height) = (64u32, 48u32);
        let mut ts = chrono::Local::now();
        for frame_idx in 0..5u8 {
            let mut buf = vec![0u8; (width * height * 3) as usize];
            for b in buf.iter_mut() {
                *b = frame_idx.wrapping_mul(17);
            }
            let frame = Arc::new(
                DynamicFrameOwned::from_buf(width, height, (width * 3) as usize, buf, PixFmt::RGB8)
                    .unwrap(),
            );
            streamer.send(frame, ts).unwrap();
            ts += chrono::Duration::milliseconds(33);
        }
        streamer.finish().unwrap();

        let mut buf = [0u8; 2048];
        let (n, _addr) = socket
            .recv_from(&mut buf)
            .expect("expected at least one RTP packet");
        assert!(n >= 12, "packet too short to be an RTP header");
        assert_eq!(buf[0] >> 6, 2, "RTP version must be 2");
        let seq = u16::from_be_bytes([buf[2], buf[3]]);
        assert_eq!(
            seq, 1000,
            "first packet must use the configured initial sequence number"
        );
    }

    #[test]
    fn openh264_stream_produces_rtp_packets() {
        assert_stream_produces_rtp_packets(EncoderKind::OpenH264(OpenH264EncoderConfig::default()));
    }

    #[test]
    fn ffmpeg_stream_produces_rtp_packets() {
        assert_stream_produces_rtp_packets(EncoderKind::Ffmpeg(FfmpegEncoderConfig::default()));
    }
}
