use frame_source::FrameDataSource;

#[test]
fn parse_h264() -> color_eyre::Result<()> {
    let file_buf = include_bytes!("data/test_less-avc_mono8_15x14.h264");
    let mut cursor = std::io::Cursor::new(file_buf);
    let do_decode_h264 = true;
    let mut h264_src = frame_source::h264_source::from_annexb_reader(&mut cursor, do_decode_h264)?;
    assert_eq!(h264_src.width(), 15);
    assert_eq!(h264_src.height(), 14);
    let frames: Vec<_> = h264_src.iter().collect();
    assert_eq!(frames.len(), 1);

    let file_buf = include_bytes!("data/test_less-avc_rgb8_16x16.h264");
    let mut cursor = std::io::Cursor::new(file_buf);
    let do_decode_h264 = true;
    let mut h264_src = frame_source::h264_source::from_annexb_reader(&mut cursor, do_decode_h264)?;
    assert_eq!(h264_src.width(), 16);
    assert_eq!(h264_src.height(), 16);
    let frames: Vec<_> = h264_src.iter().collect();
    assert_eq!(frames.len(), 1);
    Ok(())
}
