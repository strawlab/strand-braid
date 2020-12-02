use gstreamer as gst;
use gstreamer::prelude::*;

const FNAME: &str = "movie-standard41h12.mkv";
const URL_BASE: &str = "https://strawlab-cdn.com/assets";
const SHA256SUM: &str = "ddd2932d74139cd6ab5500b40c5f0482d5036df2f766be3a5f28ae2345e23aed";

fn init() {
    use std::sync::Once;
    static INIT: Once = Once::new();

    INIT.call_once(|| {
        gst::init().unwrap();
        gstrsapriltag::plugin_register_static().expect("gstrsapriltag tests");
    });
}

#[test]
fn test_create() {
    init();
    assert!(gst::ElementFactory::make("apriltagdetector", None).is_ok());
}

#[test]
fn test_runs() {
    download_verify::download_verify(
        format!("{}/{}", URL_BASE, FNAME).as_str(),
        FNAME,
        &download_verify::Hash::Sha256(SHA256SUM.into()),
    )
    .unwrap();

    init();

    let pipeline = gst::Pipeline::new(None);
    let filesrc = gst::ElementFactory::make("filesrc", None).unwrap();
    filesrc.set_property_from_str("location", FNAME);

    let decodebin = gst::ElementFactory::make("decodebin", None).unwrap();

    pipeline.add_many(&[&filesrc, &decodebin]).unwrap();
    gst::Element::link_many(&[&filesrc, &decodebin]).unwrap();

    let pipeline_weak = pipeline.downgrade();

    // see https://github.com/snapview/gstreamer-rs/blob/master/examples/src/bin/decodebin.rs

    decodebin.connect_pad_added(move |_dbin, src_pad| {
        let pipeline = match pipeline_weak.upgrade() {
            Some(pipeline) => pipeline,
            None => return,
        };

        let videoconvert = gst::ElementFactory::make("videoconvert", None).unwrap();

        let apriltagdetector = gst::ElementFactory::make("apriltagdetector", None).unwrap();
        apriltagdetector.set_property_from_str("family", "standard-41h12");

        let filesink = gst::ElementFactory::make("filesink", None).unwrap();
        // TODO: save data to something we then double check for correctness.

        let elements = &[&videoconvert, &apriltagdetector, &filesink];
        pipeline.add_many(elements).unwrap();
        gst::Element::link_many(elements).unwrap();

        // According to https://github.com/snapview/gstreamer-rs/blob/master/examples/src/bin/decodebin.rs
        // This should be done, but it fails for me:
        // for e in elements {
        //     e.sync_state_with_parent().unwrap();
        // }

        let sink_pad = videoconvert
            .get_static_pad("sink")
            .expect("videoconvert has no sinkpad");
        src_pad.link(&sink_pad).unwrap();
    });

    let bus = pipeline.get_bus().unwrap();

    pipeline.set_state(gst::State::Playing).unwrap();

    for msg in bus.iter_timed(gst::CLOCK_TIME_NONE) {
        use gst::MessageView;

        match msg.view() {
            MessageView::Eos(..) => break,
            MessageView::Error(err) => {
                println!(
                    "Error from {:?}: {} ({:?})",
                    err.get_src().map(|s| s.get_path_string()),
                    err.get_error(),
                    err.get_debug()
                );
                break;
            }
            _ => (),
        }
    }

    pipeline.set_state(gst::State::Null).unwrap();
}
