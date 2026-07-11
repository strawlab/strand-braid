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
//! `GOLDEN_DISPLAY_FNV` holds an FNV-1a hash of each decoded frame's RGB8
//! pixels, in display order. To regenerate (after changing fixtures or if the
//! openh264 crate's YUV→RGB conversion changes), run:
//!
//! ```text
//! cargo test -p frame-source --features openh264 --test test_bframe_decode \
//!     print_golden_display_hashes -- --ignored --nocapture
//! ```
//!
//! The values were originally validated against ffmpeg 7.1: the Y-planes of
//! openh264's output pictures are bit-identical to ffmpeg's decode of the same
//! fixture in display order (H.264 decoding is exactly specified), so these
//! hashes pin both pixel content and frame order to an independent decoder.
#![cfg(feature = "openh264")]

use eyre::Result;

use frame_source::{FrameData, ImageData, Timestamp, TimestampSource};
use machine_vision_formats::{ImageData as _, pixel_format::RGB8};

const MP4_FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/bframes.mp4");
const H264_FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/bframes.h264");

/// FNV-1a hash of each decoded frame's RGB8 pixel data, in display order.
/// See the module docs for how to regenerate and how these were validated.
const GOLDEN_DISPLAY_FNV: [u64; 20] = [
    0x1d415ec12859ceb9,
    0xc6db4339d7555d65,
    0xcfccc6f895bb9b6f,
    0x62c5ada562ddd560,
    0xf86ab932fb81b068,
    0x841c640b611acee5,
    0xd608e133f83bf9f0,
    0x0d27e5eac5ac7996,
    0x61ad189b56b2825c,
    0x9f4add443c7c5330,
    0x30b21d79ff5b0d07,
    0x0a909a0a40cfcb12,
    0x99819ddb93cfa04c,
    0x7f80f8edd2f88cb5,
    0x05b1deb35d90fa17,
    0xe6416e5aad53e220,
    0x0697c69dcd393332,
    0xba4cb3a3de176916,
    0x61340ff417e311a4,
    0xa52aa5bb7ae81368,
];

fn fnv1a64(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in data {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// The FNV-1a hash of a decoded frame's RGB8 pixel data.
fn rgb_fnv(frame: &FrameData) -> Result<u64> {
    let ImageData::Decoded(decoded) = frame.image() else {
        eyre::bail!("expected decoded image data, got {:?}", frame.image());
    };
    let frame_view = decoded.borrow();
    let rgb = frame_view.into_pixel_format::<RGB8>()?;
    Ok(fnv1a64(rgb.image_data()))
}

/// Assert `frames` is exactly the golden display-order frame sequence.
fn assert_pixels_are_golden_display_order(frames: &[FrameData]) -> Result<()> {
    assert_eq!(frames.len(), GOLDEN_DISPLAY_FNV.len());
    for (display_rank, frame) in frames.iter().enumerate() {
        assert_eq!(
            rgb_fnv(frame)?,
            GOLDEN_DISPLAY_FNV[display_rank],
            "frame emitted at display position {display_rank} (idx {}) has wrong pixels",
            frame.idx()
        );
    }
    Ok(())
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
    assert_eq!(frames.len(), GOLDEN_DISPLAY_FNV.len());

    let pts: Vec<std::time::Duration> = frames
        .iter()
        .map(|f| f.timestamp().unwrap_duration())
        .collect();

    // Strict, gap-free decode order (the decode_order_iter contract).
    for (position, frame) in frames.iter().enumerate() {
        assert_eq!(frame.idx(), position);
    }

    // Display rank of each decode-order frame, derived from PTS alone.
    let mut order: Vec<usize> = (0..frames.len()).collect();
    order.sort_by_key(|&i| pts[i]);
    let mut rank = vec![0usize; frames.len()];
    for (display_rank, &decode_idx) in order.iter().enumerate() {
        rank[decode_idx] = display_rank;
    }
    // The fixture must actually reorder, or this test is vacuous.
    assert_ne!(rank, (0..frames.len()).collect::<Vec<_>>());

    for (decode_idx, frame) in frames.iter().enumerate() {
        assert_eq!(
            rgb_fnv(frame)?,
            GOLDEN_DISPLAY_FNV[rank[decode_idx]],
            "decode-order frame {decode_idx} has the pixels of a different frame"
        );
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
    assert_eq!(frames.len(), GOLDEN_DISPLAY_FNV.len());

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
    let mut order: Vec<usize> = (0..frames.len()).collect();
    order.sort_by_key(|&i| keys[i]);
    let mut rank = vec![0usize; frames.len()];
    for (display_rank, &decode_idx) in order.iter().enumerate() {
        rank[decode_idx] = display_rank;
    }
    assert_ne!(rank, (0..frames.len()).collect::<Vec<_>>());

    for (decode_idx, frame) in frames.iter().enumerate() {
        assert_eq!(
            rgb_fnv(frame)?,
            GOLDEN_DISPLAY_FNV[rank[decode_idx]],
            "decode-order frame {decode_idx} has the pixels of a different frame"
        );
    }
    Ok(())
}

/// Regenerate `GOLDEN_DISPLAY_FNV` (see module docs). Also cross-checks that
/// the MP4 and raw Annex B fixtures decode to identical pixel sequences.
#[test]
#[ignore]
fn print_golden_display_hashes() -> Result<()> {
    let mut hashes_by_fixture = Vec::new();
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
        let hashes: Vec<u64> = frames.iter().map(rgb_fnv).collect::<Result<_>>()?;
        hashes_by_fixture.push(hashes);
    }
    assert_eq!(
        hashes_by_fixture[0], hashes_by_fixture[1],
        "MP4 and Annex B fixtures should hold the same encoded stream"
    );
    println!(
        "const GOLDEN_DISPLAY_FNV: [u64; {}] = [",
        hashes_by_fixture[0].len()
    );
    for h in &hashes_by_fixture[0] {
        println!("    0x{h:016x},");
    }
    println!("];");
    Ok(())
}
