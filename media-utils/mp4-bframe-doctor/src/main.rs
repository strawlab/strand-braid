// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Detect MP4 or raw Annex B `.h264` files whose timing metadata is
//! inconsistent with the true presentation order encoded in the H.264
//! bitstream itself.
//!
//! There are two places a recording states timing, and either can be wrong:
//!
//!  * the MP4 container's `stts`/`ctts` boxes, which is what a player uses to
//!    order frames; and
//!  * the per-frame precision-timestamp SEI embedded in the bitstream (written
//!    by strand-cam / braid), which can itself be mistagged at record time
//!    (e.g. paired with the wrong encoder output when B-frame reordering delays
//!    that output relative to when it was submitted), independent of what the
//!    container boxes say.
//!
//! The one signal that cannot lie is the bitstream's own picture order count
//! (POC, ITU-T H.264 §8.2.1): every slice header carries enough information to
//! reconstruct the true relative display order of samples, independent of any
//! container metadata or of what a (possibly buggy) writer put in the SEI. This
//! tool decodes POC for every sample and checks that, walked in POC order, each
//! available timing series comes out non-decreasing. It checks the container
//! timing for every MP4 (so even a plain ffmpeg recording with no SEI can be
//! verified) and the precision-timestamp SEI wherever it is present (the only
//! signal for a raw `.h264` file). Any series that is not monotonic in POC
//! order means that timing disagrees with the bitstream's real display order,
//! and the file is broken.

use camino::Utf8PathBuf;
use clap::{Parser, Subcommand};
use eyre::{Context, Result, bail, eyre};
use h264_reader::nal::{Nal, RefNal, UnitType};

use frame_source::{
    FrameDataSource,
    h264_poc::PocReader,
    h264_source::{H264Source, SeekableH264Source},
};
use pending_file::PendingFile;
use strand_cam_remote_control::{Mp4Codec, Mp4RecordingConfig, RecordingFrameRate};

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Report whether the SEI precision timestamps in MP4 or raw Annex B
    /// `.h264` files are consistent with the true presentation order encoded
    /// in the H.264 bitstream (its picture order count, POC).
    Check {
        #[arg(required = true, num_args = 1..)]
        inputs: Vec<Utf8PathBuf>,
    },
    /// Repair a file in place by reassigning its capture timestamps to frames
    /// in true (bitstream POC) display order and writing a new MP4 whose
    /// container timing and SEI both agree with that order. See
    /// [`repair_timing`] for the assumption this relies on.
    Fix {
        /// Input MP4 or raw Annex B `.h264` file. Repaired in place: the
        /// original is renamed to `<input>.bak` (or `.bak.1`, `.bak.2`, ... if
        /// that already exists) and the repaired MP4 is written to `<input>`.
        input: Utf8PathBuf,
        /// Rewrite even if analysis says the file is already fine.
        #[arg(long)]
        force: bool,
    },
}

/// One H.264 sample, in the order it appears in the file (decode order).
struct LoadedFrame {
    /// The sample's claimed presentation/capture time (nanoseconds, relative to
    /// frame0) under the timing source being checked -- either the MP4
    /// container timing (`stts`/`ctts`) or the per-frame precision-timestamp SEI.
    pts_ns: i64,
    /// Picture order count, reconstructed from the bitstream's own slice
    /// headers (ITU-T H.264 §8.2.1). Only comparable in relative order
    /// *within one coded video sequence* -- POC restarts at every IDR -- and not
    /// a real time value. Sort by [`LoadedFrame::display_key`], never by `poc`
    /// alone.
    poc: i64,
    /// Which coded video sequence this sample belongs to: 0 for the first, then
    /// one more at each IDR. Needed because POC restarts there, so a bare POC
    /// sort interleaves frames from different groups of pictures.
    cvs: i64,
}

impl LoadedFrame {
    /// Sort key putting samples in true display order across the whole file.
    fn display_key(&self) -> (i64, i64) {
        (self.cvs, self.poc)
    }
}

/// Failure loading a timing series for one timestamp source.
enum LoadError {
    /// The file simply lacks the requested per-frame timestamps (e.g. a plain
    /// ffmpeg-encoded MP4 has no precision-timestamp SEI). The caller can just
    /// skip that particular check.
    NoTimestamps,
    /// A real failure (unreadable, unsupported POC type, etc.).
    Other(eyre::Report),
}

/// Load per-frame `(poc, timestamp)` for `path` using `ts_source` as the timing
/// to compare against the bitstream POC. Both container types decode to the
/// same H.264 samples; only the builder differs, so the per-frame analysis in
/// [`load_frames`] is shared.
fn load(
    path: &Utf8PathBuf,
    ts_source: frame_source::TimestampSource,
) -> std::result::Result<Vec<LoadedFrame>, LoadError> {
    let builder = frame_source::FrameSourceBuilder::new(path)
        .do_decode_h264(false)
        .timestamp_source(ts_source);

    match path.extension().map(|e| e.to_lowercase()).as_deref() {
        Some("mp4") => {
            let mut src = builder.build_h264_in_mp4_source().map_err(build_err)?;
            load_frames(&mut src).map_err(LoadError::Other)
        }
        Some("h264") => {
            let mut src = builder.build_h264_annexb_source().map_err(build_err)?;
            load_frames(&mut src).map_err(LoadError::Other)
        }
        _ => Err(LoadError::Other(eyre!(
            "\"{path}\": unsupported extension (expected .mp4 or .h264)"
        ))),
    }
}

/// Classify a source-open error: a timestamp error means the requested timing
/// is simply absent; anything else is a real failure.
fn build_err(e: frame_source::Error) -> LoadError {
    match e {
        frame_source::Error::H264TimestampError(_) => LoadError::NoTimestamps,
        other => LoadError::Other(eyre::Report::new(other)),
    }
}

/// One timing signal checked against the bitstream POC for a file.
struct SourceCheck {
    /// Human-readable name of the timing source.
    label: &'static str,
    analysis: Analysis,
}

/// Total span of a timing series: last timestamp minus first, however the
/// samples happen to be ordered.
fn span_ns(frames: &[LoadedFrame]) -> i64 {
    match (
        frames.iter().map(|f| f.pts_ns).min(),
        frames.iter().map(|f| f.pts_ns).max(),
    ) {
        (Some(min), Some(max)) => max - min,
        _ => 0,
    }
}

/// Does the container play the recording back over the same length of time it
/// took to capture?
///
/// Ordering is not the only way container timing goes wrong: it can be in the
/// right order and still run at the wrong *rate*. That is what the
/// `real_span / source_span` scaling bug did -- it divided by the sum of all `n`
/// sample durations while the capture span covers only `n-1` intervals, so
/// playback came out a factor `n/(n-1)` too fast. [`analyze`] cannot see this at
/// all: a uniformly compressed timeline is still perfectly monotonic.
///
/// Compared as *spans* rather than per frame, because a container is entitled to
/// differ from the SEI frame by frame. Capture timestamps carry noise the
/// acquisition clock does not, so a writer may deliberately snap presentation
/// times to an estimated cadence; that changes individual frames but preserves
/// the total, so a span comparison ignores it and still catches a systematic rate
/// error.
struct CadenceCheck {
    container_span_ns: i64,
    sei_span_ns: i64,
    /// Mean capture interval -- one frame's worth of time.
    mean_interval_ns: i64,
}

impl CadenceCheck {
    /// How far the container's total timeline is from the captured one.
    fn drift_ns(&self) -> i64 {
        (self.container_span_ns - self.sei_span_ns).abs()
    }

    /// Fractional rate error, positive when the container plays too fast.
    fn rate_error(&self) -> f64 {
        if self.container_span_ns == 0 {
            return 0.0;
        }
        self.sei_span_ns as f64 / self.container_span_ns as f64 - 1.0
    }

    /// Broken once the whole timeline is out by more than half a frame.
    ///
    /// Half a frame is the natural threshold rather than an arbitrary percentage:
    /// quantizing into the movie timescale costs only a tick or two, while the
    /// `n/(n-1)` bug drifts by `span/n`, which *is* the mean interval -- one whole
    /// frame, at any recording length. So this catches that bug whether the file
    /// holds 20 frames or 20000, and stays quiet for merely rounded timing.
    fn is_broken(&self) -> bool {
        self.mean_interval_ns > 0 && self.drift_ns() * 2 > self.mean_interval_ns
    }

    fn describe(&self) -> String {
        let faster_or_slower = if self.container_span_ns < self.sei_span_ns {
            "fast"
        } else {
            "slow"
        };
        format!(
            "container plays {:.2}% too {faster_or_slower}: its timeline spans {:.1}ms \
             against {:.1}ms of capture, a drift of {:.1}ms (~{:.2} frames)",
            self.rate_error().abs() * 100.0,
            self.container_span_ns as f64 / 1e6,
            self.sei_span_ns as f64 / 1e6,
            self.drift_ns() as f64 / 1e6,
            self.drift_ns() as f64 / self.mean_interval_ns as f64,
        )
    }
}

/// Does the container present each frame when it was captured?
///
/// The old `mp4-writer` took each sample's duration from the interval *before*
/// the frame instead of the one after it. The first sample got a duration of
/// zero, and every frame was presented one capture interval early: frame `r` at
/// the capture time of frame `r-1`, with frames 0 and 1 sharing a presentation
/// time. Every strand-cam/braid and flo MP4 written before that fix has this
/// signature. [`CadenceCheck`] usually notices too, because the timeline also
/// comes up one final interval short, but it reports that as a playback rate
/// error of a fraction of a percent. This check names the actual fault.
///
/// Each frame is measured against the first frame in display order, as
/// `(container_r - container_0) - (capture_r - capture_0)`. Anchoring both
/// series there cancels any constant lead-in, such as a reordered stream's
/// composition offset when the edit list is not applied. The *median* of those
/// offsets is compared against half a mean interval, the same
/// length-independent threshold [`CadenceCheck`] uses. The median ignores the
/// frame-by-frame jitter that a writer which snaps presentation to an estimated
/// cadence introduces on purpose, while a whole-frame shift moves it by a full
/// interval.
struct OffsetCheck {
    /// Median of each frame's container offset relative to its capture time,
    /// both measured from the first frame. Negative when the container presents
    /// frames early.
    median_offset_ns: i64,
    /// Mean capture interval -- one frame's worth of time.
    mean_interval_ns: i64,
}

impl OffsetCheck {
    /// Broken once frames are typically presented more than half a frame away
    /// from when they were captured.
    fn is_broken(&self) -> bool {
        self.mean_interval_ns > 0 && self.median_offset_ns.abs() * 2 > self.mean_interval_ns
    }

    fn describe(&self) -> String {
        let early_or_late = if self.median_offset_ns < 0 {
            "early"
        } else {
            "late"
        };
        format!(
            "container presents frames {:.1}ms {early_or_late} relative to the first \
             frame's capture time (median, ~{:.2} frames)",
            self.median_offset_ns.abs() as f64 / 1e6,
            self.median_offset_ns.abs() as f64 / self.mean_interval_ns as f64,
        )
    }
}

/// Everything checked for one file.
struct FileReport {
    /// Each available timing series, checked against bitstream display order.
    sources: Vec<SourceCheck>,
    /// Container rate against capture rate. `None` unless the file has both
    /// container timing and SEI, and at least two frames spanning some time.
    cadence: Option<CadenceCheck>,
    /// Container presentation time against capture time, frame by frame. Same
    /// availability as `cadence`.
    offset: Option<OffsetCheck>,
}

impl FileReport {
    /// One description per failed check; empty when the file is fine.
    fn problems(&self) -> Vec<String> {
        let mut details: Vec<String> = self
            .sources
            .iter()
            .filter(|c| c.analysis.is_broken())
            .map(|c| {
                format!(
                    "{}: {} of {} samples inconsistent with bitstream POC order, up \
                    to {:.1}ms early",
                    c.label,
                    c.analysis.num_inversions,
                    c.analysis.num_frames,
                    c.analysis.max_inversion_ms
                )
            })
            .collect();
        if let Some(cadence) = self.cadence.as_ref().filter(|c| c.is_broken()) {
            details.push(cadence.describe());
        }
        if let Some(offset) = self.offset.as_ref().filter(|c| c.is_broken()) {
            details.push(offset.describe());
        }
        details
    }

    fn num_frames(&self) -> usize {
        self.sources
            .first()
            .map(|c| c.analysis.num_frames)
            .unwrap_or(0)
    }
}

/// Run every applicable check for `path`.
///
/// An MP4 always carries container timing (the `stts`/`ctts` boxes, which is
/// what players use to order frames), so that is checked for every MP4 --
/// including plain ffmpeg recordings with no SEI. The per-frame
/// precision-timestamp SEI (written by strand-cam / braid) is checked whenever
/// it is present; for a raw Annex B `.h264` file it is the only signal. When both
/// are present they are also checked against each other for rate, which is a
/// different fault from ordering; see [`CadenceCheck`].
fn check_file(path: &Utf8PathBuf) -> Result<FileReport> {
    let is_mp4 = path.extension().map(|e| e.to_lowercase()).as_deref() == Some("mp4");
    let mut sources = Vec::new();
    let mut container_frames = None;
    let mut sei_frames = None;

    if is_mp4 {
        let frames = load(path, frame_source::TimestampSource::Mp4Pts).map_err(|e| match e {
            LoadError::NoTimestamps => eyre!("\"{path}\": MP4 has no container sample timing"),
            LoadError::Other(r) => r,
        })?;
        sources.push(SourceCheck {
            label: "container (stts/ctts)",
            analysis: analyze(&frames),
        });
        container_frames = Some(frames);
    }

    match load(path, frame_source::TimestampSource::MispMicrosectime) {
        Ok(frames) => {
            sources.push(SourceCheck {
                label: "precision-timestamp SEI",
                analysis: analyze(&frames),
            });
            sei_frames = Some(frames);
        }
        Err(LoadError::NoTimestamps) => {
            if !is_mp4 {
                bail!(
                    "\"{path}\" has no per-frame precision-timestamp SEI and no container timing, \
                    so there is nothing for this tool to check"
                );
            }
            // An MP4 without SEI is still covered by the container check above.
        }
        Err(LoadError::Other(r)) => return Err(r),
    }

    let (cadence, offset) = match (container_frames, sei_frames) {
        (Some(container), Some(sei)) => (
            cadence_check(&container, &sei),
            offset_check(&container, &sei),
        ),
        _ => (None, None),
    };

    Ok(FileReport {
        sources,
        cadence,
        offset,
    })
}

/// Build a [`CadenceCheck`], or `None` when there is no cadence to speak of.
fn cadence_check(container: &[LoadedFrame], sei: &[LoadedFrame]) -> Option<CadenceCheck> {
    // Mismatched counts mean something more basic is wrong than the rate.
    if container.len() != sei.len() || container.len() < 2 {
        return None;
    }
    let sei_span_ns = span_ns(sei);
    // A recording with no elapsed capture time has no rate to compare.
    if sei_span_ns <= 0 {
        return None;
    }
    Some(CadenceCheck {
        container_span_ns: span_ns(container),
        sei_span_ns,
        mean_interval_ns: sei_span_ns / (sei.len() as i64 - 1),
    })
}

/// Build an [`OffsetCheck`], or `None` when there is nothing to compare.
///
/// `container` and `sei` are the same samples in the same (decode) order, so
/// index `i` is one frame in both.
fn offset_check(container: &[LoadedFrame], sei: &[LoadedFrame]) -> Option<OffsetCheck> {
    if container.len() != sei.len() || container.len() < 2 {
        return None;
    }
    let sei_span_ns = span_ns(sei);
    if sei_span_ns <= 0 {
        return None;
    }
    let first = (0..sei.len()).min_by_key(|&i| sei[i].display_key())?;
    let mut offsets: Vec<i64> = (0..sei.len())
        .filter(|&i| i != first)
        .map(|i| {
            (container[i].pts_ns - container[first].pts_ns) - (sei[i].pts_ns - sei[first].pts_ns)
        })
        .collect();
    offsets.sort_unstable();
    Some(OffsetCheck {
        median_offset_ns: offsets[offsets.len() / 2],
        mean_interval_ns: sei_span_ns / (sei.len() as i64 - 1),
    })
}

/// Extract the raw-EBSP NAL units of one decoded (non-decoded) H.264 sample.
fn frame_nals(frame: frame_source::FrameData) -> Result<Vec<Vec<u8>>> {
    match frame.into_image() {
        frame_source::ImageData::EncodedH264(encoded) => match encoded.data {
            frame_source::H264EncodingVariant::RawEbsp(nals) => Ok(nals),
            other => bail!("expected raw-EBSP H264 sample data, got {other:?}"),
        },
        other => bail!("expected H264-encoded frame data, got {other:?}"),
    }
}

/// Does this sample start a new coded video sequence? An IDR resets POC, so it
/// is where the display-order sort key has to be bumped.
fn starts_coded_video_sequence(nals: &[Vec<u8>]) -> bool {
    nals.iter().any(|nal_bytes| {
        let nal = RefNal::new(nal_bytes, &[], true);
        matches!(nal.header(), Ok(h)
            if h.nal_unit_type() == UnitType::SliceLayerWithoutPartitioningIdr)
    })
}

/// Reconstruct picture order count (POC) and read the SEI capture time for
/// every sample of an already-opened H.264 source.
fn load_frames<H: SeekableH264Source>(src: &mut H264Source<H>) -> Result<Vec<LoadedFrame>> {
    let mut reader = PocReader::new();
    reader.seed_from_container(src)?;

    let mut frames = Vec::new();
    let mut cvs = 0i64;
    for frame in src.decode_order_iter() {
        let frame = frame?;
        let pts_ns = frame.timestamp().unwrap_duration().as_nanos() as i64;
        let nals = frame_nals(frame)?;
        let poc = reader.poc_for_frame(&nals)?;
        if starts_coded_video_sequence(&nals) && !frames.is_empty() {
            cvs += 1;
        }
        frames.push(LoadedFrame { pts_ns, poc, cvs });
    }

    Ok(frames)
}

struct Analysis {
    num_frames: usize,
    num_inversions: usize,
    max_inversion_ms: f64,
}

impl Analysis {
    fn is_broken(&self) -> bool {
        self.num_inversions > 0
    }
}

/// Compare the bitstream's true picture order against a timing series: sort
/// samples into display order and check whether the timestamps come out
/// non-decreasing.
///
/// Ordering is by `(coded video sequence, POC)`, not by POC alone. POC restarts
/// at every IDR, so on a file with more than one group of pictures a bare POC
/// sort interleaves frames from different GOPs and reports inversions in a
/// perfectly good recording.
fn analyze(frames: &[LoadedFrame]) -> Analysis {
    let mut order: Vec<usize> = (0..frames.len()).collect();
    order.sort_by_key(|&i| frames[i].display_key());

    let mut num_inversions = 0usize;
    let mut max_inversion_ns = 0i64;
    let mut prev: Option<i64> = None;
    for &i in &order {
        let t = frames[i].pts_ns;
        if let Some(p) = prev
            && t < p
        {
            num_inversions += 1;
            max_inversion_ns = max_inversion_ns.max(p - t);
        }
        prev = Some(t);
    }

    Analysis {
        num_frames: frames.len(),
        num_inversions,
        max_inversion_ms: max_inversion_ns as f64 / 1e6,
    }
}

fn cmd_check(inputs: &[Utf8PathBuf]) -> Result<bool> {
    let mut any_broken = false;
    for path in inputs {
        match check_file(path) {
            Ok(report) => {
                let details = report.problems();
                if !details.is_empty() {
                    any_broken = true;
                    println!("BROKEN  {path}  ({})", details.join("; "));
                } else {
                    let num_frames = report.num_frames();
                    let mut checked: Vec<&str> = report.sources.iter().map(|c| c.label).collect();
                    if report.cadence.is_some() {
                        checked.push("container-vs-capture rate");
                    }
                    if report.offset.is_some() {
                        checked.push("container-vs-capture offset");
                    }
                    println!(
                        "OK      {path}  ({num_frames} samples; checked: {})",
                        checked.join(", ")
                    );
                }
            }
            Err(e) => {
                any_broken = true;
                println!("UNKNOWN {path}  (could not analyze: {e:#})");
            }
        }
    }
    Ok(any_broken)
}

fn main() -> Result<()> {
    env_tracing_logger::init();
    let cli = Cli::parse();

    match &cli.cmd {
        Cmd::Check { inputs } => {
            let any_broken = cmd_check(inputs)?;
            if any_broken {
                std::process::exit(1);
            }
        }
        Cmd::Fix { input, force } => {
            cmd_fix(input, *force)?;
        }
    }

    Ok(())
}

/// One decoded H.264 sample kept for the `fix` path: its (untrustworthy) SEI
/// capture time, its bitstream POC, and the raw NAL units to re-emit.
struct FixFrame {
    pts_ns: i64,
    /// Only ordered within one coded video sequence; see [`FixFrame::display_key`].
    poc: i64,
    /// Coded-video-sequence index, bumped at each IDR because POC restarts there.
    cvs: i64,
    nals: Vec<Vec<u8>>,
}

impl FixFrame {
    /// Sort key putting samples in true display order across the whole file.
    fn display_key(&self) -> (i64, i64) {
        (self.cvs, self.poc)
    }
}

/// A whole file loaded for repair.
struct Loaded {
    frames: Vec<FixFrame>,
    width: u32,
    height: u32,
    /// Container-level SPS/PPS (MP4). `None` for Annex B, whose SPS/PPS ride
    /// inline in the samples and are re-emitted as-is.
    first_sps: Option<Vec<u8>>,
    first_pps: Option<Vec<u8>>,
    frame0_time: chrono::DateTime<chrono::FixedOffset>,
}

/// Open `path` (MP4 or raw Annex B `.h264`) and load every sample for repair.
fn load_file_for_fix(path: &Utf8PathBuf) -> Result<Loaded> {
    let builder = frame_source::FrameSourceBuilder::new(path)
        .do_decode_h264(false)
        .timestamp_source(frame_source::TimestampSource::MispMicrosectime);

    let ctx = || {
        format!(
            "opening \"{path}\" (this tool requires per-frame precision-timestamp \
            SEI data, as written by strand-cam / braid)"
        )
    };
    match path.extension().map(|e| e.to_lowercase()).as_deref() {
        Some("mp4") => load_for_fix(
            &mut builder.build_h264_in_mp4_source().with_context(ctx)?,
            path,
        ),
        Some("h264") => load_for_fix(
            &mut builder.build_h264_annexb_source().with_context(ctx)?,
            path,
        ),
        _ => bail!("\"{path}\": unsupported extension (expected .mp4 or .h264)"),
    }
}

fn load_for_fix<H: SeekableH264Source>(
    src: &mut H264Source<H>,
    path: &Utf8PathBuf,
) -> Result<Loaded> {
    let width = src.width();
    let height = src.height();
    let first_sps = src.as_seekable_h264_source().first_sps();
    let first_pps = src.as_seekable_h264_source().first_pps();
    let frame0_time = src
        .frame0_time()
        .ok_or_else(|| eyre!("\"{path}\": source has no frame0 time"))?;

    let mut reader = PocReader::new();
    reader.seed_from_container(src)?;

    let mut frames = Vec::new();
    let mut cvs = 0i64;
    for frame in src.decode_order_iter() {
        let frame = frame?;
        let pts_ns = frame.timestamp().unwrap_duration().as_nanos() as i64;
        let nals = frame_nals(frame)?;
        let poc = reader.poc_for_frame(&nals)?;
        if starts_coded_video_sequence(&nals) && !frames.is_empty() {
            cvs += 1;
        }
        frames.push(FixFrame {
            pts_ns,
            poc,
            cvs,
            nals,
        });
    }

    Ok(Loaded {
        frames,
        width,
        height,
        first_sps,
        first_pps,
        frame0_time,
    })
}

/// The 16-byte UUID marking a MISB ST 0604 precision-timestamp SEI, as written
/// by strand-cam / braid (and by `mp4-writer`).
const MISP_MARKER: &[u8] = b"MISPmicrosectime";

/// Is `nal_bytes` a precision-timestamp SEI NAL? The MISB marker is plain ASCII
/// with no `00 00` runs, so emulation-prevention never splits it and a raw byte
/// search of the NAL is reliable.
fn is_precision_timestamp_sei(nal_bytes: &[u8]) -> bool {
    let nal = RefNal::new(nal_bytes, &[], true);
    matches!(nal.header(), Ok(h) if h.nal_unit_type() == UnitType::SEI)
        && nal_bytes
            .windows(MISP_MARKER.len())
            .any(|w| w == MISP_MARKER)
}

/// The repaired per-sample timing for one decode-order frame.
struct RepairedTiming {
    /// Synthetic decode duration (stts).
    decode_duration: std::time::Duration,
    /// Composition offset (ctts) placing the sample at its corrected
    /// presentation time.
    composition_offset: chrono::Duration,
    /// Corrected capture time (relative to frame0) to write into the SEI.
    corrected_pts_ns: i64,
}

/// Compute corrected timing for every sample.
///
/// The bitstream's picture order count (POC) is the one trustworthy signal for
/// *display order*. We assume the multiset of SEI capture times in the file is
/// correct but was permuted onto the wrong frames (the mistagging `check`
/// detects), and that the camera captured frames in display order. Reassigning
/// the sorted capture times onto frames by POC rank therefore restores each
/// frame's true capture time. We then keep the samples in their existing decode
/// order (so the bitstream stays valid) and lay down a nominal, evenly spaced
/// decode timeline with composition offsets (ctts) placing each sample at its
/// corrected capture time -- making the container order and the SEI agree.
///
/// # Presentation times are the raw capture times
///
/// This writes each frame's *exact* recorded capture time into the container,
/// which makes the output variable-framerate and carries over whatever noise the
/// capture timestamps hold. That is a deliberate choice for a repair tool and
/// differs from what `frame-source`'s `retime_container_from_srt` does when
/// *recording*, where the cadence is estimated and presentation times are snapped
/// to it -- there, the raw times are already preserved at full precision in the
/// SEI, so putting noise in the container as well buys nothing. Here we are
/// reconstructing a file whose timing is known to be wrong, the caller has asked
/// for the truth to be recovered, and inventing a cadence would discard evidence.
///
/// To snap to an estimated grid instead, the change would be: classify each
/// interval of `sorted_times` as `k = round(interval / rate)` frame slots, solve
/// `rate = span / total slots` iteratively from `span / (n - 1)` (a span-based
/// estimate, *not* the median of successive intervals -- differencing doubles the
/// noise and the median can be several percent out on a short recording), then
/// replace `corrected[i]` with the cumulative snapped time for that frame's
/// display rank. Everything below stays as it is. Genuine gaps survive because a
/// skipped exposure classifies as `k >= 2`.
///
/// # Why the lead-in shift is not optional
///
/// A sample cannot be presented before it has been decoded, so every composition
/// offset must be non-negative. Laying `dts` down in decode order while
/// presentation follows display order means a reordered stream needs presentation
/// pushed later by a constant: for `I P B B B` with decode timeline `0 d 2d 3d 4d`
/// and capture times `0 4d d 2d 3d`, the raw offsets would be `0 3d -d -d -d`, and
/// those three B-frames would claim to be shown a frame before they are decoded.
/// `shift` is the smallest constant that removes that, and without it the output
/// is not properly decodable -- which is what this function used to emit.
///
/// The shift moves the whole presentation timeline later, so the first frame no
/// longer sits at zero. `mp4-writer` hides that by emitting an edit list, exactly
/// as an encoder does. Note that the SEI still carries the unshifted capture time:
/// `corrected_pts_ns` is the real time the frame was captured, and only the
/// container's internal timeline is offset.
fn repair_timing(frames: &[FixFrame]) -> Vec<RepairedTiming> {
    let n = frames.len();
    assert!(n > 0);

    // Sorted capture times, reassigned to frames by their POC (display) rank.
    let mut sorted_times: Vec<i64> = frames.iter().map(|f| f.pts_ns).collect();
    sorted_times.sort_unstable();
    let mut poc_order: Vec<usize> = (0..n).collect();
    poc_order.sort_by_key(|&i| frames[i].display_key());
    let mut corrected = vec![0i64; n];
    for (display_rank, &decode_index) in poc_order.iter().enumerate() {
        corrected[decode_index] = sorted_times[display_rank];
    }

    // `n - 1` intervals span `n` samples. Dividing by `n` instead is the
    // off-by-one that made recordings play a factor `n/(n-1)` too fast.
    let avg_interval_ns: i64 = if n > 1 {
        ((sorted_times[n - 1] - sorted_times[0]) as f64 / (n - 1) as f64).round() as i64
    } else {
        // A single-frame file has no reordering to fix, but every sample needs
        // a nonzero duration.
        (1_000_000_000f64 / 30.0).round() as i64
    }
    .max(1);

    // Smallest constant keeping every composition offset non-negative.
    let shift_ns = (0..n)
        .map(|i| avg_interval_ns * i as i64 - corrected[i])
        .max()
        .unwrap_or(0)
        .max(0);

    (0..n)
        .map(|i| {
            let dts_ns = avg_interval_ns * i as i64;
            RepairedTiming {
                decode_duration: std::time::Duration::from_nanos(avg_interval_ns as u64),
                composition_offset: chrono::Duration::nanoseconds(corrected[i] + shift_ns - dts_ns),
                corrected_pts_ns: corrected[i],
            }
        })
        .collect()
}

fn write_repaired(loaded: &Loaded, fd: std::fs::File) -> Result<()> {
    let timing = repair_timing(&loaded.frames);

    let cfg = Mp4RecordingConfig {
        codec: Mp4Codec::H264RawStream,
        max_framerate: RecordingFrameRate::Unlimited,
        h264_metadata: None,
    };
    let mut new_mp4 = mp4_writer::Mp4Writer::new(fd, cfg, None)?;
    // MP4 sources carry SPS/PPS in the container; pass them through. Annex B
    // sources keep them inline in the samples, so leave them unset here.
    if loaded.first_sps.is_some() || loaded.first_pps.is_some() {
        new_mp4.set_first_sps_pps(loaded.first_sps.clone(), loaded.first_pps.clone());
    }

    let frame0_time_local: chrono::DateTime<chrono::Local> =
        loaded.frame0_time.with_timezone(&chrono::Local);

    for (frame, t) in loaded.frames.iter().zip(timing) {
        let sei_timestamp = frame0_time_local + chrono::Duration::nanoseconds(t.corrected_pts_ns);
        // Drop the file's existing (mistagged) precision-timestamp SEI so the
        // fresh, corrected one inserted below is the only one; otherwise a
        // reader would still pick up the stale timestamp.
        let nals: Vec<Vec<u8>> = frame
            .nals
            .iter()
            .filter(|n| !is_precision_timestamp_sei(n))
            .cloned()
            .collect();
        let data = frame_source::H264EncodingVariant::RawEbsp(nals);
        new_mp4.write_h264_buf_passthrough(
            &data,
            loaded.width,
            loaded.height,
            t.decode_duration,
            t.composition_offset,
            sei_timestamp,
            true,
        )?;
    }

    new_mp4.finish()?;
    Ok(())
}

fn cmd_fix(input: &Utf8PathBuf, force: bool) -> Result<()> {
    let loaded = load_file_for_fix(input)?;
    let analysis = analyze_fix(&loaded.frames);
    // The same checks `check` runs, so the two never disagree about whether a
    // file needs repair. A container that is wrong only in its rate or offset
    // has nothing to reorder, but rewriting it from the capture times fixes it.
    let problems = check_file(input)?.problems();

    if problems.is_empty() && !force {
        println!(
            "{input}: already OK, nothing to fix ({} samples). Pass --force to rewrite anyway.",
            analysis.num_frames
        );
        return Ok(());
    }

    // Write the repaired MP4 to a temporary file alongside the input first, so
    // the original is only moved aside once the repair has fully succeeded. If
    // it does not, the temporary file is deleted again.
    let (fd, repaired) = PendingFile::create(input)
        .with_context(|| format!("creating temporary output file beside \"{input}\""))?;
    write_repaired(&loaded, fd)
        .with_context(|| format!("writing repaired output for \"{input}\""))?;

    let backup_path = next_backup_path(input);
    std::fs::rename(input, &backup_path).with_context(|| {
        format!("moving original \"{input}\" aside to backup \"{backup_path}\"")
    })?;
    repaired
        .commit()
        .with_context(|| format!("moving repaired file into place at \"{input}\""))?;

    println!(
        "{input}: reassigned {} of {} samples' capture times to POC order and \
        rewrote the container timing from them (original saved as \"{backup_path}\")",
        analysis.num_inversions, analysis.num_frames
    );
    Ok(())
}

/// The first available backup path for `input`: `<input>.bak`, or
/// `<input>.bak.1`, `<input>.bak.2`, ... if earlier ones already exist.
fn next_backup_path(input: &Utf8PathBuf) -> Utf8PathBuf {
    let base: Utf8PathBuf = format!("{input}.bak").into();
    if !base.exists() {
        return base;
    }
    let mut n = 1u32;
    loop {
        let candidate: Utf8PathBuf = format!("{input}.bak.{n}").into();
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

/// Same inversion analysis as [`analyze`], over [`FixFrame`]s.
fn analyze_fix(frames: &[FixFrame]) -> Analysis {
    let loaded: Vec<LoadedFrame> = frames
        .iter()
        .map(|f| LoadedFrame {
            pts_ns: f.pts_ns,
            poc: f.poc,
            cvs: f.cvs,
        })
        .collect();
    analyze(&loaded)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sample in the first (or only) coded video sequence.
    fn frame(pts_ns: i64, poc: i64) -> LoadedFrame {
        LoadedFrame {
            pts_ns,
            poc,
            cvs: 0,
        }
    }

    /// A sample in coded video sequence `cvs`, for multi-GOP cases where POC has
    /// restarted.
    fn frame_in(cvs: i64, pts_ns: i64, poc: i64) -> LoadedFrame {
        LoadedFrame { pts_ns, poc, cvs }
    }

    #[test]
    fn analyze_flags_sei_inconsistent_with_poc() {
        // Bitstream POC says the true display order is 0,2,3,1 (by index),
        // but the SEI timestamps just increase in decode order regardless -
        // exactly the "mistagged at write time" corruption this tool is
        // built to catch.
        let frames = vec![frame(0, 0), frame(1, 3), frame(2, 1), frame(3, 2)];
        let analysis = analyze(&frames);
        assert!(analysis.is_broken());
        assert_eq!(analysis.num_frames, 4);
        assert!(analysis.num_inversions > 0);
    }

    #[test]
    fn analyze_accepts_correctly_ordered_samples() {
        let frames = vec![frame(0, 0), frame(1, 1), frame(2, 2), frame(3, 3)];
        let analysis = analyze(&frames);
        assert!(!analysis.is_broken());
    }

    /// Regression test: POC restarts at every IDR, so a file with more than one
    /// group of pictures must be ordered by `(coded video sequence, POC)`. A bare
    /// POC sort interleaves the GOPs and reports a perfectly good recording as
    /// broken -- which is what this tool used to do to any short-GOP file,
    /// including untouched ffmpeg output.
    #[test]
    fn analyze_accepts_multiple_gops_with_restarting_poc() {
        // Two GOPs of three frames at 10 ns spacing. POC restarts at 0 in the
        // second GOP, so sorted by POC alone the order would be
        // 0(cvs0), 0(cvs1), 2(cvs0), 2(cvs1), ... and the timestamps would look
        // wildly out of order.
        let frames = vec![
            frame_in(0, 0, 0),
            frame_in(0, 10, 2),
            frame_in(0, 20, 4),
            frame_in(1, 30, 0),
            frame_in(1, 40, 2),
            frame_in(1, 50, 4),
        ];
        let analysis = analyze(&frames);
        assert!(
            !analysis.is_broken(),
            "multi-GOP file wrongly reported broken: {} of {} samples inverted",
            analysis.num_inversions,
            analysis.num_frames
        );
    }

    /// The unit tests above pin the *ordering* rule, but they hand-build the
    /// coded-video-sequence index. This one drives the real loader over a real
    /// multi-GOP B-frame recording, so that IDR detection is covered too: if
    /// [`starts_coded_video_sequence`] ever stopped recognising an IDR, `cvs`
    /// would stay 0 everywhere and the false positive would come back while every
    /// other test still passed.
    ///
    /// `fix` writes the repaired movie under a temporary name and then swaps it
    /// in. Afterwards there must be the repaired movie, the backup of the
    /// original, and no temporary file.
    #[test]
    fn fix_leaves_the_movie_and_its_backup_only() {
        let dir = tempfile::tempdir().unwrap();
        let input = Utf8PathBuf::from_path_buf(dir.path().join("movie.mp4")).unwrap();

        // `fix` needs the capture times Strand Camera records, so record a few
        // frames the way it does.
        {
            let cfg = Mp4RecordingConfig {
                codec: Mp4Codec::H264LessAvc,
                max_framerate: RecordingFrameRate::Unlimited,
                h264_metadata: None,
            };
            let fd = std::fs::File::create(&input).unwrap();
            let mut wtr = mp4_writer::Mp4Writer::new(fd, cfg, None).unwrap();
            let frame =
                strand_dynamic_frame::DynamicFrameOwned::from_static(
                    machine_vision_formats::owned::OImage::<
                        machine_vision_formats::pixel_format::Mono8,
                    >::new(32, 16, 32, vec![0u8; 32 * 16])
                    .unwrap(),
                );
            let start = chrono::DateTime::from_timestamp(3600, 0).unwrap();
            for fno in 0..5 {
                let stamp = start + chrono::Duration::milliseconds(fno * 40);
                wtr.write_dynamic(&frame.borrow(), stamp).unwrap();
            }
            wtr.finish().unwrap();
        }
        let original = std::fs::read(&input).unwrap();

        cmd_fix(&input, true).unwrap();

        let mut names: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names.len(), 2, "{names:?}");
        assert!(names.contains(&"movie.mp4".to_string()), "{names:?}");
        assert!(!names.iter().any(|n| n.ends_with(".partial")), "{names:?}");
        // The backup is the original, and the movie in its place is readable.
        let backup = names.iter().find(|n| *n != "movie.mp4").unwrap();
        assert_eq!(std::fs::read(dir.path().join(backup)).unwrap(), original);
        assert_eq!(check_file(&input).unwrap().problems(), Vec::<String>::new());
    }

    /// Borrows `frame-source`'s committed fixture rather than duplicating a
    /// binary blob. The properties this test depends on are asserted rather than
    /// assumed, so if that fixture is ever regenerated differently the test says
    /// so instead of quietly passing on a file that no longer exercises anything.
    #[test]
    fn real_multi_gop_bframe_recording_is_not_flagged() {
        let path = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../frame-source/tests/data/bframes.mp4");
        assert!(path.exists(), "missing fixture {path}");

        let frames = match load(&path, frame_source::TimestampSource::Mp4Pts) {
            Ok(frames) => frames,
            Err(LoadError::NoTimestamps) => panic!("{path}: fixture has no container timing"),
            Err(LoadError::Other(e)) => panic!("{path}: {e:#}"),
        };

        let max_cvs = frames.iter().map(|f| f.cvs).max().unwrap();
        assert!(
            max_cvs >= 1,
            "fixture must span more than one group of pictures to exercise the \
             POC-restart case, but every sample landed in sequence 0"
        );
        assert!(
            frames.array_windows().any(|[a, b]| b.poc < a.poc),
            "fixture must be reordered (POC not monotonic in decode order), \
             otherwise this proves nothing about B-frame handling"
        );

        let analysis = analyze(&frames);
        assert!(
            !analysis.is_broken(),
            "a valid {}-GOP recording was reported broken: {} of {} samples \
             inverted, up to {:.1}ms early",
            max_cvs + 1,
            analysis.num_inversions,
            analysis.num_frames,
            analysis.max_inversion_ms
        );
    }

    /// Timing series for a container and an SEI at the given nanosecond times.
    fn series(times: &[i64]) -> Vec<LoadedFrame> {
        times
            .iter()
            .enumerate()
            .map(|(i, &t)| frame(t, i as i64))
            .collect()
    }

    /// A container running at exactly the capture rate must not be flagged. The
    /// small residue here is timescale rounding, which is what the half-frame
    /// threshold is meant to tolerate.
    #[test]
    fn cadence_accepts_a_container_at_the_capture_rate() {
        let sei: Vec<i64> = (0..20).map(|i| i * 40_000_000).collect();
        let container: Vec<i64> = sei.iter().map(|t| t + 11_000).collect();
        let check = cadence_check(&series(&container), &series(&sei)).unwrap();
        assert!(!check.is_broken(), "{}", check.describe());
    }

    /// The `n/(n-1)` bug: the container spans `(n-1)/n` of the capture time, so
    /// playback is that factor too fast. `analyze` sees nothing wrong -- the
    /// timeline is still monotonic -- which is exactly why this check exists.
    #[test]
    fn cadence_flags_a_uniformly_compressed_timeline() {
        let n = 20i64;
        let sei: Vec<i64> = (0..n).map(|i| i * 40_000_000).collect();
        // Every duration scaled by (n-1)/n, as the old scaling did.
        let container: Vec<i64> = (0..n).map(|i| i * 40_000_000 * (n - 1) / n).collect();

        // The ordering check is blind to it.
        assert!(!analyze(&series(&container)).is_broken());

        let check = cadence_check(&series(&container), &series(&sei)).unwrap();
        assert!(check.is_broken(), "{}", check.describe());
        // The drift is `span / n`: one *compressed* interval, which is
        // `(n-1)/n` of a mean interval -- 0.95 frames here, tending to a whole
        // frame as the recording lengthens. Never far from one frame at any
        // length, which is what makes half a frame a length-independent
        // threshold.
        let expected_drift = check.sei_span_ns / n;
        assert!(
            (check.drift_ns() - expected_drift).abs() <= expected_drift / 100,
            "expected a drift of span/n = {expected_drift}ns, got {}",
            check.describe()
        );
        assert!(check.drift_ns() * 100 > check.mean_interval_ns * 90);
    }

    /// The same bug on a long recording. The *percentage* error shrinks with
    /// length (0.1% at n=1000) but the absolute drift stays at one frame, so a
    /// relative threshold would have missed this and the half-frame one does not.
    #[test]
    fn cadence_flags_compression_on_a_long_recording() {
        let n = 1000i64;
        let sei: Vec<i64> = (0..n).map(|i| i * 40_000_000).collect();
        let container: Vec<i64> = (0..n).map(|i| i * 40_000_000 * (n - 1) / n).collect();
        let check = cadence_check(&series(&container), &series(&sei)).unwrap();
        assert!(
            check.rate_error().abs() < 0.002,
            "rate error should be tiny"
        );
        assert!(check.is_broken(), "{}", check.describe());
    }

    /// A writer may deliberately snap presentation times to an estimated cadence,
    /// discarding capture-timestamp jitter that the acquisition clock never had.
    /// That moves individual frames but preserves the total, so it must not be
    /// reported as a rate fault -- which is why this compares spans and not
    /// frames.
    #[test]
    fn cadence_accepts_a_container_snapped_to_a_clean_grid() {
        const JITTER: [i64; 10] = [
            0, 1_300_000, -900_000, 2_000_000, -1_700_000, 600_000, -400_000, 1_100_000,
            -2_000_000, 300_000,
        ];
        let n = 20usize;
        let sei: Vec<i64> = Vec::from_fn(n, |i| i as i64 * 40_000_000 + JITTER[i % JITTER.len()]);
        // Snapped to the span-derived rate, so the ends line up and the middle
        // does not.
        let rate = (sei[n - 1] - sei[0]) / (n as i64 - 1);
        let container: Vec<i64> = Vec::from_fn(n, |i| i as i64 * rate);

        let check = cadence_check(&series(&container), &series(&sei)).unwrap();
        assert!(!check.is_broken(), "{}", check.describe());
    }

    /// The old `mp4-writer` signature: the first sample has zero duration and
    /// every frame is presented at the capture time of the frame before it. The
    /// order is still right, so [`analyze`] does not notice. Irregular
    /// intervals, as from a webcam.
    #[test]
    fn offset_flags_a_container_lagging_by_one_frame() {
        const INTERVALS: [i64; 6] = [68, 64, 32, 61, 60, 66];
        let mut sei = vec![0i64];
        for i in 0..40 {
            sei.push(sei[i] + INTERVALS[i % INTERVALS.len()] * 1_000_000);
        }
        // Frame r presented at frame r-1's capture time; frames 0 and 1 collide.
        let container: Vec<i64> = (0..sei.len()).map(|r| sei[r.saturating_sub(1)]).collect();

        assert!(!analyze(&series(&container)).is_broken());

        let check = offset_check(&series(&container), &series(&sei)).unwrap();
        assert!(check.is_broken(), "{}", check.describe());
        assert!(check.median_offset_ns < 0, "{}", check.describe());
    }

    /// The same lag at a perfectly constant rate, as from a triggered camera.
    #[test]
    fn offset_flags_a_lag_at_a_constant_rate() {
        let sei: Vec<i64> = (0..100).map(|i| i * 10_000_000).collect();
        let container: Vec<i64> = (0..sei.len()).map(|r| sei[r.saturating_sub(1)]).collect();
        let check = offset_check(&series(&container), &series(&sei)).unwrap();
        assert!(check.is_broken(), "{}", check.describe());
    }

    /// Snapping to an estimated cadence moves individual frames by their jitter;
    /// the median stays near zero.
    #[test]
    fn offset_accepts_a_container_snapped_to_a_clean_grid() {
        const JITTER: [i64; 10] = [
            0, 1_300_000, -900_000, 2_000_000, -1_700_000, 600_000, -400_000, 1_100_000,
            -2_000_000, 300_000,
        ];
        let n = 20usize;
        let sei: Vec<i64> = Vec::from_fn(n, |i| i as i64 * 40_000_000 + JITTER[i % JITTER.len()]);
        let rate = (sei[n - 1] - sei[0]) / (n as i64 - 1);
        let container: Vec<i64> = Vec::from_fn(n, |i| i as i64 * rate);
        let check = offset_check(&series(&container), &series(&sei)).unwrap();
        assert!(!check.is_broken(), "{}", check.describe());
    }

    /// A reordered stream read without its edit list starts at its lead-in
    /// rather than at zero. That constant must not count as an offset.
    #[test]
    fn offset_ignores_a_constant_lead_in() {
        let sei: Vec<i64> = (0..20).map(|i| i * 50_000_000).collect();
        let container: Vec<i64> = sei.iter().map(|t| t + 100_000_000).collect();
        let check = offset_check(&series(&container), &series(&sei)).unwrap();
        assert!(!check.is_broken(), "{}", check.describe());
        assert_eq!(check.median_offset_ns, 0);
    }

    /// The multi-GOP fix must not blind the check to real corruption *within* a
    /// group of pictures.
    #[test]
    fn analyze_still_flags_inversion_inside_one_gop() {
        let frames = vec![
            frame_in(0, 0, 0),
            frame_in(0, 10, 2),
            // Second GOP, but its first frame claims a time before the previous
            // GOP's last -- genuinely out of order.
            frame_in(1, 5, 0),
            frame_in(1, 40, 2),
        ];
        assert!(analyze(&frames).is_broken());
    }

    /// `repair` reassigns capture times by display rank, so it needs the same
    /// per-sequence POC keying: keyed by POC alone it would shuffle times between
    /// groups of pictures and corrupt a multi-GOP file it was asked to fix.
    #[test]
    fn repair_keeps_capture_times_within_their_gop() {
        let frames = vec![
            fix_frame_in(0, 0, 0),
            fix_frame_in(0, 10, 4),
            fix_frame_in(0, 20, 2),
            fix_frame_in(1, 30, 0),
            fix_frame_in(1, 40, 4),
            fix_frame_in(1, 50, 2),
        ];
        let timing = repair_timing(&frames);
        let corrected: Vec<i64> = timing.iter().map(|t| t.corrected_pts_ns).collect();
        // Within each GOP the two later frames swap (POC 4 displays after POC 2),
        // but no time crosses the GOP boundary.
        assert_eq!(corrected, vec![0, 20, 10, 30, 50, 40]);
    }

    #[test]
    fn analyze_accepts_reordered_decode_order_with_matching_sei() {
        // Decode order I,P,B,B with POC 0,3,1,2 (P displays last of the
        // first four, the two B's fall in between) and SEI timestamps that
        // correctly track that same display order.
        let frames = vec![
            frame(0, 0),  // I
            frame(30, 3), // P
            frame(10, 1), // B
            frame(20, 2), // B
        ];
        let analysis = analyze(&frames);
        assert!(!analysis.is_broken());
    }

    fn fix_frame(pts_ns: i64, poc: i64) -> FixFrame {
        FixFrame {
            pts_ns,
            poc,
            cvs: 0,
            nals: vec![],
        }
    }

    fn fix_frame_in(cvs: i64, pts_ns: i64, poc: i64) -> FixFrame {
        FixFrame {
            pts_ns,
            poc,
            cvs,
            nals: vec![],
        }
    }

    #[test]
    fn repair_reassigns_capture_times_to_poc_order() {
        // Decode order with SEI capture times that climb in decode order but
        // whose POC (true display order) is jumbled - the mistagging the tool
        // detects. Display order by POC is decode indices [0, 2, 3, 1].
        let frames = vec![
            fix_frame(0, 0),
            fix_frame(10, 3),
            fix_frame(20, 1),
            fix_frame(30, 2),
        ];
        let timing = repair_timing(&frames);

        // The sorted capture times get reassigned onto frames by POC rank, so
        // in decode order the corrected times are [0, 30, 10, 20].
        let corrected: Vec<i64> = timing.iter().map(|t| t.corrected_pts_ns).collect();
        assert_eq!(corrected, vec![0, 30, 10, 20]);

        // Presentation time (decode time + composition offset) must track the
        // corrected capture time and strictly increase in POC display order.
        //
        // Tracks it up to a single constant lead-in, not exactly: presentation is
        // pushed later by the smallest amount that keeps every composition offset
        // non-negative, because a sample cannot be shown before it is decoded.
        // The same constant for every frame, so relative timing is untouched, and
        // `mp4-writer` emits an edit list to hide it.
        let interval = timing[0].decode_duration.as_nanos() as i64;
        let mut by_poc: Vec<usize> = (0..frames.len()).collect();
        by_poc.sort_by_key(|&i| frames[i].display_key());

        let presentation_of = |i: usize| {
            interval * i as i64 + timing[i].composition_offset.num_nanoseconds().unwrap()
        };
        let shift = presentation_of(0) - timing[0].corrected_pts_ns;
        assert!(shift >= 0, "lead-in must not be negative");

        let mut prev = i64::MIN;
        for &i in &by_poc {
            let presentation = presentation_of(i);
            assert_eq!(
                presentation - shift,
                timing[i].corrected_pts_ns,
                "sample {i} is not offset by the same constant as the rest"
            );
            assert!(presentation > prev, "presentation must increase by POC");
            prev = presentation;
        }

        // Every composition offset non-negative: the output has to be decodable,
        // and a negative offset would claim a frame is shown before it is decoded.
        for (i, t) in timing.iter().enumerate() {
            assert!(
                t.composition_offset.num_nanoseconds().unwrap() >= 0,
                "sample {i} has a negative composition offset, so it would be \
                 presented before it is decoded"
            );
        }

        // The repaired stream now passes the tool's own consistency check.
        let repaired: Vec<LoadedFrame> = frames
            .iter()
            .zip(&timing)
            .map(|(f, t)| frame(t.corrected_pts_ns, f.poc))
            .collect();
        assert!(!analyze(&repaired).is_broken());
    }
}
