// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Insert per-frame MISPmicrosectime precision-timestamp SEI NAL units into an
//! MP4's H.264 stream, without decoding or re-encoding the video.
//!
//! The timestamps to embed are read from a per-frame timing source -- by
//! default a companion SubRip (`.srt`) subtitle file, as written alongside
//! Strand Camera recordings -- and spliced into the existing H.264 samples as
//! new NAL units while the samples themselves are copied through unchanged.
//! The container's original per-sample timing (`stts`/`ctts`) is preserved
//! verbatim, so this works correctly even for reordered (B-frame) streams.

use camino::Utf8PathBuf;
use clap::{Parser, ValueEnum};
use eyre::{Context, Result, bail};
use frame_source::{FrameDataSource, h264_source::SeekableH264Source};
use strand_cam_remote_control::{Mp4Codec, Mp4RecordingConfig, RecordingFrameRate};

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Cli {
    /// Input MP4 file.
    input: Utf8PathBuf,

    /// Output MP4 file. Defaults to the input's path with `-misp` inserted
    /// before the `.mp4` extension.
    #[arg(long)]
    output: Option<Utf8PathBuf>,

    /// SRT file with per-frame timestamps. Only used when
    /// `--timestamp-source srt-file` (the default). Defaults to the input's
    /// path with its extension changed to `.srt`.
    #[arg(long)]
    srt: Option<Utf8PathBuf>,

    /// Source of the per-frame timestamps to embed as MISP SEI.
    #[arg(long, value_enum, default_value_t)]
    timestamp_source: TimestampSource,

    /// Overwrite the output file if it already exists.
    #[arg(long)]
    force: bool,

    /// Exit successfully (code 0) even if an error partway through forced the
    /// output to be truncated (e.g. a malformed SRT file, or a source frame
    /// that failed to decode). Without this flag, a truncated output is
    /// still written to `output`, but the process exits non-zero.
    #[arg(long)]
    allow_truncated: bool,
}

#[derive(Default, Debug, Clone, Copy, ValueEnum, PartialEq)]
enum TimestampSource {
    BestGuess,
    FrameInfoRecvTime,
    Mp4Pts,
    #[default]
    SrtFile,
}

impl From<TimestampSource> for frame_source::TimestampSource {
    fn from(orig: TimestampSource) -> Self {
        match orig {
            TimestampSource::BestGuess => frame_source::TimestampSource::BestGuess,
            TimestampSource::FrameInfoRecvTime => frame_source::TimestampSource::FrameInfoRecvTime,
            TimestampSource::Mp4Pts => frame_source::TimestampSource::Mp4Pts,
            TimestampSource::SrtFile => frame_source::TimestampSource::SrtFile,
        }
    }
}

fn default_output_path(input: &Utf8PathBuf) -> Utf8PathBuf {
    let stem = input.file_stem().unwrap_or(input.as_str());
    input.with_file_name(format!("{stem}-misp.mp4"))
}

/// Outcome of a (possibly truncated) [`insert_misp`] run.
struct InsertMispOutcome {
    /// Number of frames actually written to `output`.
    frames_written: usize,
    /// Number of frames in the source container.
    total_frames: usize,
    /// Set when `frames_written < total_frames`, explaining why.
    truncated_reason: Option<String>,
}

/// Remux `input` into `output`, inserting a MISP precision-timestamp SEI NAL
/// unit ahead of every H.264 sample using timestamps from `timestamp_source`
/// (reading `srt_file_path` when that source is [`TimestampSource::SrtFile`]).
/// Samples and the container's original per-sample timing are copied through
/// unchanged.
///
/// If an error occurs after at least one frame has been written -- e.g. the
/// SRT file ran out of usable timestamps, or a later source frame failed to
/// decode -- `output` is still finished as a valid, truncated MP4 covering
/// the frames written so far, and the reason is reported in the returned
/// [`InsertMispOutcome`] rather than as an `Err`. Only a failure before any
/// frame could be written is a hard error (in which case `output` is not
/// left behind).
fn insert_misp(
    input: &Utf8PathBuf,
    output: &Utf8PathBuf,
    timestamp_source: frame_source::TimestampSource,
    srt_file_path: Option<std::path::PathBuf>,
) -> Result<InsertMispOutcome> {
    let mut frame_src = frame_source::FrameSourceBuilder::new(input)
        .do_decode_h264(false)
        .timestamp_source(timestamp_source)
        .srt_file_path(srt_file_path)
        .build_h264_in_mp4_source()
        .with_context(|| format!("opening \"{input}\""))?;

    let frame0_time = frame_src
        .frame0_time()
        .ok_or_else(|| eyre::eyre!("\"{input}\": source has no frame0 time"))?;

    let width = frame_src.width();
    let height = frame_src.height();
    let h264_src = frame_src.as_seekable_h264_source();
    let first_sps = h264_src.first_sps();
    let first_pps = h264_src.first_pps();

    let srt_truncation_reason = frame_src
        .srt_truncation()
        .map(|t| match t.malformed_at_line {
            Some(line) => format!(
                "SRT file is malformed starting around line {line}; only {} of {} frames in the \
             source have usable timestamps (rounded down to the last complete group of \
             pictures)",
                t.usable_stanzas, t.total_frames
            ),
            None => format!(
                "SRT file only has {} timestamp entries for {} frames in the source (rounded down \
             to the last complete group of pictures)",
                t.usable_stanzas, t.total_frames
            ),
        });

    // Snapshot the source's per-sample timing (stts + ctts) before iterating
    // (which borrows `frame_src` mutably). Preserving this timing verbatim is
    // what keeps reordered (B-frame) streams correct: the container ordering
    // comes from the source, while the precise capture time is carried
    // per-frame in the precision-timestamp SEI. Its length already reflects
    // any SRT-driven truncation above.
    let sample_timing = frame_src
        .mp4_sample_timing()
        .ok_or_else(|| eyre::eyre!("\"{input}\": source has no per-sample timing"))?
        .to_vec();
    let total_frames = frame_src
        .srt_truncation()
        .map(|t| t.total_frames)
        .unwrap_or(sample_timing.len());

    let fd = std::fs::File::create(output)
        .with_context(|| format!("creating output file \"{output}\""))?;
    let cfg = Mp4RecordingConfig {
        codec: Mp4Codec::H264RawStream,
        max_framerate: RecordingFrameRate::Unlimited,
        h264_metadata: None,
    };
    let mut new_mp4 = mp4_writer::Mp4Writer::new(fd, cfg, None)?;
    new_mp4.set_first_sps_pps(first_sps, first_pps);

    let mut count = 0;
    let mut loop_error = None;
    for frame in frame_src.decode_order_iter() {
        let frame = match frame {
            Ok(frame) => frame,
            Err(e) => {
                loop_error = Some(format!("frame {count}: {e}"));
                break;
            }
        };
        let idx = frame.idx();
        let timestamp = frame0_time + frame.timestamp().unwrap_duration();
        let data = match frame.image() {
            frame_source::ImageData::EncodedH264(data) => &data.data,
            _ => {
                loop_error = Some(format!("frame {idx}: expected H264-encoded frame data"));
                break;
            }
        };
        let Some(timing) = sample_timing.get(idx) else {
            loop_error = Some(format!("frame {idx}: missing sample timing"));
            break;
        };
        if let Err(e) = new_mp4.write_h264_buf_passthrough(
            data,
            width,
            height,
            timing.decode_duration,
            timing.composition_offset,
            timestamp,
            true,
        ) {
            loop_error = Some(format!("frame {idx}: {e}"));
            break;
        }
        count += 1;
    }

    if count == 0 {
        drop(new_mp4);
        let _ = std::fs::remove_file(output);
        let reason = loop_error
            .or(srt_truncation_reason)
            .unwrap_or_else(|| "no frames available".to_string());
        bail!("\"{input}\": failed before any frame could be written: {reason}");
    }

    new_mp4
        .finish()
        .with_context(|| format!("finishing truncated output \"{output}\" after {count} frames"))?;

    let truncated_reason = match (srt_truncation_reason, loop_error) {
        (Some(srt_reason), Some(loop_reason)) => Some(format!("{srt_reason}; {loop_reason}")),
        (Some(reason), None) | (None, Some(reason)) => Some(reason),
        (None, None) => None,
    };

    Ok(InsertMispOutcome {
        frames_written: count,
        total_frames,
        truncated_reason,
    })
}

fn main() -> Result<()> {
    env_tracing_logger::init();
    let cli = Cli::parse();

    let output = cli
        .output
        .clone()
        .unwrap_or_else(|| default_output_path(&cli.input));
    if output.exists() && !cli.force {
        bail!("output file \"{output}\" already exists. Pass --force to overwrite.");
    }

    let srt_file_path = match cli.timestamp_source {
        TimestampSource::SrtFile => {
            let srt = cli
                .srt
                .clone()
                .unwrap_or_else(|| cli.input.with_extension("srt"));
            if !srt.exists() {
                bail!(
                    "timestamp source is srt-file, but SRT file \"{srt}\" does not exist. \
                    Pass --srt to specify a different path."
                );
            }
            Some(srt.into())
        }
        _ => None,
    };

    let outcome = insert_misp(
        &cli.input,
        &output,
        cli.timestamp_source.into(),
        srt_file_path,
    )?;

    match &outcome.truncated_reason {
        Some(reason) => {
            tracing::error!(
                "\"{output}\" was truncated: wrote {} of {} frames from \"{}\". Reason: {reason}",
                outcome.frames_written,
                outcome.total_frames,
                cli.input,
            );
            if !cli.allow_truncated {
                bail!(
                    "output was truncated (see error above). Pass --allow-truncated to accept \
                    a truncated output and exit successfully."
                );
            }
        }
        None => {
            println!(
                "Wrote {} frames with MISP precision timestamps to \"{output}\".",
                outcome.frames_written
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use chrono::{DateTime, Local};
    use ffmpeg_writer::{FfmpegCodecArgs, FfmpegWriter};
    use frame_source::h264_source::Mp4SampleTiming;
    use machine_vision_formats::{owned::OImage, pixel_format::RGB8};
    use serde::Serialize;

    #[derive(Serialize)]
    struct SrtMsg {
        timestamp: DateTime<Local>,
    }

    /// `<dir>/in.mp4`, `<dir>/in.srt`, `<dir>/in.out.mp4` for a test.
    fn test_paths(dir: &std::path::Path) -> (Utf8PathBuf, Utf8PathBuf, Utf8PathBuf) {
        let mp4_path = Utf8PathBuf::from_path_buf(dir.join("in.mp4")).unwrap();
        let srt_path = mp4_path.with_extension("srt");
        let out_path = mp4_path.with_extension("out.mp4");
        (mp4_path, srt_path, out_path)
    }

    /// Encode `timestamps.len()` frames of varying content (so an encoder
    /// forcing B-frames has real motion to reorder around) to `mp4_path`
    /// using `ffmpeg_codec_args`, writing each frame's SRT stanza (keyed by
    /// its mp4 PTS) with the corresponding capture time to `srt_path`.
    fn write_test_video_with_srt(
        mp4_path: &Utf8PathBuf,
        srt_path: &Utf8PathBuf,
        ffmpeg_codec_args: FfmpegCodecArgs,
        w: u32,
        h: u32,
        timestamps: &[DateTime<Local>],
    ) -> Result<()> {
        let mut ffmpeg_wtr = FfmpegWriter::new(mp4_path.as_str(), ffmpeg_codec_args, None)?;
        let srt_fd = std::fs::File::create(srt_path)?;
        let mut srt_wtr = srt_writer::BufferingSrtFrameWriter::new(Box::new(srt_fd));

        for (i, ts) in timestamps.iter().enumerate() {
            let mut data = vec![0u8; w as usize * h as usize * 3];
            for (px, chunk) in data.chunks_exact_mut(3).enumerate() {
                let v = ((px + i * 7) % 256) as u8;
                chunk[0] = v;
                chunk[1] = v.wrapping_mul(3);
                chunk[2] = v.wrapping_add(i as u8 * 11);
            }
            let frame: OImage<RGB8> = OImage::new(w, h, w as usize * 3, data).unwrap();
            let frame = strand_dynamic_frame::DynamicFrameOwned::from_static(frame);
            let pts = ffmpeg_wtr.write_dynamic_frame(&frame.borrow())?;
            let msg = serde_json::to_string(&SrtMsg { timestamp: *ts }).unwrap();
            srt_wtr.add_frame(pts, msg)?;
            srt_wtr.flush()?;
        }
        ffmpeg_wtr.close()?;
        srt_wtr.close()?;
        Ok(())
    }

    fn test_timestamps(n: usize) -> Vec<DateTime<Local>> {
        let timestamp_micros: i64 = 1_662_921_288_000_000; // Sun, 11 Sep 2022 18:34:48 UTC
        let frame_interval_micros = 40_000i64; // 25 fps
        (0..n)
            .map(|i| {
                DateTime::from_timestamp_micros(timestamp_micros + i as i64 * frame_interval_micros)
                    .unwrap()
                    .with_timezone(&Local)
            })
            .collect()
    }

    /// Round-trip test: write a plain (no MISP) MP4 plus an external `.srt`
    /// giving each frame its capture time, run [`insert_misp`] against that
    /// pair as `mp4-misp-inserter` itself does, then read the result back and
    /// check the embedded MISP timestamps match what the `.srt` specified.
    ///
    /// B-frames are explicitly disabled (`max_bframes: Some(0)`) so decode
    /// order matches presentation order here; the reordered case is covered
    /// separately by [`test_insert_misp_from_srt_with_bframes`].
    #[test]
    fn test_insert_misp_from_srt() -> Result<()> {
        let tempdir = tempfile::tempdir()?;
        let (mp4_path, srt_path, out_path) = test_paths(tempdir.path());
        let timestamps = test_timestamps(5);

        let ffmpeg_codec_args = FfmpegCodecArgs {
            max_bframes: Some(0),
            ..Default::default()
        };
        write_test_video_with_srt(&mp4_path, &srt_path, ffmpeg_codec_args, 64, 48, &timestamps)?;

        let outcome = insert_misp(
            &mp4_path,
            &out_path,
            frame_source::TimestampSource::SrtFile,
            Some(srt_path.into()),
        )?;
        assert_eq!(outcome.frames_written, timestamps.len());
        assert_eq!(outcome.total_frames, timestamps.len());
        assert!(outcome.truncated_reason.is_none());

        let mut frame_src = frame_source::FrameSourceBuilder::new(&out_path)
            .do_decode_h264(false)
            .timestamp_source(frame_source::TimestampSource::MispMicrosectime)
            .build_source()?;
        let frame0_time = frame_src.frame0_time().unwrap();
        assert_eq!(frame0_time, timestamps[0]);

        // Frames are read back in decode order, which need not match
        // presentation (input) order; compare the sets of capture times
        // rather than their order.
        let mut got = Vec::new();
        for frame in frame_src.decode_order_iter() {
            let frame = frame?;
            got.push(frame0_time + frame.timestamp().unwrap_duration());
        }
        got.sort();
        let mut expected = timestamps.clone();
        expected.sort();
        assert_eq!(got, expected);

        Ok(())
    }

    /// Same round-trip as [`test_insert_misp_from_srt`], but the source MP4
    /// is deliberately encoded with B-frames (reordered decode order, `ctts`
    /// composition offsets), which exercises the `write_h264_buf_passthrough`
    /// path rather than the sequential-timestamp fallback. Verifies that
    /// after `insert_misp` remuxes the file, walking samples in
    /// *presentation* order (reconstructed from the container's `stts` +
    /// `ctts`) still yields exactly the capture times given in the `.srt`,
    /// in the order they were written.
    #[test]
    fn test_insert_misp_from_srt_with_bframes() -> Result<()> {
        let tempdir = tempfile::tempdir()?;
        let (mp4_path, srt_path, out_path) = test_paths(tempdir.path());
        let n_frames = 24;
        let timestamps = test_timestamps(n_frames);

        // Force libx264 to insert a fixed pattern of B-frames (b_adapt=0
        // takes the content out of the decision) so the remux definitely
        // exercises the reordered path. A single keyframe keeps one GOP.
        let ffmpeg_codec_args = FfmpegCodecArgs {
            device_args: None,
            pre_codec_args: None,
            codec: Some("libx264".to_string()),
            post_codec_args: Some(vec![
                ("-bf".to_string(), "3".to_string()),
                (
                    "-x264-params".to_string(),
                    "b_adapt=0:scenecut=0:keyint=1000:min-keyint=1000".to_string(),
                ),
            ]),
            pixfmt: Some("yuv420p".to_string()),
            // This test deliberately forces B-frames via `-bf 3` in
            // `post_codec_args` to exercise reordering, so do not emit the
            // default `-bf 0`.
            max_bframes: None,
        };
        write_test_video_with_srt(&mp4_path, &srt_path, ffmpeg_codec_args, 64, 48, &timestamps)?;

        let outcome = insert_misp(
            &mp4_path,
            &out_path,
            frame_source::TimestampSource::SrtFile,
            Some(srt_path.into()),
        )?;
        assert_eq!(outcome.frames_written, n_frames);
        assert!(outcome.truncated_reason.is_none());

        // Read the re-muxed file back, keeping the H264 in decode order and
        // recovering the container timing so we can reconstruct presentation
        // order.
        let mut frame_src = frame_source::FrameSourceBuilder::new(&out_path)
            .do_decode_h264(false)
            .timestamp_source(frame_source::TimestampSource::MispMicrosectime)
            .build_h264_in_mp4_source()?;

        let frame0_time = frame_src.frame0_time().unwrap();
        assert_eq!(frame0_time, timestamps[0]);

        // Snapshot per-sample timing (stts + ctts) before iterating (which
        // borrows the source mutably).
        let sample_timing: Vec<Mp4SampleTiming> = frame_src
            .mp4_sample_timing()
            .expect("MP4 source must expose per-sample timing")
            .to_vec();
        assert_eq!(sample_timing.len(), n_frames);

        // The remux is only meaningful as a reordering test if the encoder
        // actually produced B-frames (non-zero composition offsets).
        let has_reordering = sample_timing
            .iter()
            .any(|t| t.composition_offset != chrono::Duration::zero());
        assert!(
            has_reordering,
            "expected libx264 to emit B-frames (non-zero ctts); test would be vacuous otherwise"
        );

        // Collect the SEI capture time for each sample, in decode order.
        let mut sei_times = vec![None; n_frames];
        for frame in frame_src.decode_order_iter() {
            let frame = frame?;
            sei_times[frame.idx()] = Some(frame0_time + frame.timestamp().unwrap_duration());
        }

        // Reconstruct each sample's presentation time: presentation = decode +
        // composition_offset, where the decode time is the running sum of the
        // per-sample decode durations (stts), all in decode order.
        let mut decode_time = chrono::Duration::zero();
        let mut presentation = Vec::with_capacity(n_frames);
        for (i, timing) in sample_timing.iter().enumerate() {
            let pts = decode_time
                + chrono::Duration::from_std(timing.decode_duration).unwrap()
                + timing.composition_offset;
            let sei = sei_times[i].expect("every sample must carry a SEI timestamp");
            presentation.push((pts, sei));
            decode_time += chrono::Duration::from_std(timing.decode_duration).unwrap();
        }

        // Walk samples in presentation order and assert the SEI capture times
        // match exactly what was written, in that order: the remuxed file
        // plays back with the correct capture time on every frame.
        presentation.sort_by_key(|(pts, _)| *pts);
        let ordered_sei: Vec<_> = presentation.iter().map(|(_, sei)| *sei).collect();
        assert_eq!(ordered_sei, timestamps);

        Ok(())
    }

    /// When the SRT file becomes malformed partway through, `insert_misp`
    /// must still produce a valid, truncated output rather than fail
    /// outright: it keeps only whole groups of pictures (GOPs) fully covered
    /// by a usable stanza, dropping the incomplete trailing GOP, and reports
    /// the truncation via [`InsertMispOutcome::truncated_reason`] instead of
    /// an `Err`.
    #[test]
    fn test_insert_misp_truncates_on_malformed_srt() -> Result<()> {
        let tempdir = tempfile::tempdir()?;
        let (mp4_path, srt_path, out_path) = test_paths(tempdir.path());
        let n_frames = 24;
        let timestamps = test_timestamps(n_frames);

        // Force short (5-frame) GOPs with B-frames, so the video has several
        // GOP boundaries to round down to rather than just one.
        let ffmpeg_codec_args = FfmpegCodecArgs {
            device_args: None,
            pre_codec_args: None,
            codec: Some("libx264".to_string()),
            post_codec_args: Some(vec![
                ("-bf".to_string(), "3".to_string()),
                (
                    "-x264-params".to_string(),
                    "b_adapt=0:scenecut=0:keyint=5:min-keyint=5".to_string(),
                ),
            ]),
            pixfmt: Some("yuv420p".to_string()),
            max_bframes: None,
        };
        write_test_video_with_srt(&mp4_path, &srt_path, ffmpeg_codec_args, 64, 48, &timestamps)?;

        // Corrupt the SRT: keep the first 13 stanzas intact, then append
        // content that won't parse as a stanza, simulating a truncated or
        // corrupted timestamp log.
        let srt_text = std::fs::read_to_string(&srt_path)?;
        let stanzas: Vec<&str> = srt_text.split("\n\n").collect();
        assert!(
            stanzas.len() > 13,
            "test fixture must have more than 13 stanzas"
        );
        let corrupted = format!("{}\n\nNOT A VALID STANZA\n", stanzas[..13].join("\n\n"));
        std::fs::write(&srt_path, corrupted)?;

        let outcome = insert_misp(
            &mp4_path,
            &out_path,
            frame_source::TimestampSource::SrtFile,
            Some(srt_path.into()),
        )?;

        // 13 usable stanzas; GOPs of size 5 (boundaries at presentation ranks
        // 0, 5, 10, 15, 20) round down to the last complete GOP at or before
        // 13, i.e. 10 frames.
        assert_eq!(outcome.frames_written, 10);
        assert_eq!(outcome.total_frames, n_frames);
        let reason = outcome.truncated_reason.expect("must report truncation");
        assert!(reason.contains("malformed"), "reason: {reason}");

        // The output must still be a valid, playable MP4 with exactly the
        // kept frames.
        let mut frame_src = frame_source::FrameSourceBuilder::new(&out_path)
            .do_decode_h264(false)
            .timestamp_source(frame_source::TimestampSource::MispMicrosectime)
            .build_h264_in_mp4_source()?;
        let mut got = 0;
        for frame in frame_src.decode_order_iter() {
            frame?;
            got += 1;
        }
        assert_eq!(got, 10);

        Ok(())
    }
}
