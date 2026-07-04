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
use frame_source::{
    FrameDataSource,
    h264_source::{H264Source, SeekableH264Source},
};
use h264_reader::{
    Context as H264ParsingContext,
    nal::{
        Nal, RefNal, UnitType,
        pps::PicParameterSet,
        slice::{PicOrderCountLsb, SliceHeader},
        sps::{PicOrderCntType, SeqParameterSet},
    },
};
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
    /// within one file; not a real time value.
    poc: i64,
}

/// Reconstructs picture order count (POC) for `pic_order_cnt_type == 0`
/// streams (ITU-T H.264 §8.2.1.1), which covers essentially all cameras and
/// software/hardware H.264 encoders in practice.
struct PocDecoder {
    max_poc_lsb: i64,
    prev_poc_msb: i64,
    prev_poc_lsb: i64,
}

impl PocDecoder {
    fn new(log2_max_pic_order_cnt_lsb_minus4: u8) -> Self {
        Self {
            max_poc_lsb: 1i64 << (log2_max_pic_order_cnt_lsb_minus4 as i64 + 4),
            prev_poc_msb: 0,
            prev_poc_lsb: 0,
        }
    }

    /// Feed the next sample's slice header info, in decode order, and get
    /// back its POC.
    fn next_poc(&mut self, is_idr: bool, nal_ref_idc: u8, poc_lsb: i64) -> i64 {
        if is_idr {
            self.prev_poc_msb = 0;
            self.prev_poc_lsb = 0;
        }

        let half_max = self.max_poc_lsb / 2;
        let poc_msb = if poc_lsb < self.prev_poc_lsb && (self.prev_poc_lsb - poc_lsb) >= half_max {
            self.prev_poc_msb + self.max_poc_lsb
        } else if poc_lsb > self.prev_poc_lsb && (poc_lsb - self.prev_poc_lsb) > half_max {
            self.prev_poc_msb - self.max_poc_lsb
        } else {
            self.prev_poc_msb
        };

        let poc = poc_msb + poc_lsb;

        // Only reference pictures participate in the prevPicOrderCnt chain.
        if nal_ref_idc != 0 {
            self.prev_poc_msb = poc_msb;
            self.prev_poc_lsb = poc_lsb;
        }

        poc
    }
}

/// How each frame's picture order count is obtained, selected from the SPS's
/// `pic_order_cnt_type`.
enum PocStrategy {
    /// `pic_order_cnt_type == 0`: read `pic_order_cnt_lsb` from every slice and
    /// unwrap it (this is the type that can carry B-frame reordering).
    FromSliceLsb(PocDecoder),
    /// `pic_order_cnt_type == 2`: the bitstream guarantees decode order equals
    /// display order (ITU-T H.264 §8.2.1.3 — no reordering is possible), so the
    /// POC simply follows decode order.
    DecodeOrder { next: i64 },
}

/// Parse an SPS NAL and determine the POC strategy from its
/// `pic_order_cnt_type`. Types 0 and 2 are supported; type 1 (rare, delta-based)
/// is not.
fn parse_sps(nal: &RefNal<'_>, path: &Utf8PathBuf) -> Result<(SeqParameterSet, PocStrategy)> {
    let sps = SeqParameterSet::from_bits(nal.rbsp_bits()).map_err(|e| eyre!("bad SPS: {e:?}"))?;
    let strategy = match sps.pic_order_cnt {
        PicOrderCntType::TypeZero {
            log2_max_pic_order_cnt_lsb_minus4,
        } => PocStrategy::FromSliceLsb(PocDecoder::new(log2_max_pic_order_cnt_lsb_minus4)),
        PicOrderCntType::TypeTwo => PocStrategy::DecodeOrder { next: 0 },
        PicOrderCntType::TypeOne { .. } => {
            bail!("\"{path}\" uses pic_order_cnt_type 1, which this tool doesn't support yet")
        }
    };
    Ok((sps, strategy))
}

/// Extract (is_idr, nal_ref_idc, pic_order_cnt_lsb) from the first slice NAL
/// unit in a decoded sample.
fn read_slice_poc_lsb(ctx: &H264ParsingContext, nals: &[Vec<u8>]) -> Result<(bool, u8, i64)> {
    for nal_bytes in nals {
        let nal = RefNal::new(nal_bytes, &[], true);
        let header = nal.header().map_err(|e| eyre!("bad NAL header: {e:?}"))?;
        let unit_type = header.nal_unit_type();
        if !matches!(
            unit_type,
            UnitType::SliceLayerWithoutPartitioningIdr
                | UnitType::SliceLayerWithoutPartitioningNonIdr
        ) {
            continue;
        }
        let is_idr = unit_type == UnitType::SliceLayerWithoutPartitioningIdr;
        let mut r = nal.rbsp_bits();
        let (slice_header, _sps, _pps) = SliceHeader::from_bits(ctx, &mut r, header)
            .map_err(|e| eyre!("bad slice header: {e:?}"))?;
        let poc_lsb = match slice_header.pic_order_cnt_lsb {
            Some(PicOrderCountLsb::Frame(lsb)) => lsb as i64,
            Some(_) => bail!("field pictures are not supported"),
            None => bail!("slice has no pic_order_cnt_lsb (unsupported pic_order_cnt_type)"),
        };
        return Ok((is_idr, header.nal_ref_idc(), poc_lsb));
    }
    bail!("sample has no slice NAL unit")
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
            load_frames(&mut src, path).map_err(LoadError::Other)
        }
        Some("h264") => {
            let mut src = builder.build_h264_annexb_source().map_err(build_err)?;
            load_frames(&mut src, path).map_err(LoadError::Other)
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

/// Run every applicable timing-vs-POC check for `path`.
///
/// An MP4 always carries container timing (the `stts`/`ctts` boxes, which is
/// what players use to order frames), so that is checked for every MP4 --
/// including plain ffmpeg recordings with no SEI. The per-frame
/// precision-timestamp SEI (written by strand-cam / braid) is checked whenever
/// it is present; for a raw Annex B `.h264` file it is the only signal.
fn check_file(path: &Utf8PathBuf) -> Result<Vec<SourceCheck>> {
    let is_mp4 = path.extension().map(|e| e.to_lowercase()).as_deref() == Some("mp4");
    let mut checks = Vec::new();

    if is_mp4 {
        let frames = load(path, frame_source::TimestampSource::Mp4Pts).map_err(|e| match e {
            LoadError::NoTimestamps => eyre!("\"{path}\": MP4 has no container sample timing"),
            LoadError::Other(r) => r,
        })?;
        checks.push(SourceCheck {
            label: "container (stts/ctts)",
            analysis: analyze(&frames),
        });
    }

    match load(path, frame_source::TimestampSource::MispMicrosectime) {
        Ok(frames) => checks.push(SourceCheck {
            label: "precision-timestamp SEI",
            analysis: analyze(&frames),
        }),
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

    Ok(checks)
}

/// Accumulates the H.264 parsing context (SPS/PPS) and the POC decoder as
/// samples are read, so both [`check`](cmd_check) and [`fix`](cmd_fix) can
/// reconstruct each frame's picture order count the same way.
struct PocReader {
    ctx: H264ParsingContext,
    // The POC strategy comes from the SPS's `pic_order_cnt_type`, so it can only
    // be chosen once an SPS has been seen. MP4 keeps SPS/PPS in the container;
    // Annex B streams carry them inline (picked up per-frame).
    strategy: Option<PocStrategy>,
}

impl PocReader {
    fn new() -> Self {
        Self {
            ctx: H264ParsingContext::default(),
            strategy: None,
        }
    }

    /// Record an SPS: feed it to the parsing context and, on the first one, fix
    /// the POC strategy from its `pic_order_cnt_type`.
    fn put_sps(&mut self, nal: &RefNal<'_>, path: &Utf8PathBuf) -> Result<()> {
        let (sps, strategy) = parse_sps(nal, path)?;
        if self.strategy.is_none() {
            self.strategy = Some(strategy);
        }
        self.ctx.put_seq_param_set(sps);
        Ok(())
    }

    /// Seed SPS/PPS from container-level metadata (MP4). No-op for Annex B,
    /// which carries them inline (handled in [`Self::poc_for_frame`]).
    fn seed_from_container<H: SeekableH264Source>(
        &mut self,
        src: &H264Source<H>,
        path: &Utf8PathBuf,
    ) -> Result<()> {
        if let Some(sps_bytes) = src.as_seekable_h264_source().first_sps() {
            let nal = RefNal::new(&sps_bytes, &[], true);
            self.put_sps(&nal, path)?;
        }
        if let Some(pps_bytes) = src.as_seekable_h264_source().first_pps() {
            let nal = RefNal::new(&pps_bytes, &[], true);
            let pps = PicParameterSet::from_bits(&self.ctx, nal.rbsp_bits())
                .map_err(|e| eyre!("bad PPS: {e:?}"))?;
            self.ctx.put_pic_param_set(pps);
        }
        Ok(())
    }

    /// Feed any inline SPS/PPS (Annex B) carried with this frame, then return
    /// the frame's POC.
    fn poc_for_frame(&mut self, nals: &[Vec<u8>], path: &Utf8PathBuf) -> Result<i64> {
        for nal_bytes in nals {
            let nal = RefNal::new(nal_bytes, &[], true);
            let Ok(header) = nal.header() else { continue };
            match header.nal_unit_type() {
                UnitType::SeqParameterSet => self.put_sps(&nal, path)?,
                UnitType::PicParameterSet => {
                    let pps = PicParameterSet::from_bits(&self.ctx, nal.rbsp_bits())
                        .map_err(|e| eyre!("bad PPS: {e:?}"))?;
                    self.ctx.put_pic_param_set(pps);
                }
                _ => {}
            }
        }
        // `self.ctx` and `self.strategy` are disjoint fields, so the immutable
        // borrow of the context and the mutable borrow of the decoder coexist.
        match &mut self.strategy {
            None => bail!("\"{path}\": slice data appeared before any SPS"),
            Some(PocStrategy::FromSliceLsb(decoder)) => {
                let (is_idr, nal_ref_idc, poc_lsb) = read_slice_poc_lsb(&self.ctx, nals)?;
                Ok(decoder.next_poc(is_idr, nal_ref_idc, poc_lsb))
            }
            Some(PocStrategy::DecodeOrder { next }) => {
                let poc = *next;
                *next += 1;
                Ok(poc)
            }
        }
    }
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

/// Reconstruct picture order count (POC) and read the SEI capture time for
/// every sample of an already-opened H.264 source.
fn load_frames<H: SeekableH264Source>(
    src: &mut H264Source<H>,
    path: &Utf8PathBuf,
) -> Result<Vec<LoadedFrame>> {
    let mut reader = PocReader::new();
    reader.seed_from_container(src, path)?;

    let mut frames = Vec::new();
    for frame in src.iter() {
        let frame = frame?;
        let pts_ns = frame.timestamp().unwrap_duration().as_nanos() as i64;
        let nals = frame_nals(frame)?;
        let poc = reader.poc_for_frame(&nals, path)?;
        frames.push(LoadedFrame { pts_ns, poc });
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

/// Compare the bitstream's true picture order (POC) against a timing series:
/// sort samples by POC and check whether the timestamps come out non-decreasing.
fn analyze(frames: &[LoadedFrame]) -> Analysis {
    let mut order: Vec<usize> = (0..frames.len()).collect();
    order.sort_by_key(|&i| frames[i].poc);

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
            Ok(checks) => {
                let broken: Vec<&SourceCheck> =
                    checks.iter().filter(|c| c.analysis.is_broken()).collect();
                if !broken.is_empty() {
                    any_broken = true;
                    let details = broken
                        .iter()
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
                        .collect::<Vec<_>>()
                        .join("; ");
                    println!("BROKEN  {path}  ({details})");
                } else {
                    let num_frames = checks.first().map(|c| c.analysis.num_frames).unwrap_or(0);
                    let checked = checks
                        .iter()
                        .map(|c| c.label)
                        .collect::<Vec<_>>()
                        .join(", ");
                    println!("OK      {path}  ({num_frames} samples; checked: {checked})");
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
    poc: i64,
    nals: Vec<Vec<u8>>,
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
    reader.seed_from_container(src, path)?;

    let mut frames = Vec::new();
    for frame in src.iter() {
        let frame = frame?;
        let pts_ns = frame.timestamp().unwrap_duration().as_nanos() as i64;
        let nals = frame_nals(frame)?;
        let poc = reader.poc_for_frame(&nals, path)?;
        frames.push(FixFrame { pts_ns, poc, nals });
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
/// decode timeline with composition offsets (ctts) so that
/// `decode_time + composition_offset == corrected capture time` for every
/// sample -- making the container order and the SEI agree.
fn repair_timing(frames: &[FixFrame]) -> Vec<RepairedTiming> {
    let n = frames.len();
    assert!(n > 0);

    // Sorted capture times, reassigned to frames by their POC (display) rank.
    let mut sorted_times: Vec<i64> = frames.iter().map(|f| f.pts_ns).collect();
    sorted_times.sort_unstable();
    let mut poc_order: Vec<usize> = (0..n).collect();
    poc_order.sort_by_key(|&i| frames[i].poc);
    let mut corrected = vec![0i64; n];
    for (display_rank, &decode_index) in poc_order.iter().enumerate() {
        corrected[decode_index] = sorted_times[display_rank];
    }

    let avg_interval_ns: i64 = if n > 1 {
        ((sorted_times[n - 1] - sorted_times[0]) as f64 / (n - 1) as f64).round() as i64
    } else {
        // A single-frame file has no reordering to fix, but every sample needs
        // a nonzero duration.
        (1_000_000_000f64 / 30.0).round() as i64
    }
    .max(1);

    (0..n)
        .map(|i| {
            let dts_ns = avg_interval_ns * i as i64;
            RepairedTiming {
                decode_duration: std::time::Duration::from_nanos(avg_interval_ns as u64),
                composition_offset: chrono::Duration::nanoseconds(corrected[i] - dts_ns),
                corrected_pts_ns: corrected[i],
            }
        })
        .collect()
}

fn write_repaired(loaded: &Loaded, out_path: &Utf8PathBuf) -> Result<()> {
    let timing = repair_timing(&loaded.frames);

    let fd = std::fs::File::create(out_path)
        .with_context(|| format!("creating output file \"{out_path}\""))?;
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

    if !analysis.is_broken() && !force {
        println!(
            "{input}: already OK, nothing to fix ({} samples). Pass --force to rewrite anyway.",
            analysis.num_frames
        );
        return Ok(());
    }

    // Write the repaired MP4 to a temporary file alongside the input first, so
    // the original is only moved aside once the repair has fully succeeded.
    let tmp_path: Utf8PathBuf = format!("{input}.mp4-bframe-doctor.tmp").into();
    write_repaired(&loaded, &tmp_path)
        .with_context(|| format!("writing repaired output for \"{input}\""))?;

    let backup_path = next_backup_path(input);
    std::fs::rename(input, &backup_path).with_context(|| {
        format!("moving original \"{input}\" aside to backup \"{backup_path}\"")
    })?;
    std::fs::rename(&tmp_path, input)
        .with_context(|| format!("moving repaired file into place at \"{input}\""))?;

    println!(
        "{input}: reassigned {} of {} samples' capture times to POC order \
        (original saved as \"{backup_path}\")",
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
        })
        .collect();
    analyze(&loaded)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(pts_ns: i64, poc: i64) -> LoadedFrame {
        LoadedFrame { pts_ns, poc }
    }

    #[test]
    fn parse_sps_selects_strategy_by_pic_order_cnt_type() {
        // Real SPS NAL units (EBSP, including the NAL header byte) captured from
        // sample recordings. `pic_order_cnt_type == 0` (explicit poc_lsb, can
        // carry B-frame reordering) vs `== 2` (decode order == display order).
        const SPS_TYPE0: &[u8] = &[
            0x67, 0xf4, 0x00, 0x28, 0x91, 0x9b, 0x28, 0x0f, 0x00, 0x44, 0xfc, 0x4c, 0xd9, 0x00,
            0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x03, 0x00, 0x32, 0x0f, 0x18, 0x31, 0x96,
        ];
        const SPS_TYPE2: &[u8] = &[
            0x67, 0x64, 0x44, 0x28, 0xac, 0x4d, 0x00, 0xf0, 0x04, 0x4f, 0xcb, 0x34, 0xb7, 0x00,
            0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x03, 0x00, 0x3c, 0x0f, 0x08, 0x84, 0x6a,
        ];
        let path = Utf8PathBuf::from("test.mp4");
        let (_, s0) = parse_sps(&RefNal::new(SPS_TYPE0, &[], true), &path).unwrap();
        assert!(
            matches!(s0, PocStrategy::FromSliceLsb(_)),
            "pic_order_cnt_type 0 should read poc_lsb from slices"
        );
        let (_, s2) = parse_sps(&RefNal::new(SPS_TYPE2, &[], true), &path).unwrap();
        assert!(
            matches!(s2, PocStrategy::DecodeOrder { .. }),
            "pic_order_cnt_type 2 should fall back to decode order"
        );
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

    #[test]
    fn poc_decoder_handles_simple_ipbb_gop() {
        let mut dec = PocDecoder::new(4); // MaxPicOrderCntLsb = 256
        // I(ref), P(ref), B(non-ref), B(non-ref), repeating POC pattern
        // typical of an IBBP-style GOP with POC step 2 per displayed frame.
        assert_eq!(dec.next_poc(true, 1, 0), 0); // I, poc 0
        assert_eq!(dec.next_poc(false, 1, 6), 6); // P, poc 6
        assert_eq!(dec.next_poc(false, 0, 2), 2); // B, poc 2
        assert_eq!(dec.next_poc(false, 0, 4), 4); // B, poc 4
    }

    #[test]
    fn poc_decoder_unwraps_lsb_wraparound() {
        let mut dec = PocDecoder::new(0); // MaxPicOrderCntLsb = 16
        // Step by 2 each reference frame, staying well under
        // MaxPicOrderCntLsb/2 (8) per step so no wrap is triggered yet.
        assert_eq!(dec.next_poc(true, 1, 0), 0);
        assert_eq!(dec.next_poc(false, 1, 2), 2);
        assert_eq!(dec.next_poc(false, 1, 4), 4);
        assert_eq!(dec.next_poc(false, 1, 6), 6);
        assert_eq!(dec.next_poc(false, 1, 8), 8);
        assert_eq!(dec.next_poc(false, 1, 10), 10);
        assert_eq!(dec.next_poc(false, 1, 12), 12);
        assert_eq!(dec.next_poc(false, 1, 14), 14);
        // lsb wraps from 14 back down to 0. The raw backward delta (14)
        // meets MaxPicOrderCntLsb/2, so this is really a forward step to
        // poc 16, not a jump back near zero.
        assert_eq!(dec.next_poc(false, 1, 0), 16);
    }

    fn fix_frame(pts_ns: i64, poc: i64) -> FixFrame {
        FixFrame {
            pts_ns,
            poc,
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

        // Presentation time (decode time + composition offset) must equal the
        // corrected capture time and strictly increase in POC display order.
        let interval = timing[0].decode_duration.as_nanos() as i64;
        let mut by_poc: Vec<usize> = (0..frames.len()).collect();
        by_poc.sort_by_key(|&i| frames[i].poc);
        let mut prev = i64::MIN;
        for &i in &by_poc {
            let dts = interval * i as i64;
            let presentation = dts + timing[i].composition_offset.num_nanoseconds().unwrap();
            assert_eq!(presentation, timing[i].corrected_pts_ns);
            assert!(presentation > prev, "presentation must increase by POC");
            prev = presentation;
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
