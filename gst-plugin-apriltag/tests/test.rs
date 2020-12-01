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

    let videoconvert = gst::ElementFactory::make("videoconvert", None).unwrap();

    let apriltagdetector = gst::ElementFactory::make("apriltagdetector", None).unwrap();
    apriltagdetector.set_property_from_str("family", "standard-41h12");

    let filesink = gst::ElementFactory::make("filesink", None).unwrap();
    // TODO: save data to something we then double check for correctness.

    pipeline
        .add_many(&[
            &filesrc,
            &decodebin,
            &videoconvert,
            &apriltagdetector,
            &filesink,
        ])
        .unwrap();

    pipeline.set_state(gst::State::Playing).unwrap();
    pipeline.set_state(gst::State::Null).unwrap();
}
