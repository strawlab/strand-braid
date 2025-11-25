use ads_apriltag as apriltag;
use machine_vision_formats::pixel_format::Mono8;

#[test]
fn test_detect_standard_41h12() {
    let mut td = apriltag::Detector::new();
    let tf = apriltag::Family::new_tag_standard_41h12();
    td.add_family(tf);

    let raw_td = td.as_mut();
    // raw_td.debug = 1;
    raw_td.quad_decimate = 2.0;
    raw_td.quad_sigma = 0.0;
    raw_td.refine_edges = true;
    raw_td.decode_sharpening = 0.25;

    let file_buf = include_bytes!("frame1.jpg");
    let image = image::load_from_memory(file_buf).unwrap();
    let rgb = convert_image::image_to_rgb8(image).unwrap();

    let dest = convert_image::convert_ref::<_, Mono8>(&rgb).unwrap();
    let im = apriltag::ImageU8Borrowed::view(&dest);
    let detections = td.detect(apriltag::ImageU8::inner(&im));

    println!("got {} detection(s):", detections.len());
    assert!(detections.len() == 1);
    for det in detections.as_slice().iter() {
        {
            println!(
                "  {{id: {}, center: {:?}, family: {:?}}}",
                det.id(),
                det.center(),
                det.family_type()
            );
            assert!(det.id() == 123);
            assert!((det.center()[0] - 273.0).abs() < 0.5);
            assert!((det.center()[1] - 111.0).abs() < 0.5);
        }
    }
}

#[test]
fn test_detect_standard_36h11() {
    let mut td = apriltag::Detector::new();
    let tf = apriltag::Family::new_tag_36h11();
    td.add_family(tf);

    let raw_td = td.as_mut();
    raw_td.quad_decimate = 2.0;
    raw_td.quad_sigma = 0.0;
    raw_td.refine_edges = true;
    raw_td.decode_sharpening = 0.25;

    let file_buf = include_bytes!("frame1.jpg");
    let image = image::load_from_memory(file_buf).unwrap();
    let rgb = convert_image::image_to_rgb8(image).unwrap();

    let dest = convert_image::convert_ref::<_, Mono8>(&rgb).unwrap();
    let im = apriltag::ImageU8Borrowed::view(&dest);
    let detections = td.detect(apriltag::ImageU8::inner(&im));
    assert!(detections.is_empty());
}
