use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use machine_vision_formats::pixel_format::RGB8;

use ci2_remote_control::Mp4RecordingConfig;
use frame_source::FrameDataSource;

#[test]
fn test_h264_precision_timestamps() -> anyhow::Result<()> {
    let start: DateTime<Utc> =
        DateTime::from_naive_utc_and_offset(NaiveDateTime::from_timestamp_opt(60 * 60, 0).unwrap(), Utc);

    let dt_msec = 5;

    let cfg = Mp4RecordingConfig {
        codec: ci2_remote_control::Mp4Codec::H264LessAvc,
        max_framerate: Default::default(),
        h264_metadata: None,
    };

    const W: u32 = 32;
    const H: u32 = 16;

    let mut mp4_buf = Vec::new();
    let mut ptss = Vec::new();
    {
        let mut my_mp4_writer =
            mp4_writer::Mp4Writer::new(std::io::Cursor::new(&mut mp4_buf), cfg, None)?;

        const STRIDE: usize = W as usize * 3;
        let image_data = vec![0u8; STRIDE * H as usize];

        let frame =
            simple_frame::SimpleFrame::<RGB8>::new(W, H, STRIDE.try_into().unwrap(), image_data)
                .unwrap();

        for fno in 0..=1000 {
            let pts = Duration::milliseconds(fno * dt_msec);
            let ts = start + pts;
            ptss.push(pts.to_std().unwrap());
            my_mp4_writer.write(&frame, ts)?;
        }
        my_mp4_writer.finish()?;
    }

    let size = mp4_buf.len() as u64;
    let reader = std::io::Cursor::new(mp4_buf);

    let do_decode_h264 = false; // no need to decode h264 to get timestamps.
    let mut src = frame_source::mp4_source::from_reader(reader, do_decode_h264, size)?;
    assert_eq!(src.width(), W);
    assert_eq!(src.height(), H);
    assert_eq!(src.frame0_time().unwrap(), start);

    for (frame, expected_pts) in src.iter().zip(ptss.iter()) {
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
