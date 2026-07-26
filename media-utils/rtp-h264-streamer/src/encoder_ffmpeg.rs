// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::{
    collections::VecDeque,
    io::{BufRead, BufReader},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{Arc, Mutex, mpsc::SyncSender},
    time::Duration,
};

use ffmpeg_writer::{FfmpegCodecArgs, ffmpeg_pixel_format, write_frame_rows};
use h264_reader::{annexb::AnnexBReader, push::NalFragmentHandler};
use h264_rtp::AccessUnit;
use machine_vision_formats::pixel_format::PixFmt;
use strand_dynamic_frame::DynamicFrame;

use crate::{Error, Result, encoder::H264StreamEncoder};

/// H.264 NAL unit type for an Access Unit Delimiter (RFC 6184 / ITU-T H.264
/// §7.4.1.2.3). Used only to detect AU boundaries in ffmpeg's output; never
/// forwarded onto the wire (see [`Handler::nal_fragment`]).
const NAL_TYPE_AUD: u8 = 9;
/// Supplemental Enhancement Information. x264 emits a ~642-byte
/// user-data-unregistered SEI before every IDR that carries nothing an
/// SDP-less receiver needs; dropped to save real bytes on a constrained link.
const NAL_TYPE_SEI: u8 = 6;
/// IDR slice: presence of one marks an access unit as a keyframe.
const NAL_TYPE_IDR: u8 = 5;

/// Configuration specific to the ffmpeg sidecar encoder.
///
/// Currently carries no fields of its own: bitrate, frame rate and the
/// intra-frame period all come from the shared [`crate::StreamConfig`] fields.
#[derive(Debug, Clone, Default)]
pub struct FfmpegEncoderConfig {}

/// Encoder backed by an `ffmpeg` child process piping raw video in and reading
/// an H.264 elementary stream back out.
///
/// [`Self::set_bitrate`] and [`Self::request_keyframe`] both respawn the
/// child (there is no cheaper way to change bitrate or force an IDR on an
/// already-running ffmpeg process): the RTP session lives in a different
/// thread entirely (see `sender.rs`), so a respawn loses a few frames but
/// leaves the session (and thus the receiver's decoder state) untouched — the
/// receiver just sees a fresh IDR once the new child's first frame arrives.
pub(crate) struct FfmpegStreamEncoder {
    bitrate_bps: u32,
    fps: f32,
    idr_interval_frames: u32,
    au_tx: SyncSender<AccessUnit>,
    /// Real per-frame presentation times, in submission order, popped by the
    /// stdout thread as each access unit completes. `-bf 0` (no B-frames)
    /// guarantees the encoder neither reorders nor drops frames, so this
    /// preserves the caller's real capture-time jitter instead of smoothing it
    /// into a fixed `frame_index / fps` clock.
    pts_queue: Arc<Mutex<VecDeque<Duration>>>,
    running: Option<Running>,
}

struct Running {
    child: Child,
    stdin: ChildStdin,
    pixfmt: PixFmt,
    width: u32,
    height: u32,
    stdout_thread: std::thread::JoinHandle<()>,
    stderr_thread: std::thread::JoinHandle<()>,
}

impl FfmpegStreamEncoder {
    pub(crate) fn new(
        _cfg: FfmpegEncoderConfig,
        bitrate_bps: u32,
        fps: f32,
        idr_interval_frames: u32,
        au_tx: SyncSender<AccessUnit>,
    ) -> Result<Self> {
        Ok(Self {
            bitrate_bps,
            fps,
            idr_interval_frames,
            au_tx,
            pts_queue: Arc::new(Mutex::new(VecDeque::new())),
            running: None,
        })
    }

    /// Spawn ffmpeg configured to read raw video of this frame's format and
    /// emit a zero-latency, SDP-less-receiver-friendly H.264 elementary stream.
    fn start(&mut self, frame: &DynamicFrame) -> Result<()> {
        let pixfmt = frame.pixel_format();
        let ff_pixfmt = ffmpeg_pixel_format(pixfmt)?;
        let width = frame.width();
        let height = frame.height();

        // bufsize = 2 seconds' worth of frames at the target bitrate, a
        // conventional VBV buffer size for low-latency live encoding.
        let bufsize_bits = (self.bitrate_bps as f64 / self.fps as f64 * 2.0).round() as u64;
        let codec_args = FfmpegCodecArgs {
            codec: Some("libx264".to_string()),
            max_bframes: Some(0),
            post_codec_args: Some(vec![
                ("-preset".into(), "ultrafast".into()),
                ("-tune".into(), "zerolatency".into()),
                ("-g".into(), self.idr_interval_frames.to_string()),
                ("-b:v".into(), format!("{}", self.bitrate_bps)),
                ("-maxrate".into(), format!("{}", self.bitrate_bps)),
                ("-bufsize".into(), bufsize_bits.to_string()),
                // repeat-headers is the Annex-B default (not disabled here),
                // so SPS/PPS precede every IDR with no extra flag; aud=1 gives
                // the stdout thread explicit AU boundaries to delimit on.
                ("-x264-params".into(), "slice-max-size=1200:aud=1".into()),
                ("-flush_packets".into(), "1".into()),
                ("-f".into(), "h264".into()),
            ]),
            ..Default::default()
        };

        let input_args = vec![
            "-f".to_string(),
            "rawvideo".to_string(),
            "-pixel_format".to_string(),
            ff_pixfmt.to_string(),
            "-video_size".to_string(),
            format!("{width}x{height}"),
            "-framerate".to_string(),
            format!("{}/1", self.fps.round().max(1.0) as u32),
        ];

        let mut args = codec_args.to_args(&input_args);
        args.push("pipe:1".to_string());

        let mut child = Command::new("ffmpeg")
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let stdin = child.stdin.take().expect("stdin was piped");
        let stdout = child.stdout.take().expect("stdout was piped");
        let stderr = child.stderr.take().expect("stderr was piped");

        let stdout_thread = {
            let au_tx = self.au_tx.clone();
            let pts_queue = self.pts_queue.clone();
            std::thread::spawn(move || run_stdout_reader(stdout, au_tx, pts_queue))
        };
        // Mandatory: with stdout piped, an undrained stderr deadlocks the
        // child once its pipe buffer fills.
        let stderr_thread = std::thread::spawn(move || {
            for line in BufReader::new(stderr)
                .lines()
                .map_while(std::result::Result::ok)
            {
                tracing::debug!("ffmpeg: {line}");
            }
        });

        self.running = Some(Running {
            child,
            stdin,
            pixfmt,
            width,
            height,
            stdout_thread,
            stderr_thread,
        });
        Ok(())
    }

    /// Kill the running child (if any), drain its threads, and clear any
    /// presentation times left over from frames it never got to encode
    /// before dying, so they cannot be misattributed to the next child's
    /// access units.
    fn stop(&mut self) -> Result<()> {
        if let Some(running) = self.running.take() {
            let Running {
                mut child,
                stdin,
                stdout_thread,
                stderr_thread,
                ..
            } = running;
            drop(stdin); // signal EOF on ffmpeg's stdin
            let _ = child.wait();
            let _ = stdout_thread.join();
            let _ = stderr_thread.join();
        }
        self.pts_queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        Ok(())
    }
}

impl H264StreamEncoder for FfmpegStreamEncoder {
    fn submit(&mut self, frame: &DynamicFrame, pts: Duration) -> Result<()> {
        if self.running.is_none() {
            self.start(frame)?;
        }
        let running = self
            .running
            .as_ref()
            .expect("just started if it wasn't running");
        if frame.pixel_format() != running.pixfmt
            || frame.width() != running.width
            || frame.height() != running.height
        {
            return Err(Error::FfmpegWriter(
                ffmpeg_writer::Error::FormatOrSizeChanged,
            ));
        }

        self.pts_queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_back(pts);
        let running = self.running.as_mut().expect("checked above");
        write_frame_rows(frame, &mut running.stdin)?;
        Ok(())
    }

    fn set_bitrate(&mut self, bps: u32) -> Result<()> {
        self.bitrate_bps = bps;
        self.stop()
    }

    fn request_keyframe(&mut self) -> Result<()> {
        // No cheap in-process equivalent to openh264's force_intra_frame for
        // an already-spawned ffmpeg child; respawning also forces a fresh IDR.
        self.stop()
    }

    fn finish(mut self: Box<Self>) -> Result<()> {
        self.stop()
    }
}

/// Groups the parsed NAL stream into [`AccessUnit`]s and pushes them to
/// `au_tx`, using AUD (type 9) NALs purely as boundary markers: an
/// [`AccessUnit`] is only known to be complete once the *next* one starts, so
/// this never emits early. SEI (type 6) is dropped; every other NAL type is
/// forwarded verbatim.
struct Handler {
    au_tx: SyncSender<AccessUnit>,
    pts_queue: Arc<Mutex<VecDeque<Duration>>>,
    /// Accumulates fragments of the NAL currently being parsed, across
    /// however many `nal_fragment` calls (and thus however many stdout reads)
    /// it takes to complete.
    current_nal: Vec<u8>,
    /// Completed NALs of the access unit not yet flushed.
    pending_nals: Vec<Vec<u8>>,
    pending_is_keyframe: bool,
}

impl Handler {
    fn flush(&mut self) {
        if self.pending_nals.is_empty() {
            return;
        }
        let nals = std::mem::take(&mut self.pending_nals);
        let is_keyframe = std::mem::take(&mut self.pending_is_keyframe);
        let pts = self
            .pts_queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pop_front()
            .unwrap_or_default();
        let au = AccessUnit {
            nals,
            is_keyframe,
            pts,
        };
        // If the sender side is gone, the whole encoder is being torn down;
        // there is nothing more useful to do than stop forwarding.
        let _ = self.au_tx.send(au);
    }
}

impl NalFragmentHandler for Handler {
    fn nal_fragment(&mut self, bufs: &[&[u8]], is_end: bool) {
        for buf in bufs {
            self.current_nal.extend_from_slice(buf);
        }
        if !is_end {
            return;
        }
        let nal = std::mem::take(&mut self.current_nal);
        let Some(&header) = nal.first() else {
            return;
        };
        match header & 0x1F {
            NAL_TYPE_AUD => self.flush(),
            NAL_TYPE_SEI => {}
            nal_type => {
                if nal_type == NAL_TYPE_IDR {
                    self.pending_is_keyframe = true;
                }
                self.pending_nals.push(nal);
            }
        }
    }
}

fn run_stdout_reader(
    mut stdout: impl std::io::Read,
    au_tx: SyncSender<AccessUnit>,
    pts_queue: Arc<Mutex<VecDeque<Duration>>>,
) {
    let mut reader = AnnexBReader::for_fragment_handler(Handler {
        au_tx,
        pts_queue,
        current_nal: Vec::new(),
        pending_nals: Vec::new(),
        pending_is_keyframe: false,
    });
    let mut buf = [0u8; 64 * 1024];
    loop {
        match std::io::Read::read(&mut stdout, &mut buf) {
            Ok(0) => break,
            Ok(n) => reader.push(&buf[..n]),
            Err(_) => break,
        }
    }
    // Flush whatever access unit was still pending when the child exited.
    reader.into_fragment_handler().flush();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::sync_channel;

    fn new_handler(
        au_tx: SyncSender<AccessUnit>,
        pts: impl IntoIterator<Item = Duration>,
    ) -> Handler {
        Handler {
            au_tx,
            pts_queue: Arc::new(Mutex::new(pts.into_iter().collect())),
            current_nal: Vec::new(),
            pending_nals: Vec::new(),
            pending_is_keyframe: false,
        }
    }

    fn feed(handler: &mut Handler, nal: &[u8]) {
        handler.nal_fragment(&[nal], true);
    }

    /// AUD (type 9) NALs delimit access units but are never themselves
    /// forwarded; SEI (type 6) is dropped; an IDR slice (type 5) anywhere in
    /// the AU marks it as a keyframe; every other NAL type is forwarded
    /// verbatim in arrival order.
    #[test]
    fn aud_boundaries_group_nals_and_drop_sei_and_aud() {
        let (au_tx, au_rx) = sync_channel(8);
        let mut handler = new_handler(au_tx, [Duration::from_millis(0), Duration::from_millis(33)]);

        // AU 1: (no preceding AUD -- this is the very first access unit) SPS,
        // PPS, a dropped SEI, then an IDR slice.
        feed(&mut handler, &[7, 1, 2, 3]); // SPS
        feed(&mut handler, &[8, 4, 5]); // PPS
        feed(&mut handler, &[6, 9, 9, 9]); // SEI, dropped
        feed(&mut handler, &[5, 0xAA, 0xBB]); // IDR slice

        // The AUD starting AU 2 flushes AU 1.
        feed(&mut handler, &[9, 0xF0]);
        feed(&mut handler, &[1, 0xCC]); // P slice

        // "Stream end": flush whatever was still pending.
        handler.flush();

        let au1 = au_rx.try_recv().unwrap();
        assert_eq!(
            au1.nals,
            vec![vec![7, 1, 2, 3], vec![8, 4, 5], vec![5, 0xAA, 0xBB]]
        );
        assert!(au1.is_keyframe);
        assert_eq!(au1.pts, Duration::from_millis(0));

        let au2 = au_rx.try_recv().unwrap();
        assert_eq!(au2.nals, vec![vec![1, 0xCC]]);
        assert!(!au2.is_keyframe);
        assert_eq!(au2.pts, Duration::from_millis(33));

        assert!(
            au_rx.try_recv().is_err(),
            "no third access unit was ever pushed"
        );
    }

    /// A NAL delivered across several `nal_fragment` calls (as happens when it
    /// straddles two stdout reads) must be reassembled whole before its type
    /// is inspected, not treated as multiple separate NALs.
    #[test]
    fn nal_fragments_spanning_multiple_calls_are_reassembled() {
        let (au_tx, au_rx) = sync_channel(8);
        let mut handler = new_handler(au_tx, [Duration::ZERO]);

        handler.nal_fragment(&[&[1]], false);
        handler.nal_fragment(&[&[0xAA, 0xBB]], false);
        handler.nal_fragment(&[&[0xCC]], true);
        handler.flush();

        let au = au_rx.try_recv().unwrap();
        assert_eq!(au.nals, vec![vec![1, 0xAA, 0xBB, 0xCC]]);
    }

    /// Flushing an access unit with no NALs accumulated (e.g. two AUDs in a
    /// row) must be a no-op: in particular it must not consume a PTS that a
    /// later, real access unit needs.
    #[test]
    fn flush_with_no_pending_nals_is_a_no_op() {
        let (au_tx, au_rx) = sync_channel(8);
        let mut handler = new_handler(au_tx, [Duration::from_millis(5)]);

        feed(&mut handler, &[9, 0xF0]); // AUD with nothing pending yet
        feed(&mut handler, &[1, 0x11]);
        handler.flush();

        let au = au_rx.try_recv().unwrap();
        assert_eq!(
            au.pts,
            Duration::from_millis(5),
            "the sole PTS must still be consumed by the real AU"
        );
        assert!(au_rx.try_recv().is_err());
    }
}
