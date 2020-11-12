use imagesrc::ImageSource;
use futures::stream::StreamExt;
use gstreamer_video as gst_video;

use byte_slice_cast::*;

async fn async_main() -> Result<(),Box<dyn std::error::Error>> {
    let mut args: Vec<String> = std::env::args().into_iter().collect();
    args.remove(0);
    if args.len() == 0 {
        println!("no arguments given. nothing to do. (Hint, try re-running with \
            CLI args 'v4l2src videoconvert' or 'nvarguscamerasrc capsfilter nvvidconv')");
        return Ok(());
    }
    for arg in args.iter() {
        println!("  pipeline element: {}", arg);
    }
    let mut src = imagesrc_gst::GstSink::spawn_gstreamer_mainloop(args,10);
    let mut frame_stream = src.frames()?;
    while let Some(ref sample) = frame_stream.next().await {
        let sample = sample.sample();
        if let Some(ref buffer) = sample.get_buffer() {

            let info = sample
                .get_caps()
                .and_then(|caps| gst_video::VideoInfo::from_caps(caps).ok())
                .expect("failed converting caps");

            // At this point, buffer is only a reference to an existing memory region somewhere.
            // When we want to access its content, we have to map it while requesting the required
            // mode of access (read, read/write).
            // This type of abstraction is necessary, because the buffer in question might not be
            // on the machine's main memory itself, but rather in the GPU's memory.
            // So mapping the buffer makes the underlying memory region accessible to us.
            // See: https://gstreamer.freedesktop.org/documentation/plugin-development/advanced/allocation.html
            let map = buffer.map_readable()?;

            // We know what format the data in the memory region has, since we requested
            // it by setting the appsink's caps. So what we do here is interpret the
            // memory region we mapped as an array of unsigned 8 bit integers.
            let samples = map.as_slice_of::<u8>()?;

            println!("{}x{} {:?} (len={}), pts={:?}", info.width(), info.height(),
                info.format(), samples.len(), buffer.get_pts());
        }
    }
    src.join_handle.join().expect("join").expect("join2");
    Ok(())
}

fn main() -> Result<(),Box<dyn std::error::Error>> {
    futures::executor::block_on(async_main())?;
    Ok(())
}
