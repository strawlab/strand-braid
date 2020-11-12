#[cfg(feature="opencv")]
#[test]
fn integration_checkerboards() {
    use freemovr_calibration::pinhole_wizard_yaml_support::PinholeCalib;
    let buf = include_str!("data/pinhole_wizard_sample.yaml");
    let rdr = std::io::Cursor::new(buf.as_bytes());

    let data = freemovr_calibration::parse_pinhole_yaml(rdr, "data").unwrap();
    let (width, height) = (data.loaded.width(), data.loaded.width());
    let display_cam = freemovr_calibration::intrinsics_from_checkerboards(
        data.loaded.checkerboards().unwrap(), width, height).unwrap();

    let p = &display_cam.p;
    let fx = p[(0,0)];
    let fy = p[(1,1)];
    let cx = p[(0,2)];
    let cy = p[(1,2)];
    approx::assert_relative_eq!(fx, fy, epsilon = 1e-0);
    approx::assert_relative_eq!(fx, 741.6, epsilon = 1e-0);
    approx::assert_relative_eq!(cx, 523.2, epsilon = 1e-0);
    approx::assert_relative_eq!(cy, 888.4, epsilon = 1e-0);

    let d = &display_cam.distortion;
    approx::assert_relative_eq!(d.radial1(),     -0.05988580518773874, epsilon = 1e-3);
    approx::assert_relative_eq!(d.radial2(),      0.005990801529784802, epsilon = 1e-3);
    approx::assert_relative_eq!(d.tangential1(), -0.002057300001454975, epsilon = 1e-3);
    approx::assert_relative_eq!(d.tangential2(),  0.00035685420838146533, epsilon = 1e-3);
    approx::assert_relative_eq!(d.radial3(),      0.0, epsilon = 1e-3);
}

#[test]
fn integration_linear_simple() {
    let buf = include_str!("data/pinhole_wizard_simple.yaml");
    let rdr = std::io::Cursor::new(buf.as_bytes());

    let src_data = freemovr_calibration::ActualFiles::new(rdr, ".", 1e-10).unwrap();
    let _float_image = freemovr_calibration::fit_pinholes_compute_cal_image(&src_data, false, false).unwrap();
    // TODO: check generated EXR
}

#[test]
fn integration_linear_sample() {
    let buf = include_str!("data/pinhole_wizard_sample.yaml");
    let rdr = std::io::Cursor::new(buf.as_bytes());
    let src_data = freemovr_calibration::ActualFiles::new(rdr, ".", 1e-10).unwrap();
    let _float_image = freemovr_calibration::fit_pinholes_compute_cal_image(&src_data, false, false).unwrap();
    // TODO: check generated EXR
}

/*
#[test]
fn integration_linear_from_file() {
    let buf = include_str!("pinhole_wizard_geom_from_file.yaml");
    let rdr = std::io::Cursor::new(buf.as_bytes());

    let src_data = freemovr_calibration::ActualFiles::new(rdr, "tests", 1e-10).unwrap();
    let _float_image = freemovr_calibration::fit_pinholes_compute_cal_image(&src_data, false, false).unwrap();
    // TODO: check generated EXR
}
*/

#[test]
fn load_obj() {
    let buf = include_bytes!("data/cylinder.obj");
    let objects = simple_obj_parse::obj_parse(buf).unwrap();
    assert_eq!(objects.len(), 1);
    // TODO: more tests
}

#[test]
fn test_chessboard_corner_finding() {
    use image::GenericImageView;

    let buf: &[u8] = include_bytes!("data/left01.jpg");
    let img = image::load_from_memory(buf).unwrap();

    let (w,h) = img.dimensions();
    let rgb = img.to_rgb().into_raw();

    let corners = opencv_calibrate::find_chessboard_corners(&rgb, w, h, 9, 6 ).unwrap().unwrap();
    assert_eq!(corners.len(),54);


    let buf: &[u8] = include_bytes!("data/blank.png");
    let img = image::load_from_memory(buf).unwrap();

    let (w,h) = img.dimensions();
    let rgb = img.to_rgb().into_raw();

    let corners = opencv_calibrate::find_chessboard_corners(&rgb, w, h, 9, 6 ).unwrap();
    assert!(corners.is_none());

}

// TODO: test yaml file which specifies display geometry in .obj file
