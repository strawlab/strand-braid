// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Detect MP4 or raw Annex B `.h264` files whose per-frame
//! precision-timestamp SEI data is inconsistent with the true presentation
//! order encoded in the H.264 bitstream itself.
//!
//! A container's `stts`/`ctts` boxes are one place a recording can claim the
//! wrong presentation order, but they aren't the only place: the SEI
//! timestamp embedded in each sample can itself be mistagged at record time
//! (e.g. associated with the wrong encoder output when B-frame reordering
//! delays that output relative to when it was submitted for encoding),
//! independent of what the container boxes say. Such a file has no
//! trustworthy timing metadata left in the container at all: neither `ctts`
//! nor the SEI can be assumed correct.
//!
//! The one signal that cannot lie is the bitstream's own picture order count
//! (POC, ITU-T H.264 §8.2.1): every slice header carries enough information
//! to reconstruct the true relative display order of samples, independent of
//! any container metadata or of what a (possibly buggy) writer put in the
//! SEI. This tool decodes POC for every sample and checks whether sorting
//! samples by POC reproduces non-decreasing SEI timestamps. If not, the
//! SEI data is inconsistent with the bitstream's real presentation order and
//! the file is broken.

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
    // The `fix` subcommand is temporarily disabled: its repair strategy
    // assumed the SEI timestamps were trustworthy and only the container's
    // `ctts` needed correcting. That assumption doesn't hold for files where
    // the SEI itself is mistagged (the case `check` is now built to find),
    // so there is currently no valid repair for what `check` detects. See
    // the commented-out implementation further down in this file.
}

/// One H.264 sample, in the order it appears in the file (decode order).
struct LoadedFrame {
    /// True capture/presentation time, read from the per-frame
    /// precision-timestamp SEI, relative to the source's frame0 time.
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

/// Parse an SPS NAL and extract its `log2_max_pic_order_cnt_lsb_minus4`
/// (required to reconstruct POC). Only `pic_order_cnt_type == 0` is supported.
fn parse_sps(nal: &RefNal<'_>, path: &Utf8PathBuf) -> Result<(SeqParameterSet, u8)> {
    let sps = SeqParameterSet::from_bits(nal.rbsp_bits()).map_err(|e| eyre!("bad SPS: {e:?}"))?;
    let log2_max_pic_order_cnt_lsb_minus4 = match sps.pic_order_cnt {
        PicOrderCntType::TypeZero {
            log2_max_pic_order_cnt_lsb_minus4,
        } => log2_max_pic_order_cnt_lsb_minus4,
        _ => bail!(
            "\"{path}\" uses a pic_order_cnt_type other than 0, which this tool doesn't \
            support yet"
        ),
    };
    Ok((sps, log2_max_pic_order_cnt_lsb_minus4))
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

/// Open `path` (either an MP4 or a raw Annex B `.h264` file) and load its
/// frames. Both container types decode to the same H.264 samples; only the
/// builder differs, so the per-frame analysis in [`load_frames`] is shared.
fn read_source(path: &Utf8PathBuf) -> Result<Vec<LoadedFrame>> {
    let builder = frame_source::FrameSourceBuilder::new(path)
        .do_decode_h264(false)
        .timestamp_source(frame_source::TimestampSource::MispMicrosectime);

    let ext = path.extension().map(|e| e.to_lowercase());
    let ctx = || {
        format!(
            "opening \"{path}\" (this tool requires per-frame precision-timestamp \
            SEI data, as written by strand-cam / braid)"
        )
    };
    match ext.as_deref() {
        Some("mp4") => load_frames(
            &mut builder.build_h264_in_mp4_source().with_context(ctx)?,
            path,
        ),
        Some("h264") => load_frames(
            &mut builder.build_h264_annexb_source().with_context(ctx)?,
            path,
        ),
        _ => bail!("\"{path}\": unsupported extension (expected .mp4 or .h264)"),
    }
}

/// Reconstruct picture order count (POC) and read the SEI capture time for
/// every sample of an already-opened H.264 source.
fn load_frames<H: SeekableH264Source>(
    src: &mut H264Source<H>,
    path: &Utf8PathBuf,
) -> Result<Vec<LoadedFrame>> {
    let mut ctx = H264ParsingContext::default();
    // The POC decoder needs the SPS's `log2_max_pic_order_cnt_lsb`, so it can
    // only be built once an SPS has been seen. MP4 keeps SPS/PPS in the
    // container; Annex B streams carry them inline (picked up in the loop).
    let mut poc_decoder: Option<PocDecoder> = None;

    if let Some(sps_bytes) = src.as_seekable_h264_source().first_sps() {
        let nal = RefNal::new(&sps_bytes, &[], true);
        let (sps, log2_max_poc_lsb) = parse_sps(&nal, path)?;
        poc_decoder = Some(PocDecoder::new(log2_max_poc_lsb));
        ctx.put_seq_param_set(sps);
    }
    if let Some(pps_bytes) = src.as_seekable_h264_source().first_pps() {
        let nal = RefNal::new(&pps_bytes, &[], true);
        let pps = PicParameterSet::from_bits(&ctx, nal.rbsp_bits())
            .map_err(|e| eyre!("bad PPS: {e:?}"))?;
        ctx.put_pic_param_set(pps);
    }

    let mut frames = Vec::new();
    for frame in src.iter() {
        let frame = frame?;
        let pts_ns = frame.timestamp().unwrap_duration().as_nanos() as i64;
        let nals = match frame.into_image() {
            frame_source::ImageData::EncodedH264(encoded) => match encoded.data {
                frame_source::H264EncodingVariant::RawEbsp(nals) => nals,
                other => bail!("expected raw-EBSP H264 sample data, got {other:?}"),
            },
            other => bail!("expected H264-encoded frame data, got {other:?}"),
        };

        // Feed any inline SPS/PPS (Annex B) so the parsing context is ready
        // before the slice headers that reference them. For MP4 these were
        // already seeded from the container and are absent from the samples.
        for nal_bytes in &nals {
            let nal = RefNal::new(nal_bytes, &[], true);
            let Ok(header) = nal.header() else { continue };
            match header.nal_unit_type() {
                UnitType::SeqParameterSet => {
                    let (sps, log2_max_poc_lsb) = parse_sps(&nal, path)?;
                    poc_decoder.get_or_insert_with(|| PocDecoder::new(log2_max_poc_lsb));
                    ctx.put_seq_param_set(sps);
                }
                UnitType::PicParameterSet => {
                    let pps = PicParameterSet::from_bits(&ctx, nal.rbsp_bits())
                        .map_err(|e| eyre!("bad PPS: {e:?}"))?;
                    ctx.put_pic_param_set(pps);
                }
                _ => {}
            }
        }

        let (is_idr, nal_ref_idc, poc_lsb) = read_slice_poc_lsb(&ctx, &nals)?;
        let poc_decoder = poc_decoder
            .as_mut()
            .ok_or_else(|| eyre!("\"{path}\": slice data appeared before any SPS"))?;
        let poc = poc_decoder.next_poc(is_idr, nal_ref_idc, poc_lsb);
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

/// Compare the bitstream's true picture order (POC) against the SEI capture
/// times: sort samples by POC and check whether the SEI timestamps come out
/// non-decreasing.
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
        match read_source(path) {
            Ok(frames) => {
                let analysis = analyze(&frames);
                if analysis.is_broken() {
                    any_broken = true;
                    println!(
                        "BROKEN  {path}  ({} of {} samples' SEI timestamps inconsistent with \
                        bitstream POC order, up to {:.1}ms early)",
                        analysis.num_inversions, analysis.num_frames, analysis.max_inversion_ms
                    );
                } else {
                    println!("OK      {path}  ({} samples)", analysis.num_frames);
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
    }

    Ok(())
}

// The `fix` subcommand, disabled while `check`'s new POC-based detection is
// validated (see the module doc comment above for why the old repair
// strategy no longer applies to what `check` now finds). This will need
// rework before being re-enabled: `LoadedFrame` no longer carries the raw
// sample data or container timing it used.
//
// /// Repair affected MP4 files by rewriting their timing metadata.
// Fix {
//     #[arg(required = true, num_args = 1..)]
//     inputs: Vec<Utf8PathBuf>,
//     /// Write the repaired copy here instead of overwriting in place.
//     /// Only valid with a single input file.
//     #[arg(long)]
//     output: Option<Utf8PathBuf>,
//     /// Rewrite the file even if analysis says it is already fine.
//     #[arg(long)]
//     force: bool,
//     /// When overwriting in place, don't keep a `.bak` copy of the original.
//     #[arg(long)]
//     no_backup: bool,
// },
//
// /// Compute (decode_duration, composition_offset) for every sample, keeping
// /// decode order untouched but assigning a nominal, evenly spaced decode
// /// timeline so that `decode_time + composition_offset == true capture time`
// /// for every sample.
// fn repair_timing(frames: &[LoadedFrame]) -> Vec<(std::time::Duration, chrono::Duration)> {
//     let n = frames.len();
//     assert!(n > 0);
//
//     let avg_interval_ns: i64 = if n > 1 {
//         let min = frames.iter().map(|f| f.pts_ns).min().unwrap();
//         let max = frames.iter().map(|f| f.pts_ns).max().unwrap();
//         (((max - min) as f64) / ((n - 1) as f64)).round() as i64
//     } else {
//         // Nominal fallback: a single-frame file has no reordering to fix
//         // anyway, but every mp4 sample needs a nonzero duration.
//         (1_000_000_000f64 / 30.0).round() as i64
//     }
//     .max(1);
//
//     (0..n)
//         .map(|i| {
//             let dts_ns = avg_interval_ns * i as i64;
//             let offset_ns = frames[i].pts_ns - dts_ns;
//             (
//                 std::time::Duration::from_nanos(avg_interval_ns as u64),
//                 chrono::Duration::nanoseconds(offset_ns),
//             )
//         })
//         .collect()
// }
//
// fn write_repaired(loaded: &Loaded, out_path: &Utf8PathBuf) -> Result<()> {
//     let timing = repair_timing(&loaded.frames);
//
//     let fd = std::fs::File::create(out_path)
//         .with_context(|| format!("creating output file \"{out_path}\""))?;
//     let cfg = Mp4RecordingConfig {
//         codec: Mp4Codec::H264RawStream,
//         max_framerate: RecordingFrameRate::Unlimited,
//         h264_metadata: None,
//     };
//     let mut new_mp4 = mp4_writer::Mp4Writer::new(fd, cfg, None)?;
//     new_mp4.set_first_sps_pps(loaded.first_sps.clone(), loaded.first_pps.clone());
//
//     let frame0_time_local: chrono::DateTime<chrono::Local> =
//         loaded.frame0_time.with_timezone(&chrono::Local);
//
//     for (frame, (decode_duration, composition_offset)) in loaded.frames.iter().zip(timing) {
//         let sei_timestamp = frame0_time_local + chrono::Duration::nanoseconds(frame.pts_ns);
//         new_mp4.write_h264_buf_passthrough(
//             &frame.data,
//             loaded.width,
//             loaded.height,
//             decode_duration,
//             composition_offset,
//             sei_timestamp,
//             true,
//         )?;
//     }
//
//     new_mp4.finish()?;
//     Ok(())
// }
//
// fn cmd_fix(
//     inputs: &[Utf8PathBuf],
//     output: Option<&Utf8PathBuf>,
//     force: bool,
//     no_backup: bool,
// ) -> Result<()> {
//     if output.is_some() && inputs.len() != 1 {
//         bail!("--output can only be used with a single input file");
//     }
//
//     for path in inputs {
//         let loaded = read_source(path)?;
//         let analysis = analyze(&loaded.frames);
//
//         if !analysis.needs_fix() && !force {
//             println!(
//                 "{path}: already OK, skipping ({} samples)",
//                 analysis.num_frames
//             );
//             continue;
//         }
//
//         if let Some(output) = output {
//             write_repaired(&loaded, output)?;
//             println!(
//                 "{path}: repaired {} of {} samples, wrote \"{output}\"",
//                 analysis.num_inversions, analysis.num_frames
//             );
//         } else {
//             let tmp_path: Utf8PathBuf = format!("{path}.mp4-bframe-doctor-tmp").into();
//             write_repaired(&loaded, &tmp_path)?;
//
//             if !no_backup {
//                 let backup_path: Utf8PathBuf = format!("{path}.bak").into();
//                 if backup_path.exists() {
//                     bail!(
//                         "backup path \"{backup_path}\" already exists; refusing to overwrite. \
//                         Remove it or pass --no-backup."
//                     );
//                 }
//                 std::fs::rename(path, &backup_path)
//                     .with_context(|| format!("renaming \"{path}\" to backup \"{backup_path}\""))?;
//             }
//             std::fs::rename(&tmp_path, path)
//                 .with_context(|| format!("renaming repaired file into place at \"{path}\""))?;
//
//             println!(
//                 "{path}: repaired {} of {} samples in place{}",
//                 analysis.num_inversions,
//                 analysis.num_frames,
//                 if no_backup {
//                     ""
//                 } else {
//                     " (original kept as .bak)"
//                 }
//             );
//         }
//     }
//
//     Ok(())
// }

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(pts_ns: i64, poc: i64) -> LoadedFrame {
        LoadedFrame { pts_ns, poc }
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
}
