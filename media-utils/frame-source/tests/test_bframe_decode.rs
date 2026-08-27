// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Decode B-frame streams with the built-in OpenH264 decoder and assert the
//! decoded pixels are paired with the correct frame (index and timestamp).
//!
//! OpenH264's decoder supports B-frames (upstream since v2.2, 2022, see
//! <https://github.com/cisco/openh264/issues/3546>), but with B-frames present
//! it buffers pictures internally to reorder them into display order: a decode
//! call may return no picture, and the buffered pictures must be drained at
//! end of stream. `frame-source` drives the decoder accordingly and pairs each
//! output picture back with its input frame, which these tests verify.
//!
//! Uses the same fixtures as `test_frame_order.rs` (libx264, `bframes=2`,
//! `b-pyramid=normal`, two closed GOPs of 10 frames each; see that file for
//! the exact ffmpeg invocations).
//!
//! ## Golden data
//!
//! `bframes.rgb24` is ffmpeg 7.1's own decode of `bframes.h264`, as packed
//! RGB8 in display order -- an independent decoder's answer for what each
//! frame should look like. Regenerate it (only if the fixtures change) with:
//!
//! ```text
//! ffmpeg -y -v error -i tests/data/bframes.h264 -pix_fmt rgb24 -f rawvideo \
//!     tests/data/bframes.rgb24
//! ```
//!
//! Frames are compared to it per channel within [`MAX_CHANNEL_DIFF`] rather
//! than exactly, because only the *decoding* is bit-exactly specified by the
//! H.264 spec; the YUV->RGB step afterwards is not. openh264 and swscale round
//! it differently, and either may change that rounding across releases.
//!
//! The tolerance is what makes this hold still. Over this fixture, openh264's
//! RGB differs from ffmpeg's by at most 3 per channel for the *same* frame,
//! while the two closest *different* frames differ by at least 28 -- so
//! [`MAX_CHANNEL_DIFF`] sits with roughly 3x margin on either side, tight
//! enough to catch a mispaired or misordered frame and loose enough to ignore
//! conversion rounding.
//!
//! This replaced a table of exact hashes of openh264's RGB output, which had
//! pinned openh264 0.9.3's YUV->RGB green coefficient typo (`0.299/0.687`,
//! fixed to `0.299/0.587` in 0.9.8) as if it were correct: an error of up to
//! 16 per channel against ffmpeg, which the tolerance above would have caught.
#![cfg(feature = "openh264")]

use eyre::Result;

use frame_source::{FrameData, ImageData, Timestamp, TimestampSource};
use machine_vision_formats::{ImageData as _, Stride as _, pixel_format::RGB8};

const MP4_FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/bframes.mp4");
const H264_FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/bframes.h264");

/// ffmpeg 7.1's decode of [`H264_FIXTURE`]: packed RGB8, display order.
/// See the module docs for how to regenerate.
const GOLDEN_RGB24: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/data/bframes.rgb24"
));

/// Largest per-channel difference from ffmpeg's RGB accepted as the same
/// frame. See the module docs for how this value is bounded on both sides.
const MAX_CHANNEL_DIFF: u8 = 8;

const FIXTURE_FRAMES: usize = 20;

/// A decoded frame's pixels as packed RGB8 (stride padding removed).
fn rgb_bytes(frame: &FrameData) -> Result<Vec<u8>> {
    let ImageData::Decoded(decoded) = frame.image() else {
        eyre::bail!("expected decoded image data, got {:?}", frame.image());
    };
    let frame_view = decoded.borrow();
    let rgb = frame_view.into_pixel_format::<RGB8>()?;
    let (width, height, stride) = (rgb.width() as usize, rgb.height() as usize, rgb.stride());
    let data = rgb.image_data();
    let mut packed = Vec::with_capacity(width * height * 3);
    for row in 0..height {
        packed.extend_from_slice(&data[row * stride..row * stride + width * 3]);
    }
    Ok(packed)
}

/// Assert `frame` holds the picture that ffmpeg decoded at `display_rank`.
fn assert_is_display_frame(frame: &FrameData, display_rank: usize, context: &str) -> Result<()> {
    let actual = rgb_bytes(frame)?;
    let frame_len = GOLDEN_RGB24.len() / FIXTURE_FRAMES;
    assert_eq!(
        actual.len(),
        frame_len,
        "{context}: decoded frame size does not match the ffmpeg reference"
    );
    let expected = &GOLDEN_RGB24[display_rank * frame_len..(display_rank + 1) * frame_len];

    // Report the worst channel rather than the first, so a failure says how
    // far off it is: rounding-scale or a different picture entirely.
    let worst = actual
        .iter()
        .zip(expected)
        .enumerate()
        .map(|(i, (&a, &e))| (a.abs_diff(e), i, a, e))
        .max();
    if let Some((diff, i, actual_val, expected_val)) = worst {
        assert!(
            diff <= MAX_CHANNEL_DIFF,
            "{context}: differs from ffmpeg's display-order frame {display_rank} by {diff} \
             (> {MAX_CHANNEL_DIFF}) at byte {i}: got {actual_val}, expected {expected_val}. \
             A difference this large means the wrong picture, not conversion rounding."
        );
    }
    Ok(())
}

/// Assert `frames` is exactly the golden display-order frame sequence.
fn assert_pixels_are_golden_display_order(frames: &[FrameData]) -> Result<()> {
    assert_eq!(frames.len(), FIXTURE_FRAMES);
    for (display_rank, frame) in frames.iter().enumerate() {
        assert_is_display_frame(
            frame,
            display_rank,
            &format!(
                "frame emitted at display position {display_rank} (idx {})",
                frame.idx()
            ),
        )?;
    }
    Ok(())
}

/// The display rank of each frame, given a sort key per frame. Panics unless
/// the ranks are a non-identity permutation, since a fixture that does not
/// reorder would make its test vacuous.
fn display_ranks<K: Ord>(keys: &[K]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..keys.len()).collect();
    order.sort_by_key(|&i| &keys[i]);
    let mut rank = vec![0usize; keys.len()];
    for (display_rank, &decode_idx) in order.iter().enumerate() {
        rank[decode_idx] = display_rank;
    }
    assert_ne!(rank, (0..keys.len()).collect::<Vec<_>>());
    rank
}

/// Decoding an MP4 with B-frames: `decode_order_iter` must yield all frames
/// gap-free in decode order, with each frame's pixels being those of *that*
/// frame (verified via the frame's display rank, computed here independently
/// from the container PTS).
#[test]
fn mp4_bframes_decode_order_pairs_pixels_with_frames() -> Result<()> {
    let mut src = frame_source::FrameSourceBuilder::new(MP4_FIXTURE)
        .do_decode_h264(true)
        .timestamp_source(TimestampSource::Mp4Pts)
        .build_source()?;
    let frames: Vec<FrameData> = src.decode_order_iter().collect::<Result<_, _>>()?;
    assert_eq!(frames.len(), FIXTURE_FRAMES);

    // Strict, gap-free decode order (the decode_order_iter contract).
    for (position, frame) in frames.iter().enumerate() {
        assert_eq!(frame.idx(), position);
    }

    // Display rank of each decode-order frame, derived from PTS alone.
    let pts: Vec<std::time::Duration> = frames
        .iter()
        .map(|f| f.timestamp().unwrap_duration())
        .collect();
    let rank = display_ranks(&pts);

    for (decode_idx, frame) in frames.iter().enumerate() {
        assert_is_display_frame(
            frame,
            rank[decode_idx],
            &format!("decode-order frame {decode_idx}"),
        )?;
    }
    Ok(())
}

/// Decoding an MP4 with B-frames in presentation order: strictly increasing
/// timestamps and pixels in exact display order.
#[test]
fn mp4_bframes_presentation_order_decodes_in_display_order() -> Result<()> {
    let mut src = frame_source::FrameSourceBuilder::new(MP4_FIXTURE)
        .do_decode_h264(true)
        .timestamp_source(TimestampSource::Mp4Pts)
        .build_source()?;
    let frames: Vec<FrameData> = src.presentation_order_iter()?.collect::<Result<_, _>>()?;

    let pts: Vec<std::time::Duration> = frames
        .iter()
        .map(|f| f.timestamp().unwrap_duration())
        .collect();
    for w in pts.windows(2) {
        assert!(
            w[1] > w[0],
            "presentation timestamps must strictly increase"
        );
    }
    // B-frame streams: display order is a non-identity permutation of decode order.
    let decode_indices: Vec<usize> = frames.iter().map(|f| f.idx()).collect();
    assert_ne!(decode_indices, (0..frames.len()).collect::<Vec<_>>());

    assert_pixels_are_golden_display_order(&frames)
}

/// Decoding a raw Annex B stream with B-frames (no container timestamps, so
/// pairing relies on the bitstream POC) in presentation order.
#[test]
fn annexb_bframes_presentation_order_decodes_in_display_order() -> Result<()> {
    let mut src = frame_source::FrameSourceBuilder::new(H264_FIXTURE)
        .do_decode_h264(true)
        .build_source()?;
    let frames: Vec<FrameData> = src.presentation_order_iter()?.collect::<Result<_, _>>()?;

    // Fraction-done timestamps must increase in presentation order.
    let mut prev = -1.0f32;
    for frame in &frames {
        match frame.timestamp() {
            Timestamp::Fraction(f) => {
                assert!(f > prev);
                prev = f;
            }
            Timestamp::Duration(_) => eyre::bail!("expected fraction timestamps for raw Annex B"),
        }
    }
    assert_pixels_are_golden_display_order(&frames)
}

/// Decoding a raw Annex B stream with B-frames in decode order: gap-free
/// decode indices, with each frame's pixels verified via a display rank
/// computed here independently from the frames' POC values (POC resets to 0
/// at each IDR, which starts a new coded video sequence).
#[test]
fn annexb_bframes_decode_order_pairs_pixels_with_frames() -> Result<()> {
    let mut src = frame_source::FrameSourceBuilder::new(H264_FIXTURE)
        .do_decode_h264(true)
        .build_source()?;
    let frames: Vec<FrameData> = src.decode_order_iter().collect::<Result<_, _>>()?;
    assert_eq!(frames.len(), FIXTURE_FRAMES);

    for (position, frame) in frames.iter().enumerate() {
        assert_eq!(frame.idx(), position);
    }

    // Display sort key: (coded video sequence, POC within it). In these
    // fixtures POC is 0 exactly at each IDR.
    let mut cvs = 0i64;
    let mut keys = Vec::with_capacity(frames.len());
    for (i, frame) in frames.iter().enumerate() {
        let poc = frame.poc().expect("fixture frames must carry a POC");
        if poc == 0 && i != 0 {
            cvs += 1;
        }
        keys.push((cvs, poc));
    }
    let rank = display_ranks(&keys);

    for (decode_idx, frame) in frames.iter().enumerate() {
        assert_is_display_frame(
            frame,
            rank[decode_idx],
            &format!("decode-order frame {decode_idx}"),
        )?;
    }
    Ok(())
}

/// The MP4 and raw Annex B fixtures hold the same coded stream, so they must
/// decode to byte-identical pixels (same decoder, same conversion -- no
/// tolerance needed here).
#[test]
fn mp4_and_annexb_fixtures_decode_identically() -> Result<()> {
    let mut pixels_by_fixture = Vec::new();
    for (fixture, ts) in [
        (MP4_FIXTURE, Some(TimestampSource::Mp4Pts)),
        (H264_FIXTURE, None),
    ] {
        let mut builder = frame_source::FrameSourceBuilder::new(fixture).do_decode_h264(true);
        if let Some(ts) = ts {
            builder = builder.timestamp_source(ts);
        }
        let mut src = builder.build_source()?;
        let frames: Vec<FrameData> = src.presentation_order_iter()?.collect::<Result<_, _>>()?;
        pixels_by_fixture.push(frames.iter().map(rgb_bytes).collect::<Result<Vec<_>>>()?);
    }
    assert_eq!(pixels_by_fixture[0].len(), FIXTURE_FRAMES);
    assert_eq!(
        pixels_by_fixture[0], pixels_by_fixture[1],
        "MP4 and Annex B fixtures should hold the same encoded stream"
    );
    Ok(())
}
