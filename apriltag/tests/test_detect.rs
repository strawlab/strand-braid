use machine_vision_formats::ImageData;
use ads_apriltag as apriltag;

#[test]
fn test_detect_standard_41h12() {
    let mut td = apriltag::Detector::new();
    let tf = apriltag::Family::new_tag_standard_41h12();
    td.add_family(tf);

    let mut raw_td = td.as_mut();
    // raw_td.debug = 1;
    raw_td.quad_decimate = 2.0;
    raw_td.quad_sigma = 0.0;
    raw_td.refine_edges = 1;
    raw_td.decode_sharpening = 0.25;

    let file_buf = include_bytes!("frame1.jpg");
    let image = image::load_from_memory(file_buf).unwrap();
    let rgb = convert_image::piston_to_frame(image).unwrap();

    let width = rgb.width() as usize;
    let height = rgb.height() as usize;
    let stride = rgb.width() as usize;
    let mut im_data = vec![0; height*stride];
    convert_image::encode_into_gray8(&rgb,&mut im_data[..],stride).unwrap();

    let im = apriltag::ImageU8Owned::new(width as i32, height as i32, stride as i32, im_data);
    let detections = td.detect(apriltag::ImageU8::inner(&im));

    println!("got {} detection(s):", detections.len());
    assert!(detections.len()==1);
    for det in detections.as_slice().iter() {
        {
            println!("  {{id: {}, center: {:?}, family: {:?}}}", det.id(), det.center(), det.family_type());
            assert!(det.id() == 123);
        }
    }
}

#[test]
fn test_detect_standard_36h11() {
    let mut td = apriltag::Detector::new();
    let tf = apriltag::Family::new_tag_36h11();
    td.add_family(tf);

    let mut raw_td = td.as_mut();
    raw_td.quad_decimate = 2.0;
    raw_td.quad_sigma = 0.0;
    raw_td.refine_edges = 1;
    raw_td.decode_sharpening = 0.25;

    let file_buf = include_bytes!("frame1.jpg");
    let image = image::load_from_memory(file_buf).unwrap();
    let rgb = convert_image::piston_to_frame(image).unwrap();

    let width = rgb.width() as usize;
    let height = rgb.height() as usize;
    let stride = rgb.width() as usize;
    let mut im_data = vec![0; height*stride];
    convert_image::encode_into_gray8(&rgb,&mut im_data[..],stride).unwrap();

    let im = apriltag::ImageU8Owned::new(width as i32, height as i32, stride as i32, im_data);
    let detections = td.detect(apriltag::ImageU8::inner(&im));
    assert!(detections.len()==0);
}
