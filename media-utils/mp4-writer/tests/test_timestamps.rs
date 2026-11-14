// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

use chrono::{DateTime, Duration, Utc};
use machine_vision_formats::pixel_format::RGB8;

use frame_source::h264_source::SeekRead;
use frame_source::{FrameDataSource, Result};
use strand_cam_remote_control::Mp4RecordingConfig;
use strand_dynamic_frame::DynamicFrameOwned;

#[test]
fn test_h264_precision_timestamps() -> Result<()> {
    let start: DateTime<Utc> = DateTime::from_timestamp(60 * 60, 0).unwrap();

    let dt_msec = 5;

    let cfg = Mp4RecordingConfig {
        codec: strand_cam_remote_control::Mp4Codec::H264LessAvc,
        max_framerate: Default::default(),
        h264_metadata: None,
    };

    const W: u32 = 32;
    const H: u32 = 16;

    let mut mp4_buf = Vec::new();
    let mut ptss = Vec::new();
    {
        let mut my_mp4_writer = mp4_writer::Mp4Writer::new(
            std::io::Cursor::new(&mut mp4_buf),
            cfg,
            #[cfg(feature = "nv-encode")]
            None,
        )
        .unwrap();

        const STRIDE: usize = W as usize * 3;
        let image_data = vec![0u8; STRIDE * H as usize];

        let frame = DynamicFrameOwned::from_static(
            machine_vision_formats::owned::OImage::<RGB8>::new(W, H, STRIDE, image_data).unwrap(),
        );

        for fno in 0..=1000 {
            let pts = Duration::try_milliseconds(fno * dt_msec).unwrap();
            let ts = start + pts;
            ptss.push(pts.to_std().unwrap());
            my_mp4_writer.write_dynamic(&frame.borrow(), ts).unwrap();
        }
        my_mp4_writer.finish().unwrap();
    }

    let size = mp4_buf.len() as u64;
    let rdr = std::io::Cursor::new(mp4_buf);

    let buf_reader: Box<dyn SeekRead + Send> = Box::new(std::io::BufReader::new(rdr));
    let mp4_reader = mp4::Mp4Reader::read_header(buf_reader, size)?;

    let do_decode_h264 = false; // no need to decode h264 to get timestamps.
    let mut src = frame_source::mp4_source::from_reader_with_timestamp_source(
        mp4_reader,
        do_decode_h264,
        frame_source::TimestampSource::BestGuess,
        None,
        false,
        None,
    )?;

    assert_eq!(src.width(), W);
    assert_eq!(src.height(), H);
    assert_eq!(src.frame0_time().unwrap(), start);

    for (frame, expected_pts) in src.decode_order_iter().zip(ptss.iter()) {
        let frame = frame?;
        match frame.timestamp() {
            frame_source::Timestamp::Duration(actual_pts) => {
                assert_eq!(&actual_pts, expected_pts);
            }
            _ => {
                panic!("expected duration");
            }
        }
    }

    Ok(())
}

/// The container must present frames at the cadence they were captured at.
///
/// [`test_h264_precision_timestamps`] above checks the per-frame MISP
/// precision-timestamp SEI, which is a different thing: it is read straight out
/// of the bitstream and is correct regardless of what the container says. This
/// test reads the container's own timing (`stts` durations, `ctts` offsets) --
/// what a player actually schedules against.
///
/// Currently FAILS, for the reason recorded in the `FIXME` in
/// `Mp4Writer::avcc_sample`: on this path (no source timing to pass through)
/// each sample's duration is derived as the delta from the *previous*
/// presentation time rather than to the *next* one. So sample 0 gets duration 0
/// and every later sample gets the interval that preceded it: the first two
/// samples share PTS 0, every frame is presented one interval early, and the
/// track ends up one interval short.
///
/// Fixing it needs real one-frame lookahead in the writer -- hold each sample
/// until the next arrives, and flush at `finish()`. `last_sample` looks like
/// such a buffer but is drained on every frame, so today the next timestamp is
/// not available when the duration has to be chosen. The final frame's duration
/// has no successor to measure against and must be extrapolated.
#[test]
fn test_container_cadence_matches_capture_cadence() -> Result<()> {
    use frame_source::h264_source::Mp4SampleTiming;

    let start: DateTime<Utc> = DateTime::from_timestamp(60 * 60, 0).unwrap();
    const N_FRAMES: i64 = 20;
    const INTERVAL_US: i64 = 40_000; // 25 fps
    /// Two terms, because the two legitimate error sources differ in character:
    /// requantizing into the writer's 90 kHz movie timescale costs a fixed few
    /// ticks per interval whatever the cadence, while any estimator residual
    /// scales with the interval. One absolute number would be too loose at high
    /// frame rates and unmeetable at low ones. (Same rule as
    /// `ffmpeg-rewriter`'s `interval_tolerance_us`, which documents where the
    /// margin runs out.)
    const TOLERANCE_US: i64 = if INTERVAL_US / 200 > 48 {
        INTERVAL_US / 200
    } else {
        48
    };

    let cfg = Mp4RecordingConfig {
        codec: strand_cam_remote_control::Mp4Codec::H264LessAvc,
        max_framerate: Default::default(),
        h264_metadata: None,
    };

    const W: u32 = 32;
    const H: u32 = 16;
    const STRIDE: usize = W as usize * 3;

    let mut mp4_buf = Vec::new();
    {
        let mut wtr = mp4_writer::Mp4Writer::new(
            std::io::Cursor::new(&mut mp4_buf),
            cfg,
            #[cfg(feature = "nv-encode")]
            None,
        )
        .unwrap();
        let frame = DynamicFrameOwned::from_static(
            machine_vision_formats::owned::OImage::<RGB8>::new(
                W,
                H,
                STRIDE,
                vec![0u8; STRIDE * H as usize],
            )
            .unwrap(),
        );
        for fno in 0..N_FRAMES {
            let ts = start + Duration::microseconds(fno * INTERVAL_US);
            wtr.write_dynamic(&frame.borrow(), ts).unwrap();
        }
        wtr.finish().unwrap();
    }

    let size = mp4_buf.len() as u64;
    let buf_reader: Box<dyn SeekRead + Send> =
        Box::new(std::io::BufReader::new(std::io::Cursor::new(mp4_buf)));
    let mp4_reader = mp4::Mp4Reader::read_header(buf_reader, size)?;
    let src = frame_source::mp4_source::from_reader_with_timestamp_source(
        mp4_reader,
        false, // no need to decode h264 to get timing
        frame_source::TimestampSource::BestGuess,
        None, // no SRT: supplying one makes H264Source rescale the timing under test
        false,
        None,
    )?;

    let timing: Vec<Mp4SampleTiming> = src
        .mp4_sample_timing()
        .expect("MP4 source must expose per-sample timing")
        .to_vec();
    assert_eq!(timing.len(), N_FRAMES as usize);

    // pts = dts + ctts, where dts is the running sum of the durations of the
    // samples *before* this one.
    let mut dts = Duration::zero();
    let mut pts = Vec::with_capacity(timing.len());
    for t in timing.iter() {
        pts.push(dts + t.composition_offset);
        dts += Duration::from_std(t.decode_duration).unwrap();
    }

    assert_eq!(
        pts[0].num_microseconds().unwrap(),
        0,
        "the first frame must be presented at the start of the track"
    );
    for (i, [earlier, later]) in pts.array_windows().enumerate() {
        let interval = (*later - *earlier).num_microseconds().unwrap();
        assert!(
            (interval - INTERVAL_US).abs() <= TOLERANCE_US,
            "presentation interval {i} is {interval} us, expected {INTERVAL_US} us \
             (tolerance {TOLERANCE_US} us)"
        );
    }

    let span = (*pts.last().unwrap() - pts[0]).num_microseconds().unwrap();
    let expected_span = (N_FRAMES - 1) * INTERVAL_US;
    assert!(
        (span - expected_span).abs() <= TOLERANCE_US,
        "presentation span is {span} us, expected {expected_span} us"
    );

    Ok(())
}
