// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::{
    collections::VecDeque,
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{Arc, Mutex},
};

use machine_vision_formats::pixel_format::PixFmt;

const FFMPEG: &str = "ffmpeg";

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("ffmpeg error ({})", output.status)]
    FfmpegError { output: std::process::Output },
    /// A streaming ffmpeg child died. Unlike [`Error::FfmpegError`], whose
    /// one-shot command was waited on for its complete output, this child's
    /// stderr was being drained line by line as it ran (see [`StderrTail`]), so
    /// what we can report is the tail of it.
    #[error("ffmpeg exited ({status}); last stderr:\n{stderr}")]
    FfmpegExited {
        status: std::process::ExitStatus,
        stderr: String,
    },
    #[error("string not valid UTF8")]
    FromUtf8Error(#[from] std::string::FromUtf8Error),
    #[error("unexpected ffmpeg output: {0}")]
    UnexpectedFfmpegOutput(String),
    #[error("the frame format or size changed mid-stream")]
    FormatOrSizeChanged,
    // We deliberately do not (yet) convert unsupported pixel formats to a format
    // ffmpeg accepts; such conversions belong in the `convert-image` crate. For
    // now, formats without a direct raw-video equivalent are unimplemented.
    #[error("no direct ffmpeg raw-video pixel format for {0}; conversion unimplemented")]
    UnimplementedPixelFormat(PixFmt),
}

type Result<T> = std::result::Result<T, Error>;

/// The ffmpeg raw-video (`-f rawvideo`) pixel-format name corresponding to a
/// [`PixFmt`], if the bytes can be piped to ffmpeg without any conversion on
/// our side (ffmpeg itself does any conversion the encoder needs).
///
/// Returns `Err(Error::UnimplementedPixelFormat)` for formats that would
/// require us to convert first (e.g. 32-bit float or planar formats).
pub fn ffmpeg_pixel_format(pixfmt: PixFmt) -> Result<&'static str> {
    use PixFmt::*;
    // The Bayer names differ between the machine-vision-formats convention
    // (named by the first two pixels of the first row) and ffmpeg's (named by
    // the top-left 2x2 block): e.g. `BayerRG8` (row0 = R,G; row1 = G,B) is
    // ffmpeg's `bayer_rggb8`.
    Ok(match pixfmt {
        Mono8 => "gray",
        RGB8 => "rgb24",
        // machine-vision-formats YUV422 is UYVY-packed ([U, Y0, V, Y1]).
        YUV422 => "uyvy422",
        BayerRG8 => "bayer_rggb8",
        BayerGR8 => "bayer_grbg8",
        BayerGB8 => "bayer_gbrg8",
        BayerBG8 => "bayer_bggr8",
        other => return Err(Error::UnimplementedPixelFormat(other)),
    })
}

/// How a mono frame is handed to ffmpeg.
///
/// ffmpeg is happiest being given `gray`: one byte per pixel, and it
/// synthesizes the neutral chroma the encoder needs. Some releases get that
/// wrong (see [`probe_mono_framing`]), and on those the frame is handed over as
/// NV12 with a neutral chroma plane supplied here instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonoFraming {
    /// `-pixel_format gray`, luma only.
    Gray,
    /// `-pixel_format nv12`, luma followed by a constant neutral chroma plane.
    Nv12,
}

impl MonoFraming {
    /// The ffmpeg raw-video pixel format this framing pipes.
    fn ffmpeg_pixel_format(self) -> &'static str {
        match self {
            Self::Gray => "gray",
            Self::Nv12 => "nv12",
        }
    }

    /// Bytes of chroma this framing appends after the luma rows.
    ///
    /// NV12 interleaves U and V at half resolution in both directions, rounding
    /// up, so an odd width or height still gets a whole chroma sample.
    fn chroma_len(self, width: u32, height: u32) -> usize {
        match self {
            Self::Gray => 0,
            Self::Nv12 => {
                let (w, h) = (width as usize, height as usize);
                w.div_ceil(2) * h.div_ceil(2) * 2
            }
        }
    }
}

/// What probing this machine's ffmpeg found out about a codec configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonoFramingProbe {
    /// `gray` records neutral chroma here, so frames are piped at one byte per
    /// pixel.
    GrayIsSafe,
    /// `gray` records a green cast here. Frames are piped as NV12 instead,
    /// which costs a neutral chroma plane per frame.
    Nv12Required,
    /// Neither framing came back neutral, or the probe could not be run at all.
    /// NV12 is used, being the one that does not depend on ffmpeg synthesizing
    /// anything, but something about this configuration needs looking at.
    Inconclusive,
}

impl MonoFramingProbe {
    /// The framing to record with.
    pub fn framing(self) -> MonoFraming {
        match self {
            Self::GrayIsSafe => MonoFraming::Gray,
            Self::Nv12Required | Self::Inconclusive => MonoFraming::Nv12,
        }
    }
}

/// How far a chroma sample may sit from neutral and still count as neutral.
///
/// Encoding is lossy, so exact 128 is not something to insist on. The failure
/// this distinguishes is not subtle: the affected releases write 0, not 127.
const NEUTRAL_CHROMA_TOLERANCE: u8 = 4;

/// Frames the probe records. Two, so the encoder emits an inter frame as well
/// as its opening key frame.
const PROBE_FRAMES: usize = 2;

/// Ask this machine's ffmpeg what it actually does with a mono frame.
///
/// Some ffmpeg releases write 0 rather than 128 into the chroma planes when
/// converting a mono (`gray`) source to a *full-range* *semi-planar*
/// destination such as NV12 -- which is what [`FfmpegCodecArgs::to_args`] asks
/// for, since it always requests full-range output, and what the `vaapi` preset
/// converts to. Chroma 0 is not neutral, it is green, so the luma comes out
/// perfect and the whole recording has a solid green cast. Known bad: 7.1.5
/// (Debian trixie). Known good: 8.1.2, and git builds from 2026-07-31 onward.
///
/// Rather than trust a version number, this records `PROBE_FRAMES` frames with
/// the caller's real codec arguments, decodes one back, and looks at the
/// chroma. That covers the whole pipeline -- filter chain, hardware upload,
/// encoder, driver -- and so also covers distro backports, private builds and
/// hardware differences that no version comparison could see.
///
/// `gray` is tried first because it is the cheaper framing, and NV12 only if
/// `gray` proves broken; NV12 is verified rather than assumed. The probe is
/// deliberately run at the caller's real frame size: encoders have their own
/// size limits (VAAPI on one tested GPU accepts nothing below 128x128), so a
/// convenient small size can fail to open an encoder that works fine in
/// production.
///
/// Costs roughly 0.1 s. [`FfmpegWriter`] never runs it, so recording is never
/// delayed by it; call this when a codec is chosen, and the answer is cached
/// for the process. An unprobed configuration records as NV12.
pub fn probe_mono_framing(
    codec_args: &FfmpegCodecArgs,
    width: u32,
    height: u32,
) -> MonoFramingProbe {
    if let Some(cached) = cached_probe(codec_args, width, height) {
        return cached;
    }

    // Serialize probing, so two cameras sharing a configuration establish this
    // once instead of running competing encoders against the same device. Held
    // across the whole probe, and deliberately not the cache's own lock:
    // reading a cached answer is what starting a recording does, and that must
    // never wait behind a probe.
    let _serialized = PROBE_LOCK.lock().expect("the probe lock is never poisoned");
    if let Some(cached) = cached_probe(codec_args, width, height) {
        // Answered while this call was waiting its turn.
        return cached;
    }

    let mut outcome = MonoFramingProbe::Inconclusive;
    for (framing, verdict) in [
        (MonoFraming::Gray, MonoFramingProbe::GrayIsSafe),
        (MonoFraming::Nv12, MonoFramingProbe::Nv12Required),
    ] {
        match records_neutral_chroma(codec_args, framing, width, height) {
            Ok(true) => {
                outcome = verdict;
                break;
            }
            Ok(false) => {
                tracing::debug!(
                    "this ffmpeg records {width}x{height} mono frames piped as {} with a green \
                     cast",
                    framing.ffmpeg_pixel_format()
                );
            }
            Err(e) => {
                tracing::warn!(
                    "could not probe {} framing for {width}x{height}: {e}",
                    framing.ffmpeg_pixel_format()
                );
            }
        }
    }

    match outcome {
        MonoFramingProbe::GrayIsSafe => {
            tracing::debug!("this ffmpeg handles mono frames correctly; piping them as gray");
        }
        MonoFramingProbe::Nv12Required => {
            tracing::warn!(
                "this ffmpeg would record mono video with a green cast, so mono frames will be \
                 piped as NV12 with a neutral chroma plane. Upgrading ffmpeg avoids the extra \
                 work."
            );
        }
        MonoFramingProbe::Inconclusive => {
            tracing::error!(
                "could not establish how this ffmpeg records {width}x{height} mono video; \
                 recording as NV12, which does not rely on ffmpeg supplying neutral chroma. \
                 Check that these codec arguments work at all."
            );
        }
    }
    remember_probe(codec_args, width, height, outcome);
    outcome
}

/// Probe results for this process, keyed by what was probed.
///
/// A `Vec` rather than a map: a program records from a handful of
/// configurations at most, and this way [`FfmpegCodecArgs`] needs no `Hash`.
static PROBE_CACHE: std::sync::Mutex<Vec<(FfmpegCodecArgs, u32, u32, MonoFramingProbe)>> =
    std::sync::Mutex::new(Vec::new());

/// Held for the duration of a probe, so concurrent callers asking the same
/// question wait for the first answer rather than duplicating it.
static PROBE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn cached_probe(codec_args: &FfmpegCodecArgs, width: u32, height: u32) -> Option<MonoFramingProbe> {
    let cache = PROBE_CACHE
        .lock()
        .expect("the probe cache is never poisoned");
    cache
        .iter()
        .find(|(args, w, h, _)| args == codec_args && *w == width && *h == height)
        .map(|(_, _, _, outcome)| *outcome)
}

fn remember_probe(
    codec_args: &FfmpegCodecArgs,
    width: u32,
    height: u32,
    outcome: MonoFramingProbe,
) {
    let mut cache = PROBE_CACHE
        .lock()
        .expect("the probe cache is never poisoned");
    cache.push((codec_args.clone(), width, height, outcome));
}

/// The framing to record mono frames of this size with, if it has been probed.
///
/// Unprobed configurations get NV12: it does not depend on ffmpeg synthesizing
/// neutral chroma, so it is right everywhere, and paying for a chroma plane
/// beats delaying a recording to find out whether it was necessary.
fn probed_mono_framing(codec_args: &FfmpegCodecArgs, width: u32, height: u32) -> MonoFraming {
    match cached_probe(codec_args, width, height) {
        Some(outcome) => outcome.framing(),
        None => {
            tracing::debug!(
                "no mono framing probe for this configuration at {width}x{height}; piping NV12"
            );
            MonoFraming::Nv12
        }
    }
}

/// Record a couple of synthetic mono frames and report whether the chroma
/// survives as neutral.
fn records_neutral_chroma(
    codec_args: &FfmpegCodecArgs,
    framing: MonoFraming,
    width: u32,
    height: u32,
) -> Result<bool> {
    let dir = tempfile::tempdir()?;
    // An extension ffmpeg can infer a container from; the codec arguments say
    // nothing about muxing.
    let probe_path = dir.path().join("probe.mp4");
    let probe_name = probe_path.to_string_lossy().into_owned();

    let mut args = codec_args.to_args(&raw_video_input_args(
        framing.ffmpeg_pixel_format(),
        width,
        height,
        25,
        1,
    ));
    args.push(probe_name);

    let mut child = Command::new(FFMPEG)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    {
        let mut stdin = child.stdin.take().expect("stdin was piped");
        // A horizontal ramp, so the frames are not uniform and the encoder has
        // something to do. Chroma is neutral, whatever the luma.
        let luma: Vec<u8> = (0..width)
            .map(|x| (x * 255 / width.max(2).saturating_sub(1)) as u8)
            .collect();
        let chroma = vec![128u8; framing.chroma_len(width, height)];
        for _ in 0..PROBE_FRAMES {
            for _ in 0..height {
                // A dead encoder is reported from its exit status below, which
                // says far more than the broken pipe would.
                if stdin.write_all(&luma).is_err() {
                    break;
                }
            }
            if stdin.write_all(&chroma).is_err() {
                break;
            }
        }
    }
    let encoded = child.wait_with_output()?;
    if !encoded.status.success() {
        return Err(Error::FfmpegError { output: encoded });
    }

    decoded_chroma_is_neutral(&probe_path, width, height)
}

/// Decode the first frame of `path` and report whether its chroma is neutral.
///
/// Decoding to `yuv420p` asks for H.264's own planar 4:2:0 layout, so the
/// decode introduces no conversion of its own that could hide -- or invent --
/// a chroma error.
fn decoded_chroma_is_neutral(path: &std::path::Path, width: u32, height: u32) -> Result<bool> {
    let decoded = Command::new(FFMPEG)
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(path)
        .args([
            "-frames:v",
            "1",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "yuv420p",
            "-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;
    if !decoded.status.success() {
        return Err(Error::FfmpegError { output: decoded });
    }

    let luma_len = width as usize * height as usize;
    let chroma = decoded.stdout.get(luma_len..).ok_or_else(|| {
        Error::UnexpectedFfmpegOutput(format!(
            "decoded {} bytes, too few for one {width}x{height} frame",
            decoded.stdout.len()
        ))
    })?;
    if chroma.is_empty() {
        return Err(Error::UnexpectedFfmpegOutput(
            "decoded frame carried no chroma".to_string(),
        ));
    }
    Ok(chroma
        .iter()
        .all(|c| c.abs_diff(128) <= NEUTRAL_CHROMA_TOLERANCE))
}

/// The raw-video input options, placed just before `-i -`, describing the frames
/// arriving on stdin.
///
/// The input is unconditionally tagged full range (`pc`) so ffmpeg preserves the
/// whole 0-255 intensity range; paired with the unconditional `-color_range pc`
/// output tag in [`FfmpegCodecArgs::to_args`], this is not meant to be
/// overridable.
fn raw_video_input_args(
    ff_pixfmt: &str,
    width: u32,
    height: u32,
    raten: usize,
    rated: usize,
) -> Vec<String> {
    vec![
        "-f".to_string(),
        "rawvideo".to_string(),
        "-pixel_format".to_string(),
        ff_pixfmt.to_string(),
        "-video_size".to_string(),
        format!("{width}x{height}"),
        "-framerate".to_string(),
        format!("{raten}/{rated}"),
        "-color_range".to_string(),
        "pc".to_string(),
    ]
}

/// Saves video frames to a video file using ffmpeg.
///
/// A thin layer over [`FfmpegFrameSink`] (which does the spawning and piping)
/// adding the presentation timestamps a file recording needs. The ffmpeg
/// process is spawned lazily on the first frame, once the frame width, height
/// and pixel format are known.
pub struct FfmpegWriter {
    fname: String,
    ffmpeg_codec_args: FfmpegCodecArgs,
    raten: usize,
    rated: usize,
    count: usize,
    sink: Option<FfmpegFrameSink>,
}

/// Where a spawned ffmpeg writes its encoded output.
pub enum FfmpegOutput {
    /// A file, named by the path handed to ffmpeg as its output argument.
    File(String),
    /// ffmpeg's own stdout (`pipe:1`), for a caller that wants the encoded
    /// bytes back rather than a file. Take the pipe with
    /// [`FfmpegFrameSink::take_stdout`] and keep reading it: an undrained
    /// stdout deadlocks the child exactly as an undrained stderr does.
    Stdout,
}

/// How many lines of a child's stderr to keep for its epitaph.
///
/// ffmpeg repeats itself when it is unhappy, so the last few lines are
/// generally the whole story; keeping all of them would let a chatty child grow
/// this without bound over a long recording.
const STDERR_TAIL_LINES: usize = 20;

/// A spawned ffmpeg process being fed raw video frames on its stdin.
///
/// This owns everything that is the same whether the encoded result lands in a
/// file or comes back to us on a pipe:
///
/// - choosing the raw-video framing for the frame's pixel format, including the
///   mono `gray`-versus-NV12 decision (see [`probe_mono_framing`]) and the
///   neutral chroma plane NV12 framing has to supply;
/// - pinning the geometry the child was spawned for, so a mid-stream change is
///   reported rather than silently producing garbled video;
/// - draining stderr, which is not optional (see [`StderrTail`]);
/// - turning a dead child into an error carrying ffmpeg's own complaint instead
///   of a bare `BrokenPipe`.
///
/// Every ffmpeg process in this workspace that is fed frames on stdin should be
/// one of these. Rolling the spawn by hand is how a caller ends up silently
/// missing the mono framing decision and recording green video.
pub struct FfmpegFrameSink {
    child: Child,
    stdin: ChildStdin,
    /// Present only for [`FfmpegOutput::Stdout`], until the caller takes it.
    stdout: Option<ChildStdout>,
    pixfmt: PixFmt,
    width: u32,
    height: u32,
    /// Neutral chroma to append after each frame's luma rows. Empty unless a
    /// mono camera is being piped as NV12; allocated once for the recording,
    /// never rewritten.
    chroma: Vec<u8>,
    /// Absent when ffmpeg's output was left attached to our own terminal (see
    /// `FFMPEG_WRITER_SHOW`), in which case there is nothing to drain.
    stderr: Option<StderrTail>,
}

/// A thread draining a child's stderr, keeping the tail of it.
///
/// Draining is mandatory rather than merely tidy: ffmpeg blocks once a piped
/// stderr's buffer fills, and a blocked ffmpeg stops reading our frames, so the
/// whole pipeline wedges. `-nostats` keeps ffmpeg quiet enough that this is
/// unlikely, but "unlikely" over a multi-hour flight is not a guarantee worth
/// relying on. Lines go to `tracing` at `debug`, and the last
/// [`STDERR_TAIL_LINES`] are kept so that a child which later dies can say why.
struct StderrTail {
    thread: std::thread::JoinHandle<()>,
    lines: Arc<Mutex<VecDeque<String>>>,
}

impl StderrTail {
    fn spawn(stderr: std::process::ChildStderr) -> Self {
        let lines = Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_TAIL_LINES)));
        let thread = {
            let lines = lines.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stderr)
                    .lines()
                    .map_while(std::result::Result::ok)
                {
                    tracing::debug!("ffmpeg: {line}");
                    let mut lines = lines.lock().unwrap_or_else(|e| e.into_inner());
                    if lines.len() == STDERR_TAIL_LINES {
                        lines.pop_front();
                    }
                    lines.push_back(line);
                }
            })
        };
        Self { thread, lines }
    }

    /// Wait for the draining thread to finish, then return what it collected.
    ///
    /// Joining first matters: the caller reaches here just after the child
    /// died, so its stderr is at EOF and the thread is about to exit with the
    /// last few lines -- exactly the interesting ones -- possibly still in
    /// flight.
    fn join_and_tail(self) -> String {
        let Self { thread, lines } = self;
        let _ = thread.join();
        let lines = lines.lock().unwrap_or_else(|e| e.into_inner());
        lines.iter().cloned().collect::<Vec<_>>().join("\n")
    }
}

impl FfmpegFrameSink {
    /// Spawn ffmpeg ready to receive frames of `frame`'s format and size.
    ///
    /// `frame` is inspected but not written; the first frame is normally passed
    /// here and then to [`Self::send`].
    pub fn new(
        frame: &strand_dynamic_frame::DynamicFrame,
        ffmpeg_codec_args: &FfmpegCodecArgs,
        rate: (usize, usize),
        output: FfmpegOutput,
    ) -> Result<Self> {
        let pixfmt = frame.pixel_format();
        let width = frame.width();
        let height = frame.height();
        let (raten, rated) = rate;

        // A mono camera is piped either as `gray` or, where this ffmpeg would
        // turn that green, as NV12 with chroma supplied here. Everything else
        // has real chroma of its own and goes as-is.
        let framing = (ffmpeg_pixel_format(pixfmt)? == MonoFraming::Gray.ffmpeg_pixel_format())
            .then(|| probed_mono_framing(ffmpeg_codec_args, width, height));
        let ff_pixfmt = match framing {
            Some(framing) => framing.ffmpeg_pixel_format(),
            None => ffmpeg_pixel_format(pixfmt)?,
        };
        let chroma = vec![128u8; framing.map_or(0, |framing| framing.chroma_len(width, height))];

        let input_args = raw_video_input_args(ff_pixfmt, width, height, raten, rated);
        let mut args = ffmpeg_codec_args.to_args(&input_args);
        args.push(match &output {
            FfmpegOutput::File(fname) => fname.clone(),
            FfmpegOutput::Stdout => "pipe:1".to_string(),
        });

        let show_ffmpeg = match std::env::var_os("FFMPEG_WRITER_SHOW") {
            Some(v) => &v != "0",
            None => false,
        };
        if show_ffmpeg {
            println!("ffmpeg {}", args.join(" "));
        }

        // Piping stdout is not a choice when it carries the encoded stream;
        // otherwise it is piped merely to keep ffmpeg's chatter off our
        // terminal, which `FFMPEG_WRITER_SHOW` turns off.
        let pipe_stdout = matches!(output, FfmpegOutput::Stdout) || !show_ffmpeg;
        let mut cmd0 = Command::new(FFMPEG);
        let cmd = cmd0.args(args).stdin(Stdio::piped());
        let cmd = if pipe_stdout {
            cmd.stdout(Stdio::piped())
        } else {
            cmd
        };
        let cmd = if show_ffmpeg {
            cmd
        } else {
            cmd.stderr(Stdio::piped())
        };
        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take().expect("stdin was piped");
        let stdout = match output {
            FfmpegOutput::Stdout => Some(child.stdout.take().expect("stdout was piped")),
            // Left on the child, unread: ffmpeg writing to a file says nothing
            // on stdout, and `collect_error` wants whatever is there.
            FfmpegOutput::File(_) => None,
        };
        let stderr = child.stderr.take().map(StderrTail::spawn);

        Ok(Self {
            child,
            stdin,
            stdout,
            pixfmt,
            width,
            height,
            chroma,
            stderr,
        })
    }

    /// Take ffmpeg's stdout pipe, for [`FfmpegOutput::Stdout`]. Returns `None`
    /// on a file sink, or if it has already been taken.
    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.stdout.take()
    }

    /// The pixel format, width and height this child was spawned for. Frames
    /// [`Self::send`] accepts must match.
    pub fn geometry(&self) -> (PixFmt, u32, u32) {
        (self.pixfmt, self.width, self.height)
    }

    /// Pipe one frame's rows (plus the neutral chroma plane, if this is a mono
    /// camera framed as NV12) to ffmpeg's stdin.
    pub fn send(&mut self, frame: &strand_dynamic_frame::DynamicFrame) -> Result<()> {
        if frame.pixel_format() != self.pixfmt
            || frame.width() != self.width
            || frame.height() != self.height
        {
            return Err(Error::FormatOrSizeChanged);
        }

        let written = write_frame_rows(frame, &mut self.stdin).and_then(|()| {
            // Empty unless this is a mono camera being piped as NV12.
            self.stdin.write_all(&self.chroma)?;
            Ok(())
        });
        match written {
            Ok(()) => Ok(()),
            Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::BrokenPipe => {
                // ffmpeg apparently died; surface its own complaint instead.
                Err(self.collect_error())
            }
            Err(e) => Err(e),
        }
    }

    /// Wait for the (apparently dead) ffmpeg process and describe why it died.
    fn collect_error(&mut self) -> Error {
        let status = match self.child.wait() {
            Ok(status) => status,
            Err(e) => return Error::Io(e),
        };
        Error::FfmpegExited {
            status,
            // `None` when output went to our terminal, in which case the user
            // has already seen whatever ffmpeg had to say.
            stderr: self
                .stderr
                .take()
                .map_or_else(String::new, StderrTail::join_and_tail),
        }
    }

    /// Tell ffmpeg to finish (by closing its stdin) and wait for it.
    ///
    /// A caller reading [`Self::take_stdout`] should keep reading until EOF and
    /// join its reader thread after this returns, so no encoded output is lost.
    pub fn close(mut self) -> Result<()> {
        // Closing stdin is what tells ffmpeg to flush and exit.
        drop(self.stdin);
        let status = self.child.wait()?;
        let stderr = self
            .stderr
            .take()
            .map_or_else(String::new, StderrTail::join_and_tail);
        if status.success() {
            Ok(())
        } else {
            Err(Error::FfmpegExited { status, stderr })
        }
    }
}

type FfmpegCodecArgList = Option<Vec<(String, String)>>;

/// The default output pixel format. 4:2:0 chroma subsampling is the most widely
/// decodable choice; in particular the built-in OpenH264 decoder only handles
/// 4:2:0, so anything we might want to decode later must be encoded this way.
/// Without forcing this, encoders like libx264 pick a format matching the input
/// (e.g. `yuv444p` for RGB input), which OpenH264 cannot decode.
const DEFAULT_OUTPUT_PIXFMT: &str = "yuv420p";

#[derive(Debug, PartialEq, Clone)]
pub struct FfmpegCodecArgs {
    pub device_args: FfmpegCodecArgList,
    pub pre_codec_args: FfmpegCodecArgList,
    pub codec: Option<String>,
    pub post_codec_args: FfmpegCodecArgList,
    /// Output pixel format passed to ffmpeg as `-pix_fmt`. Defaults to
    /// [`DEFAULT_OUTPUT_PIXFMT`] (`yuv420p`). Set to `None` to let ffmpeg (or a
    /// `-vf`/`-pix_fmt` in the other arg lists) decide, e.g. for hardware
    /// encoders whose filter chain already fixes the format.
    pub pixfmt: Option<String>,
    /// Maximum number of B-frames passed to ffmpeg as `-bf`. `None`, the
    /// default, lets the encoder (or a `-bf` in the other arg lists) decide.
    pub max_bframes: Option<u32>,
}

impl Default for FfmpegCodecArgs {
    fn default() -> Self {
        Self {
            device_args: None,
            pre_codec_args: None,
            codec: None,
            post_codec_args: None,
            pixfmt: Some(DEFAULT_OUTPUT_PIXFMT.to_string()),
            max_bframes: None,
        }
    }
}

fn prefix() -> Vec<String> {
    zq(&["-nostats", "-hide_banner", "-nostdin", "-y"])
}

fn zq(x: &[&str]) -> Vec<String> {
    x.iter().map(|x| (*x).into()).collect()
}

fn zq2(opt_x: Option<&Vec<(String, String)>>) -> Vec<String> {
    if let Some(x) = opt_x {
        x.iter()
            .flat_map(|(x1, x2)| [x1.clone(), x2.clone()])
            .collect()
    } else {
        vec![]
    }
}

impl FfmpegCodecArgs {
    /// Build the full ffmpeg argument list.
    ///
    /// `input_args` are inserted immediately before `-i -` and describe the raw
    /// video arriving on stdin (format, pixel format, size, frame rate, color
    /// range). We also unconditionally force full-range (`pc`) output, appended
    /// after `post_codec_args`, so the full 0-255 intensity range is always
    /// preserved rather than the limited "tv" range: unlike `-pix_fmt`/`-bf`
    /// above, this is not meant to be overridable, so codec presets (see
    /// `from_str` below) must not also set `-color_range` in their own args —
    /// it would just be redundant.
    pub fn to_args(&self, input_args: &[String]) -> Vec<String> {
        const VIDEO_CODEC: &str = "-c:v";
        let output_color_range = zq(&["-color_range", "pc"]);
        let input: Vec<String> = input_args.to_vec();
        let stdin_input = zq(&["-i", "-"]);
        // Emit `-pix_fmt <fmt>` and `-bf <n>` for the output before
        // `post_codec_args` so an explicit `-pix_fmt`/`-bf` in `post_codec_args`
        // still takes precedence.
        let output_pixfmt = match &self.pixfmt {
            Some(pixfmt) => zq(&["-pix_fmt", pixfmt]),
            None => vec![],
        };
        let output_bframes = match &self.max_bframes {
            Some(max_bframes) => vec!["-bf".to_string(), max_bframes.to_string()],
            None => vec![],
        };
        if let Some(codec) = &self.codec {
            vec![
                prefix(),
                zq2(self.device_args.as_ref()),
                input,
                stdin_input,
                zq2(self.pre_codec_args.as_ref()),
                zq(&[VIDEO_CODEC, codec]),
                output_pixfmt,
                output_bframes,
                zq2(self.post_codec_args.as_ref()),
                output_color_range,
            ]
        } else {
            assert_eq!(self.device_args, None);
            assert_eq!(self.pre_codec_args, None);
            assert_eq!(self.post_codec_args, None);
            vec![
                prefix(),
                input,
                stdin_input,
                output_pixfmt,
                output_bframes,
                output_color_range,
            ]
        }
        .into_iter()
        .flatten()
        .collect()
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            // Keep these in sync with the list in strand-cam-remote-control.
            "vaapi" => Some(Self {
                device_args: Some(vec![("-vaapi_device".into(), "/dev/dri/renderD128".into())]),
                pre_codec_args: Some(vec![("-vf".into(), "format=nv12,hwupload".into())]),
                codec: Some("h264_vaapi".to_string()),
                // `to_args` already appends `-color_range pc` unconditionally,
                // so no need to set it here too.
                // The `format=nv12,hwupload` filter chain already fixes the
                // format and the encoder works on hardware surfaces; forcing an
                // output `-pix_fmt` here would conflict.
                pixfmt: None,
                ..Default::default()
            }),
            "videotoolbox" => Some(Self {
                codec: Some("h264_videotoolbox".into()),
                ..Default::default()
            }),
            _ => None,
        }
    }
}

pub fn ffmpeg_version() -> Result<String> {
    let args = ["-hide_banner", "-nostdin", "-version"];
    let ffmpeg_child = Command::new(FFMPEG)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let out = ffmpeg_child.wait_with_output()?;
    let lines = String::from_utf8(out.stdout)?;

    let mut ffmpeg_stderr_iter = lines.split_ascii_whitespace();
    assert_eq!(ffmpeg_stderr_iter.next(), Some("ffmpeg"));
    assert_eq!(ffmpeg_stderr_iter.next(), Some("version"));

    if let Some(version_str) = ffmpeg_stderr_iter.next() {
        Ok(version_str.into())
    } else {
        Err(Error::UnexpectedFfmpegOutput(lines))
    }
}

pub fn platform_hardware_encoder() -> Result<FfmpegCodecArgs> {
    let args = ["-hide_banner", "-nostdin", "-hwaccels"];
    let ffmpeg_child = Command::new(FFMPEG)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let out = ffmpeg_child.wait_with_output()?;
    let lines = String::from_utf8(out.stdout)?;
    let mut lines: VecDeque<_> = lines.split("\n").collect();
    let line0 = lines.pop_front().unwrap();
    if line0 != "Hardware acceleration methods:" {
        return Err(Error::UnexpectedFfmpegOutput(line0.into()));
    }
    for line in lines.into_iter() {
        if let Some(opt) = FfmpegCodecArgs::from_str(line) {
            return Ok(opt);
        }
    }
    Ok(FfmpegCodecArgs {
        ..Default::default()
    })
}

/// Write one frame's pixel rows to `w`, stripping any stride padding.
///
/// This is the raw-video framing ffmpeg's `-f rawvideo` demuxer expects on
/// stdin: each row's bytes back-to-back, with no stride/alignment padding.
pub fn write_frame_rows<W: Write>(
    frame: &strand_dynamic_frame::DynamicFrame,
    w: &mut W,
) -> Result<()> {
    strand_dynamic_frame::match_all_dynamic_fmts!(
        frame,
        x,
        {
            use machine_vision_formats::iter::HasRowChunksExact;
            for row in x.rowchunks_exact() {
                w.write_all(row)?;
            }
            Ok(())
        },
        // Reached only for formats `FfmpegWriter::start` did not already reject.
        Error::UnimplementedPixelFormat(frame.pixel_format())
    )
}

impl FfmpegWriter {
    pub fn new(
        fname: &str,
        ffmpeg_codec_args: FfmpegCodecArgs,
        rate: Option<(usize, usize)>,
    ) -> Result<Self> {
        let (raten, rated) = rate.unwrap_or((25, 1));
        // ffmpeg is spawned lazily on the first frame, once we know the frame
        // width, height and pixel format needed for the raw-video input options.
        Ok(Self {
            fname: fname.to_string(),
            ffmpeg_codec_args,
            raten,
            rated,
            count: 0,
            sink: None,
        })
    }

    /// Write a frame. Return the presentation timestamp (PTS).
    pub fn write_dynamic_frame(
        &mut self,
        frame: &strand_dynamic_frame::DynamicFrame,
    ) -> Result<std::time::Duration> {
        let sink = match &mut self.sink {
            Some(sink) => sink,
            None => self.sink.insert(FfmpegFrameSink::new(
                frame,
                &self.ffmpeg_codec_args,
                (self.raten, self.rated),
                FfmpegOutput::File(self.fname.clone()),
            )?),
        };
        sink.send(frame)?;

        let num = self.rated * self.count;
        let dur_sec = num as f64 / self.raten as f64;
        let pts = std::time::Duration::from_secs_f64(dur_sec);
        self.count += 1;
        Ok(pts)
    }

    pub fn close(self) -> Result<()> {
        match self.sink {
            Some(sink) => sink.close(),
            // No frames were ever written, so ffmpeg was never spawned.
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use machine_vision_formats::PixFmt;
    use strand_dynamic_frame::DynamicFrameOwned;

    /// The default codec args force `-pix_fmt yuv420p` on the output (so the
    /// result is decodable by OpenH264, which only supports 4:2:0), and that
    /// pixel format appears after the codec but before `post_codec_args` so an
    /// explicit `-pix_fmt` there can still override it. `None` omits it entirely.
    #[test]
    fn to_args_emits_output_pixfmt() {
        let default_args = FfmpegCodecArgs {
            codec: Some("libx264".to_string()),
            ..Default::default()
        };
        let args = default_args.to_args(&[]);
        let pixfmt_at = args.iter().position(|a| a == "-pix_fmt").unwrap();
        assert_eq!(args[pixfmt_at + 1], "yuv420p");
        let codec_at = args.iter().position(|a| a == "-c:v").unwrap();
        assert!(codec_at < pixfmt_at, "pix_fmt must come after the codec");

        // A `None` pixfmt/max_bframes emits no `-pix_fmt`/`-bf`.
        let no_pixfmt = FfmpegCodecArgs {
            codec: Some("libx264".to_string()),
            pixfmt: None,
            max_bframes: None,
            ..Default::default()
        };
        let args = no_pixfmt.to_args(&[]);
        assert!(!args.iter().any(|a| a == "-pix_fmt"));
        assert!(!args.iter().any(|a| a == "-bf"));

        // The struct pixfmt precedes post_codec_args, so an explicit `-pix_fmt`
        // there is the last one and wins in ffmpeg.
        let overridden = FfmpegCodecArgs {
            codec: Some("libx264".to_string()),
            post_codec_args: Some(vec![("-pix_fmt".into(), "yuv444p".into())]),
            ..Default::default()
        };
        let args = overridden.to_args(&[]);
        let last_pixfmt = args.iter().rposition(|a| a == "-pix_fmt").unwrap();
        assert_eq!(args[last_pixfmt + 1], "yuv444p");
    }

    /// NV12 carries one interleaved chroma pair per 2x2 block of luma, rounding
    /// up, so an odd width or height still gets a whole sample.
    #[test]
    fn nv12_chroma_len_rounds_odd_dimensions_up() {
        assert_eq!(MonoFraming::Gray.chroma_len(640, 480), 0);
        assert_eq!(MonoFraming::Nv12.chroma_len(640, 480), 320 * 240 * 2);
        assert_eq!(MonoFraming::Nv12.chroma_len(641, 481), 321 * 241 * 2);
        assert_eq!(MonoFraming::Nv12.chroma_len(2, 2), 2);
    }

    /// An inconclusive probe must still leave a usable framing, and it must be
    /// the one that does not depend on ffmpeg synthesizing chroma.
    #[test]
    fn every_probe_outcome_yields_a_framing() {
        assert_eq!(MonoFramingProbe::GrayIsSafe.framing(), MonoFraming::Gray);
        assert_eq!(MonoFramingProbe::Nv12Required.framing(), MonoFraming::Nv12);
        assert_eq!(MonoFramingProbe::Inconclusive.framing(), MonoFraming::Nv12);
    }

    /// A configuration that forces a semi-planar encoder input, which is where
    /// the chroma bug lives. `libx264` accepts `nv12` directly, so this
    /// reproduces on any machine without needing a GPU.
    fn forces_semi_planar_input() -> FfmpegCodecArgs {
        FfmpegCodecArgs {
            codec: Some("libx264".to_string()),
            pixfmt: Some("nv12".to_string()),
            max_bframes: None,
            ..Default::default()
        }
    }

    /// Record a few mono frames through the writer and report whether the
    /// chroma survived as neutral.
    fn recorded_mono_chroma_is_neutral(
        codec_args: &FfmpegCodecArgs,
        width: u32,
        height: u32,
    ) -> bool {
        let tmp = tempfile::tempdir().unwrap();
        let out_path = tmp.path().join("mono.mp4");
        {
            let mut wtr = FfmpegWriter::new(
                out_path.to_str().unwrap(),
                codec_args.clone(),
                Some((25, 1)),
            )
            .unwrap();
            let frame = mono_ramp(width, height);
            for _ in 0..3 {
                wtr.write_dynamic_frame(&frame.borrow()).unwrap();
            }
            wtr.close().unwrap();
        }
        decoded_chroma_is_neutral(&out_path, width, height).unwrap()
    }

    /// The whole point: whatever this machine's ffmpeg turns out to do, the
    /// framing the probe settles on must record a mono camera with neutral
    /// chroma. Version-independent by construction -- an ffmpeg with the bug
    /// should come back `Nv12Required` and one without it `GrayIsSafe`, and
    /// either way the recording must not be green.
    #[test]
    fn the_probed_framing_records_a_mono_camera_neutral() {
        // A size of its own, so this test's cache entry cannot be confused with
        // another's.
        let (width, height) = (320u32, 240u32);
        let codec_args = forces_semi_planar_input();

        let outcome = probe_mono_framing(&codec_args, width, height);
        assert_ne!(
            outcome,
            MonoFramingProbe::Inconclusive,
            "the probe must reach a verdict for a configuration ffmpeg accepts"
        );
        assert!(
            recorded_mono_chroma_is_neutral(&codec_args, width, height),
            "recording with the probed framing ({outcome:?}) came out green"
        );
        assert_eq!(
            cached_probe(&codec_args, width, height),
            Some(outcome),
            "the verdict must be cached, so recording never pays for it again"
        );
        // Asking again must agree, and comes from the cache.
        assert_eq!(probe_mono_framing(&codec_args, width, height), outcome);
    }

    /// A configuration nobody probed must still record correctly. This is the
    /// path taken when a recording starts before anything has had a chance to
    /// probe, and it is why the fallback is NV12 rather than `gray`: piping
    /// `gray` here would come out green on an affected ffmpeg.
    #[test]
    fn an_unprobed_configuration_records_a_mono_camera_neutral() {
        // Never probed at this size, so the writer has nothing to go on.
        let (width, height) = (352u32, 288u32);
        let codec_args = forces_semi_planar_input();
        assert_eq!(cached_probe(&codec_args, width, height), None);

        assert!(
            recorded_mono_chroma_is_neutral(&codec_args, width, height),
            "an unprobed configuration must fall back to a framing that is right \
             everywhere"
        );
    }

    /// A mono ramp frame, the shape a tracking camera produces.
    fn mono_ramp(width: u32, height: u32) -> DynamicFrameOwned {
        let mut buf = vec![0u8; (width * height) as usize];
        for y in 0..height as usize {
            for x in 0..width as usize {
                buf[y * width as usize + x] = (x * 255 / (width as usize - 1)) as u8;
            }
        }
        DynamicFrameOwned::from_buf(width, height, width as usize, buf, PixFmt::Mono8).unwrap()
    }

    /// The whole reason the sink is shared: a caller taking the encoded stream
    /// back on a pipe (as the RTP streamer does) gets the mono framing decision
    /// for free, so it cannot record green video by forgetting to ask for it.
    ///
    /// Configured to force a semi-planar encoder input, which is where the bug
    /// lives, and the same check applied as for a file recording: the chroma
    /// that comes back out must be neutral.
    #[test]
    fn a_piped_sink_frames_mono_the_same_way_a_file_sink_does() {
        // A size of its own, so this test's cache entry cannot be confused
        // with another's.
        let (width, height) = (192u32, 144u32);
        let codec_args = FfmpegCodecArgs {
            post_codec_args: Some(vec![("-f".into(), "h264".into())]),
            ..forces_semi_planar_input()
        };
        let frame = mono_ramp(width, height);

        let mut sink =
            FfmpegFrameSink::new(&frame.borrow(), &codec_args, (25, 1), FfmpegOutput::Stdout)
                .unwrap();
        assert_eq!(sink.geometry(), (PixFmt::Mono8, width, height));

        // Read stdout on a thread: an elementary stream that nobody drains
        // fills the pipe and deadlocks the child mid-`send`.
        let mut stdout = sink.take_stdout().expect("a piped sink has a stdout");
        let reader = std::thread::spawn(move || {
            let mut encoded = Vec::new();
            std::io::Read::read_to_end(&mut stdout, &mut encoded).unwrap();
            encoded
        });
        for _ in 0..3 {
            sink.send(&frame.borrow()).unwrap();
        }
        sink.close().unwrap();
        let encoded = reader.join().unwrap();

        assert!(
            encoded.starts_with(&[0, 0, 0, 1]) || encoded.starts_with(&[0, 0, 1]),
            "expected an Annex-B elementary stream, got {:?}",
            &encoded[..encoded.len().min(8)]
        );

        // Decoding wants a path, and this is an elementary stream rather than a
        // container, so name it for what it is.
        let tmp = tempfile::tempdir().unwrap();
        let stream_path = tmp.path().join("piped.h264");
        std::fs::write(&stream_path, &encoded).unwrap();
        assert!(
            decoded_chroma_is_neutral(&stream_path, width, height).unwrap(),
            "a piped mono recording came out green"
        );
    }

    /// A sink whose ffmpeg died must say why. Before the shared sink existed,
    /// the streaming caller got a bare `BrokenPipe` with ffmpeg's actual
    /// complaint discarded.
    #[test]
    fn a_dead_ffmpeg_reports_its_own_complaint() {
        let (width, height) = (64u32, 48u32);
        let frame = mono_ramp(width, height);
        let codec_args = FfmpegCodecArgs {
            codec: Some("no-such-codec".to_string()),
            ..Default::default()
        };
        let mut sink = FfmpegFrameSink::new(
            &frame.borrow(),
            &codec_args,
            (25, 1),
            FfmpegOutput::File(
                tempfile::tempdir()
                    .unwrap()
                    .path()
                    .join("never-written.mp4")
                    .to_str()
                    .unwrap()
                    .to_string(),
            ),
        )
        .unwrap();

        // ffmpeg rejects the codec and exits at startup, but the first frames
        // may still land in the pipe buffer before the write fails, so keep
        // sending until it does. A frame this size fills a pipe within a few
        // sends.
        let mut err = None;
        for _ in 0..64 {
            if let Err(e) = sink.send(&frame.borrow()) {
                err = Some(e);
                break;
            }
        }
        let err = err.expect("sending to a dead ffmpeg must eventually fail");
        let Error::FfmpegExited { status, stderr } = err else {
            panic!("expected FfmpegExited, got {err:?}");
        };
        assert!(!status.success());
        assert!(
            stderr.contains("no-such-codec"),
            "the error must carry ffmpeg's own complaint, got: {stderr:?}"
        );
    }

    /// One pixel format's lossless round-trip case.
    struct Case {
        pixfmt: PixFmt,
        /// The ffmpeg raw-video pixel-format name the mapping should produce.
        /// Hardcoded (not read from the code under test) so it is an independent
        /// ground truth: the writer encodes using `ffmpeg_pixel_format(pixfmt)`,
        /// while we decode back using this. If the mapping were wrong, encode
        /// and decode would disagree and the round trip would not match.
        ffmpeg_pixfmt: &'static str,
        /// Bytes per pixel of the packed layout (used to compute row size).
        bytes_per_pixel: usize,
        /// A lossless codec that preserves this format's bytes exactly.
        codec: &'static str,
        /// Container extension matching `codec`.
        ext: &'static str,
    }

    /// Decode the first (only) video frame of `path` back to tightly packed raw
    /// bytes in `pix_fmt`, via ffmpeg.
    fn ffmpeg_decode_raw(path: &std::path::Path, pix_fmt: &str) -> Vec<u8> {
        let output = std::process::Command::new(FFMPEG)
            .args(["-nostdin", "-loglevel", "error", "-i"])
            .arg(path)
            .args(["-f", "rawvideo", "-pix_fmt", pix_fmt, "-"])
            .output()
            .expect("running ffmpeg to decode the recording");
        assert!(
            output.status.success(),
            "ffmpeg decode failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    }

    fn assert_roundtrips_exactly(case: &Case) {
        // Direct check of the mapping under test against the ground truth.
        assert_eq!(
            ffmpeg_pixel_format(case.pixfmt).unwrap(),
            case.ffmpeg_pixfmt,
            "unexpected ffmpeg pixel-format mapping for {:?}",
            case.pixfmt
        );
        let ffmpeg_pixfmt = case.ffmpeg_pixfmt;
        let (width, height) = (64u32, 48u32);
        let valid_stride = width as usize * case.bytes_per_pixel;
        // Give the frame stride padding so we also exercise the writer stripping
        // it off before piping (rows must arrive tightly packed).
        let pad = 16usize;
        let stride = valid_stride + pad;

        // Deterministic, non-constant content so any misframing or channel-order
        // mistake in the pixel-format mapping would change the decoded bytes.
        let mut buf = vec![0xAAu8; height as usize * stride]; // padding sentinel
        let mut expected = Vec::with_capacity(height as usize * valid_stride);
        for row in 0..height as usize {
            for i in 0..valid_stride {
                let v = ((row * valid_stride + i) * 31 + 7) as u8;
                buf[row * stride + i] = v;
                expected.push(v);
            }
        }

        let frame = DynamicFrameOwned::from_buf(width, height, stride, buf, case.pixfmt).unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let out_path = tmp.path().join(format!("roundtrip.{}", case.ext));
        {
            let codec_args = FfmpegCodecArgs {
                codec: Some(case.codec.to_string()),
                // This test asserts a byte-exact lossless roundtrip in the
                // frame's native pixel format, so we must NOT force an output
                // `-pix_fmt` (which would convert the pixels) nor `-bf` (the
                // rawvideo/ffv1 encoders reject it).
                pixfmt: None,
                max_bframes: None,
                ..Default::default()
            };
            // Pin the mono framing, so a mono case really does exercise the
            // `gray` mapping this test is checking. Left unpinned it would be
            // piped as NV12, the safe default for an unprobed configuration,
            // and the mapping would go untested. Truthful as well as
            // convenient: these codecs take gray natively, so there is no
            // conversion here for the chroma bug to affect.
            remember_probe(&codec_args, width, height, MonoFramingProbe::GrayIsSafe);
            let mut wtr = FfmpegWriter::new(out_path.to_str().unwrap(), codec_args, None).unwrap();
            wtr.write_dynamic_frame(&frame.borrow()).unwrap();
            wtr.close().unwrap();
        }

        let got = ffmpeg_decode_raw(&out_path, ffmpeg_pixfmt);
        assert_eq!(
            got.len(),
            expected.len(),
            "{:?} ({ffmpeg_pixfmt}): decoded byte count differs",
            case.pixfmt
        );
        assert!(
            got == expected,
            "{:?} ({ffmpeg_pixfmt}): pixel data did not round-trip exactly through ffmpeg",
            case.pixfmt
        );
    }

    /// Frame data piped raw to ffmpeg (see the crate docs / `ffmpeg_pixel_format`)
    /// must survive a round trip byte-for-byte. Mono8/RGB8 go through the lossless
    /// FFV1 codec, which also interprets the colorspace and so catches
    /// channel-order mistakes (e.g. RGB vs BGR). YUV422 and Bayer use the verbatim
    /// `rawvideo` codec: FFV1 has no packed 4:2:2 format, so encoding uyvy422 with
    /// it forces a chroma repack through swscale that is not bit-exact across
    /// ffmpeg versions, and Bayer has no non-debayering codec at all. For those
    /// two, the pixel-format mapping is instead guarded by the direct
    /// `ffmpeg_pixel_format` assertion above; that the real (H.264) encoder accepts
    /// each format is covered by the sim smoke test.
    #[test]
    fn frame_data_roundtrips_exactly_via_ffmpeg() {
        let cases = [
            Case {
                pixfmt: PixFmt::Mono8,
                ffmpeg_pixfmt: "gray",
                bytes_per_pixel: 1,
                codec: "ffv1",
                ext: "mkv",
            },
            Case {
                pixfmt: PixFmt::RGB8,
                ffmpeg_pixfmt: "rgb24",
                bytes_per_pixel: 3,
                codec: "ffv1",
                ext: "mkv",
            },
            Case {
                pixfmt: PixFmt::YUV422,
                ffmpeg_pixfmt: "uyvy422",
                bytes_per_pixel: 2,
                codec: "rawvideo",
                ext: "nut",
            },
            Case {
                pixfmt: PixFmt::BayerRG8,
                ffmpeg_pixfmt: "bayer_rggb8",
                bytes_per_pixel: 1,
                codec: "rawvideo",
                ext: "nut",
            },
        ];
        for case in &cases {
            assert_roundtrips_exactly(case);
        }
    }
}
