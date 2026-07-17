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

/// Remux `input` into `output`, inserting a MISP precision-timestamp SEI NAL
/// unit ahead of every H.264 sample using timestamps from `timestamp_source`
/// (reading `srt_file_path` when that source is [`TimestampSource::SrtFile`]).
/// Samples and the container's original per-sample timing are copied through
/// unchanged. Returns the number of frames written.
fn insert_misp(
    input: &Utf8PathBuf,
    output: &Utf8PathBuf,
    timestamp_source: frame_source::TimestampSource,
    srt_file_path: Option<std::path::PathBuf>,
) -> Result<usize> {
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

    // Snapshot the source's per-sample timing (stts + ctts) before iterating
    // (which borrows `frame_src` mutably). Preserving this timing verbatim is
    // what keeps reordered (B-frame) streams correct: the container ordering
    // comes from the source, while the precise capture time is carried
    // per-frame in the precision-timestamp SEI.
    let sample_timing = frame_src
        .mp4_sample_timing()
        .ok_or_else(|| eyre::eyre!("\"{input}\": source has no per-sample timing"))?
        .to_vec();

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
    for frame in frame_src.decode_order_iter() {
        let frame = frame?;
        let idx = frame.idx();
        let timestamp = frame0_time + frame.timestamp().unwrap_duration();
        let data = match frame.image() {
            frame_source::ImageData::EncodedH264(data) => &data.data,
            _ => bail!("\"{input}\": expected H264-encoded frame data"),
        };
        let timing = sample_timing
            .get(idx)
            .ok_or_else(|| eyre::eyre!("\"{input}\": missing sample timing for frame {idx}"))?;
        new_mp4.write_h264_buf_passthrough(
            data,
            width,
            height,
            timing.decode_duration,
            timing.composition_offset,
            timestamp,
            true,
        )?;
        count += 1;
    }

    new_mp4.finish()?;

    Ok(count)
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

    let count = insert_misp(
        &cli.input,
        &output,
        cli.timestamp_source.into(),
        srt_file_path,
    )?;

    println!("Wrote {count} frames with MISP precision timestamps to \"{output}\".");

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use chrono::{DateTime, Local};
    use ffmpeg_writer::{FfmpegCodecArgs, FfmpegWriter};
    use machine_vision_formats::{owned::OImage, pixel_format::RGB8};
    use serde::Serialize;

    #[derive(Serialize)]
    struct SrtMsg {
        timestamp: DateTime<Local>,
    }

    /// Round-trip test: write a plain (no MISP) MP4 plus an external `.srt`
    /// giving each frame its capture time, run [`insert_misp`] against that
    /// pair as `mp4-misp-inserter` itself does, then read the result back and
    /// check the embedded MISP timestamps match what the `.srt` specified.
    #[test]
    fn test_insert_misp_from_srt() -> Result<()> {
        let tempdir = tempfile::tempdir()?;
        let mp4_path: Utf8PathBuf =
            Utf8PathBuf::from_path_buf(tempdir.path().join("in.mp4")).unwrap();
        let srt_path = mp4_path.with_extension("srt");
        let out_path = mp4_path.with_extension("out.mp4");

        let timestamp_micros: i64 = 1_662_921_288_000_000; // Sun, 11 Sep 2022 18:34:48 UTC
        let timestamps: Vec<DateTime<Local>> = (0..5)
            .map(|i| {
                DateTime::from_timestamp_micros(timestamp_micros + i * 40_000)
                    .unwrap()
                    .with_timezone(&Local)
            })
            .collect();

        let w = 64;
        let h = 48;
        {
            let mut ffmpeg_wtr =
                FfmpegWriter::new(mp4_path.as_str(), FfmpegCodecArgs::default(), None)?;
            let srt_fd = std::fs::File::create(&srt_path)?;
            let mut srt_wtr = srt_writer::BufferingSrtFrameWriter::new(Box::new(srt_fd));

            for (i, ts) in timestamps.iter().enumerate() {
                let value = (i % 255) as u8;
                let frame: OImage<RGB8> = OImage::new(
                    w,
                    h,
                    w as usize * 3,
                    vec![value; w as usize * h as usize * 3],
                )
                .unwrap();
                let frame = strand_dynamic_frame::DynamicFrameOwned::from_static(frame);
                let pts = ffmpeg_wtr.write_dynamic_frame(&frame.borrow())?;
                let msg = serde_json::to_string(&SrtMsg { timestamp: *ts }).unwrap();
                srt_wtr.add_frame(pts, msg)?;
                srt_wtr.flush()?;
            }
            ffmpeg_wtr.close()?;
            srt_wtr.close()?;
        }

        let count = insert_misp(
            &mp4_path,
            &out_path,
            frame_source::TimestampSource::SrtFile,
            Some(srt_path.into()),
        )?;
        assert_eq!(count, timestamps.len());

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
}
