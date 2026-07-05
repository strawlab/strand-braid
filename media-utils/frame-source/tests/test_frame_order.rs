// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Assert the ordering guarantee of the main iterator loop
//! (`FrameDataSource::iter`) for H264 sources, including streams encoded with
//! B-frames.
//!
//! ## What "in order" means here
//!
//! By design, `iter()` yields frames in **decode order** (the order the samples
//! are stored in the stream), not presentation order. This is deliberate and
//! relied upon downstream: e.g. `ffmpeg-rewriter` re-muxes by writing samples
//! back in decode order and reconstructs presentation order itself from the
//! per-sample composition offsets. Feeding an H264 decoder also requires decode
//! order.
//!
//! The concrete, testable invariant is therefore: frames come out in a strict,
//! gap-free decode sequence -- `frame.idx()` is `0, 1, 2, ... N-1` in the order
//! yielded, with none skipped, duplicated, or reordered. For B-frame streams the
//! presentation timestamps (PTS) are *not* monotonic in this order (that is the
//! whole point of B-frames); we assert instead that the reported PTS values form
//! a valid presentation schedule -- distinct and, once sorted, strictly
//! increasing -- and that the fixture genuinely exercises reordering.
//!
//! The fixtures are generated on the fly with `ffmpeg` (the same approach used
//! by `mp4-writer`'s roundtrip test; CI installs ffmpeg for exactly this kind of
//! test).

use eyre::{Context, Result};

use frame_source::{FrameData, Timestamp, TimestampSource};

/// Run ffmpeg to encode a short, deterministic clip with B-frames, so decode
/// order differs from presentation order. `extra_args` selects the container /
/// bitstream format and output path.
fn ffmpeg_encode(out_path: &std::path::Path, extra_args: &[&str]) -> Result<()> {
    let out_str = format!("{}", out_path.display());
    let mut args: Vec<&str> = vec![
        "-y",
        "-v",
        "error",
        "-f",
        "lavfi",
        "-i",
        "testsrc=size=32x32:rate=10:duration=2",
        "-c:v",
        "libx264",
        "-bf",
        "2",
        "-g",
        "10",
        "-pix_fmt",
        "yuv420p",
        "-x264-params",
        "bframes=2:b-pyramid=normal",
    ];
    args.extend_from_slice(extra_args);
    args.push(&out_str);

    let output = std::process::Command::new("ffmpeg")
        .args(&args)
        .output()
        .with_context(|| format!("When running: ffmpeg {:?}", args))?;
    if !output.status.success() {
        eyre::bail!(
            "'ffmpeg {}' failed. stdout: {}, stderr: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

/// Assert the emitted frames form a strict, gap-free decode sequence: `idx()` is
/// `0, 1, 2, ... N-1` in the order yielded.
fn assert_gap_free_decode_order(frames: &[FrameData]) {
    assert!(
        frames.len() > 2,
        "expected several frames, got {}",
        frames.len()
    );
    for (position, frame) in frames.iter().enumerate() {
        assert_eq!(
            frame.idx(),
            position,
            "frame at iteration position {position} reported idx() = {} \
             (frames must be yielded in strict, gap-free decode order)",
            frame.idx()
        );
    }
}

#[test]
fn test_mp4_with_bframes_iterates_in_decode_order() -> Result<()> {
    let tmpdir = tempfile::tempdir()?;
    let path = tmpdir.path().join("bframes.mp4");
    ffmpeg_encode(&path, &[])?;

    let mut src = frame_source::FrameSourceBuilder::new(&path)
        // We only care about frame ordering, not the decoded pixels.
        .do_decode_h264(false)
        .timestamp_source(TimestampSource::Mp4Pts)
        .build_source()?;

    let frames: Vec<FrameData> = src.iter().collect::<Result<_, _>>()?;

    // Core invariant: strict, gap-free decode order.
    assert_gap_free_decode_order(&frames);

    // The reported presentation timestamps must form a valid presentation
    // schedule: all distinct and strictly increasing once sorted.
    let mut pts: Vec<std::time::Duration> = frames
        .iter()
        .map(|f| match f.timestamp() {
            Timestamp::Duration(d) => Ok(d),
            Timestamp::Fraction(_) => eyre::bail!("expected duration timestamps for an MP4 source"),
        })
        .collect::<Result<_>>()?;
    let emitted = pts.clone();
    pts.sort();
    for w in pts.windows(2) {
        assert!(
            w[1] > w[0],
            "presentation timestamps must be distinct and strictly increasing when sorted, \
             got {:?} then {:?}",
            w[0],
            w[1]
        );
    }

    // Sanity-check that this fixture actually exercises B-frame reordering:
    // decode order must NOT already equal presentation order, otherwise the test
    // would be vacuous.
    assert_ne!(
        emitted, pts,
        "expected libx264 to emit B-frames so decode order != presentation order; \
         fixture is not exercising reordering"
    );

    Ok(())
}

#[test]
fn test_raw_h264_annexb_with_bframes_iterates_in_decode_order() -> Result<()> {
    let tmpdir = tempfile::tempdir()?;
    let path = tmpdir.path().join("bframes.h264");
    // Emit a raw Annex B elementary stream (no container).
    ffmpeg_encode(&path, &["-f", "h264"])?;

    let mut src = frame_source::FrameSourceBuilder::new(&path)
        .do_decode_h264(false)
        .build_source()?;

    let frames: Vec<FrameData> = src.iter().collect::<Result<_, _>>()?;

    // A raw Annex B stream carries no per-frame timestamps, so the only ordering
    // guarantee is the strict, gap-free decode sequence. The "timestamp" is a
    // fraction-done value, which by construction increases monotonically.
    assert_gap_free_decode_order(&frames);

    let mut prev = -1.0f32;
    for frame in &frames {
        match frame.timestamp() {
            Timestamp::Fraction(f) => {
                assert!(
                    f > prev,
                    "fraction-done must increase monotonically, got {f} after {prev}"
                );
                prev = f;
            }
            Timestamp::Duration(_) => {
                eyre::bail!("expected fraction timestamps for a raw Annex B source")
            }
        }
    }

    Ok(())
}
