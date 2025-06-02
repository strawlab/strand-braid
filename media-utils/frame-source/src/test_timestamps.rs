use chrono::{DateTime, Duration, Utc};
use machine_vision_formats::pixel_format::RGB8;

use crate::{h264_source::SeekRead, FrameDataSource, Result};
use strand_cam_remote_control::Mp4RecordingConfig;

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
        let mut my_mp4_writer =
            mp4_writer::Mp4Writer::new(std::io::Cursor::new(&mut mp4_buf), cfg, None).unwrap();

        const STRIDE: usize = W as usize * 3;
        let image_data = vec![0u8; STRIDE * H as usize];

        let frame =
            machine_vision_formats::owned::OImage::<RGB8>::new(W, H, STRIDE, image_data).unwrap();

        for fno in 0..=1000 {
            let pts = Duration::try_milliseconds(fno * dt_msec).unwrap();
            let ts = start + pts;
            ptss.push(pts.to_std().unwrap());
            my_mp4_writer.write(&frame, ts).unwrap();
        }
        my_mp4_writer.finish().unwrap();
    }

    let size = mp4_buf.len() as u64;
    let rdr = std::io::Cursor::new(mp4_buf);

    let buf_reader: Box<(dyn SeekRead + Send)> = Box::new(std::io::BufReader::new(rdr));
    let mp4_reader = mp4::Mp4Reader::read_header(buf_reader, size)?;

    let do_decode_h264 = false; // no need to decode h264 to get timestamps.
    let mut src = crate::mp4_source::from_reader_with_timestamp_source(
        mp4_reader,
        do_decode_h264,
        crate::TimestampSource::BestGuess,
        None,
        false,
        None,
    )?;

    assert_eq!(src.width(), W);
    assert_eq!(src.height(), H);
    assert_eq!(src.frame0_time().unwrap(), start);

    for (frame, expected_pts) in src.iter().zip(ptss.iter()) {
        let frame = frame?;
        match frame.timestamp() {
            crate::Timestamp::Duration(actual_pts) => {
                assert_eq!(&actual_pts, expected_pts);
            }
            _ => {
                panic!("expected duration");
            }
        }
    }

    Ok(())
}
