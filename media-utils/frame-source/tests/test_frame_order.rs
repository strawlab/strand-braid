// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Assert the ordering guarantees of the frame-source iterators for H264
//! sources, including streams encoded with B-frames.
//!
//! ## What the two iterators guarantee
//!
//! `decode_order_iter()` yields frames in **decode order** (the order the
//! samples are stored in the stream), not presentation order. This is
//! deliberate and relied upon downstream: e.g. `ffmpeg-rewriter` re-muxes by
//! writing samples back in decode order and reconstructs presentation order
//! itself from the per-sample composition offsets, and feeding an H264 decoder
//! also requires decode order. The concrete invariant is that frames come out
//! in a strict, gap-free decode sequence -- `frame.idx()` is `0, 1, 2, ... N-1`
//! in the order yielded. For B-frame streams the presentation timestamps (PTS)
//! are *not* monotonic in this order (that is the whole point of B-frames).
//!
//! `presentation_order_iter()` yields frames in **presentation (display)
//! order**, with monotonically increasing timestamps, recovered from the
//! container PTS or the bitstream picture order count (POC).
//!
//! ## Test fixtures
//!
//! The fixtures in `tests/data/` are short clips encoded with libx264 using
//! B-frames (so decode order differs from presentation order). They are tiny
//! (a few KB) and committed to the repo, so the tests need neither `ffmpeg` nor
//! network access. They were generated with:
//!
//! ```text
//! ffmpeg -y -v error -f lavfi -i testsrc=size=32x32:rate=10:duration=2 \
//!     -c:v libx264 -bf 2 -g 10 -pix_fmt yuv420p \
//!     -x264-params bframes=2:b-pyramid=normal tests/data/bframes.mp4
//! ffmpeg <same args> -f h264 tests/data/bframes.h264
//! ```

use eyre::Result;

use frame_source::{FrameData, Timestamp, TimestampSource};

const MP4_FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/bframes.mp4");
const H264_FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/bframes.h264");

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

/// Assert the decode indices of `frames` are a permutation of `0..N`, and (since
/// the fixtures use B-frames) that this permutation is not the identity -- i.e.
/// reordering genuinely happened.
fn assert_reordered_permutation(frames: &[FrameData]) {
    let decode_indices: Vec<usize> = frames.iter().map(|f| f.idx()).collect();
    let mut sorted = decode_indices.clone();
    sorted.sort_unstable();
    let expected: Vec<usize> = (0..frames.len()).collect();
    assert_eq!(sorted, expected, "frames must be a permutation of 0..N");
    assert_ne!(
        decode_indices, expected,
        "expected B-frame reordering, but presentation order equals decode order"
    );
}

#[test]
fn test_mp4_with_bframes_iterates_in_decode_order() -> Result<()> {
    let mut src = frame_source::FrameSourceBuilder::new(MP4_FIXTURE)
        // We only care about frame ordering, not the decoded pixels.
        .do_decode_h264(false)
        .timestamp_source(TimestampSource::Mp4Pts)
        .build_source()?;

    let frames: Vec<FrameData> = src.decode_order_iter().collect::<Result<_, _>>()?;

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
fn test_mp4_with_bframes_presentation_order_is_monotonic() -> Result<()> {
    let mut src = frame_source::FrameSourceBuilder::new(MP4_FIXTURE)
        .do_decode_h264(false)
        .timestamp_source(TimestampSource::Mp4Pts)
        .build_source()?;

    let frames: Vec<FrameData> = src.presentation_order_iter()?.collect::<Result<_, _>>()?;

    assert!(
        frames.len() > 2,
        "expected several frames, got {}",
        frames.len()
    );

    // Presentation order: the timestamps must be strictly increasing as yielded.
    let pts: Vec<std::time::Duration> = frames
        .iter()
        .map(|f| match f.timestamp() {
            Timestamp::Duration(d) => Ok(d),
            Timestamp::Fraction(_) => eyre::bail!("expected duration timestamps for an MP4 source"),
        })
        .collect::<Result<_>>()?;
    for w in pts.windows(2) {
        assert!(
            w[1] > w[0],
            "presentation-order timestamps must strictly increase, got {:?} then {:?}",
            w[0],
            w[1]
        );
    }

    // Every source frame appears exactly once, in a non-identity permutation.
    assert_reordered_permutation(&frames);

    Ok(())
}

#[test]
fn test_raw_h264_annexb_with_bframes_iterates_in_decode_order() -> Result<()> {
    let mut src = frame_source::FrameSourceBuilder::new(H264_FIXTURE)
        .do_decode_h264(false)
        .build_source()?;

    let frames: Vec<FrameData> = src.decode_order_iter().collect::<Result<_, _>>()?;

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

#[test]
fn test_raw_h264_annexb_with_bframes_presentation_order_uses_poc() -> Result<()> {
    let mut src = frame_source::FrameSourceBuilder::new(H264_FIXTURE)
        .do_decode_h264(false)
        .build_source()?;

    // A raw Annex B stream carries no per-frame timestamps, so presentation
    // order must be recovered from the bitstream POC. The restamped
    // fraction-done "timestamp" must then increase monotonically, and the decode
    // indices must be a non-identity permutation of 0..N.
    let frames: Vec<FrameData> = src.presentation_order_iter()?.collect::<Result<_, _>>()?;
    assert!(frames.len() > 2, "expected several frames");

    let mut prev = -1.0f32;
    for frame in &frames {
        match frame.timestamp() {
            Timestamp::Fraction(f) => {
                assert!(
                    f > prev,
                    "fraction-done must increase in presentation order, got {f} after {prev}"
                );
                prev = f;
            }
            Timestamp::Duration(_) => eyre::bail!("expected fraction timestamps for raw Annex B"),
        }
    }

    assert_reordered_permutation(&frames);

    Ok(())
}
