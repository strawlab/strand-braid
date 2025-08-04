use eyre::Result;
use frame_source::{h264_source::SeekableH264Source, FrameDataSource};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if !(args.len() == 2 || args.len() == 3) {
        eyre::bail!("Usage: {} <h264-in-mp4-path> [<srt-path>]", args[0]);
    }
    let h264_in_mp4_path = &args[1];

    let builder = frame_source::FrameSourceBuilder::new(&h264_in_mp4_path).do_decode_h264(false);

    let builder = if args.len() == 3 {
        println!("Using SRT file: {}", args[2]);
        builder
            .timestamp_source(frame_source::TimestampSource::SrtFile)
            .srt_file_path(Some(args[2].as_str().into()))
    } else {
        builder
    };

    let mut frame_src = builder.build_h264_in_mp4_source()?;

    let h264_src = frame_src.as_seekable_h264_source();
    let _sps = h264_src.first_sps();
    let _pps = h264_src.first_pps();

    let _width = frame_src.width();
    let _height = frame_src.height();

    let mut count = 0;
    for frame in frame_src.iter() {
        let frame = frame?;
        let _data = match frame.image() {
            frame_source::ImageData::EncodedH264(data) => &data.data,
            _ => {
                eyre::bail!("source is not H264 encoded");
            }
        };
        count += 1;
    }
    println!("{h264_in_mp4_path} has {count} frames");
    Ok(())
}
