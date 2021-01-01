use gstreamer as gst;

fn init() {
    use std::sync::Once;
    static INIT: Once = Once::new();

    INIT.call_once(|| {
        gst::init().unwrap();
        gstrsnvargustime::plugin_register_static().expect("gstrsnvargustime tests");
    });
}

#[test]
fn test_create() {
    init();
    assert!(gst::ElementFactory::make("nvargustime", None).is_ok());
}
