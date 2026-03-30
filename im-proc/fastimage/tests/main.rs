use std::os::raw as ipp_ctypes;

use fastimage::{
    ripp, CompareOp, FastImage, FastImageData, FastImageView, IppStatusType, IppVersion,
    MomentState, MutableFastImage, MutableFastImageView,
};

#[test]
fn test_simd_absdiff() {
    // const W: usize = 1280;
    // const H: usize = 1024;

    // Choose not divisible-by-256 size to test alignment code.
    const W: usize = 50;
    const H: usize = 11;

    let im10: Vec<u8> = [10; W * H].to_vec();
    let im9: Vec<u8> = [9; W * H].to_vec();
    let mut im_dest: Vec<u8> = [0; W * H].to_vec();

    #[cfg(feature = "simd-avx2")]
    use fastimage::simd_avx2 as simd;

    #[cfg(feature = "simd-sse2")]
    use fastimage::simd_sse2 as simd;

    unsafe { simd::abs_diff_8u_c1r(&im10, &im9, &mut im_dest) };
    for dest_element in im_dest.iter() {
        assert_eq!(*dest_element, 1);
    }

    unsafe { simd::abs_diff_8u_c1r(&im9, &im10, &mut im_dest) };
    for dest_element in im_dest.iter() {
        assert_eq!(*dest_element, 1);
    }

    unsafe { simd::abs_diff_8u_c1r(&im9, &im9, &mut im_dest) };
    for dest_element in im_dest.iter() {
        assert_eq!(*dest_element, 0);
    }
}

#[test]
fn test_version() {
    let _version = IppVersion::new();
}

#[test]
fn test_check_version() {
    let version = IppVersion::new();
    // check that compile-time headers match runtime version
    assert!(ipp_sys::IPP_VERSION_MAJOR as ipp_ctypes::c_int == version.major());
    assert!(ipp_sys::IPP_VERSION_MINOR as ipp_ctypes::c_int == version.minor());
    assert!(ipp_sys::IPP_VERSION_UPDATE as ipp_ctypes::c_int == version.major_build());
}

#[test]
fn test_status_string() {
    // ippStsNormErr has value -229 and bindgen might give it type i32
    let es = fastimage::ipp_status_string(ipp_sys::ippStsNormErr as IppStatusType);
    assert!(es.to_lowercase().contains("norm"));

    // ippStsUnderflow has value 17 and bindgen might give it type u32
    let es = fastimage::ipp_status_string(ipp_sys::ippStsUnderflow as IppStatusType);
    assert!(es.to_lowercase().contains("underflow"));
}

#[test]
fn test_new_u8() {
    ripp::init().unwrap();
    let w = 5;
    let h = 6;
    let im10 = FastImageData::<u8>::new(w, h, 10).unwrap();
    assert!(im10.pixel_slice(4, 3) == &[10]);
}

#[test]
fn test_my_image_f32() {
    ripp::init().unwrap();
    let w = 6;
    let h = 7;
    let mut im10 = FastImageData::<f32>::new(w, h, 10.0).unwrap();
    println!("im10 {:?}", im10);
    assert!(im10.pixel_slice(4, 3) == &[10.0]);

    im10.pixel_slice_mut(6, 5)[0] = 20.0;
    assert!(im10.pixel_slice(6, 5) == &[20.0]);
}

#[test]
fn test_view() {
    ripp::init().unwrap();
    let w = 10;
    let h = 10;
    let mut im10 = FastImageData::<u8>::new(w, h, 0).unwrap();

    // fill array with useful pattern
    for row in 0..h as usize {
        for col in 0..w as usize {
            im10.pixel_slice_mut(row, col)[0] = (row * 10_usize + col) as u8;
        }
    }

    // generate an ROI
    let roi_sz = fastimage::FastImageSize::new(3, 4);
    let roi = fastimage::FastImageRegion::new(fastimage::Point::new(2, 5), roi_sz);

    // check contents of ROI for FastImageView
    {
        let im10_view = fastimage::FastImageView::view_region(&mut im10, &roi).unwrap();
        assert!(im10_view.pixel_slice(0, 0)[0] == 52);
        assert!(im10_view.pixel_slice(0, 1)[0] == 53);
        assert!(im10_view.pixel_slice(0, 2)[0] == 54);
        assert!(im10_view.pixel_slice(3, 0)[0] == 82);
        assert!(im10_view.pixel_slice(3, 1)[0] == 83);
        assert!(im10_view.pixel_slice(3, 2)[0] == 84);
        assert!(im10_view.size() == roi_sz);
    }

    let value = 123;
    let result_im = FastImageData::<u8>::new(3, 4, value).unwrap();

    {
        // check contents of ROI for MutableFastImageView
        let mut im10_view = fastimage::MutableFastImageView::view_region(&mut im10, &roi).unwrap();
        assert!(im10_view.pixel_slice(0, 0)[0] == 52);
        assert!(im10_view.pixel_slice(0, 1)[0] == 53);
        assert!(im10_view.pixel_slice(0, 2)[0] == 54);
        assert!(im10_view.pixel_slice(3, 0)[0] == 82);
        assert!(im10_view.pixel_slice(3, 1)[0] == 83);
        assert!(im10_view.pixel_slice(3, 2)[0] == 84);
        assert!(im10_view.size() == roi_sz);
        // set contents of ROI
        ripp::set_8u_c1r(value, &mut im10_view, roi_sz).unwrap();
        // check contents of ROI after set
        assert!(im10_view.all_equal(&result_im));
    }

    // check contents of ROI after set
    {
        let im10_view = fastimage::FastImageView::view_region(&im10, &roi).unwrap();
        assert!(im10_view.all_equal(&result_im));
    }
}

#[test]
fn test_end_of_roi() {
    let w = 10;
    let h = 10;
    let mut im10 = FastImageData::<u8>::new(w, h, 0).unwrap();

    // fill array with useful pattern
    for row in 0..h as usize {
        for col in 0..w as usize {
            im10.pixel_slice_mut(row, col)[0] = (row * 10_usize + col) as u8;
        }
    }

    // generate an ROI
    let roi_sz = fastimage::FastImageSize::new(3, 4);
    let roi = fastimage::FastImageRegion::new(fastimage::Point::new(7, 6), roi_sz);

    // check contents of ROI for FastImageView
    {
        let im10_view = fastimage::FastImageView::view_region(&mut im10, &roi).unwrap();
        assert!(im10_view.pixel_slice(0, 0)[0] == 67);
        assert!(im10_view.pixel_slice(0, 1)[0] == 68);
        assert!(im10_view.pixel_slice(0, 2)[0] == 69);
        assert!(im10_view.pixel_slice(3, 0)[0] == 97);
        assert!(im10_view.pixel_slice(3, 1)[0] == 98);
        assert!(im10_view.pixel_slice(3, 2)[0] == 99);
        assert!(im10_view.size() == roi_sz);
    }

    let value = 123;
    let result_im = FastImageData::<u8>::new(3, 4, value).unwrap();

    {
        // check contents of ROI for MutableFastImageView
        let mut im10_view = fastimage::MutableFastImageView::view_region(&mut im10, &roi).unwrap();
        assert!(im10_view.pixel_slice(0, 0)[0] == 67);
        assert!(im10_view.pixel_slice(0, 1)[0] == 68);
        assert!(im10_view.pixel_slice(0, 2)[0] == 69);
        assert!(im10_view.pixel_slice(3, 0)[0] == 97);
        assert!(im10_view.pixel_slice(3, 1)[0] == 98);
        assert!(im10_view.pixel_slice(3, 2)[0] == 99);
        assert!(im10_view.size() == roi_sz);
        // set contents of ROI
        ripp::set_8u_c1r(value, &mut im10_view, roi_sz).unwrap();
        // check contents of ROI after set
        assert!(im10_view.all_equal(&result_im));
    }

    // check contents of ROI after set
    {
        let im10_view = fastimage::FastImageView::view_region(&im10, &roi).unwrap();
        assert!(im10_view.all_equal(&result_im));
    }
}

#[test]
fn test_mask() {
    let mut im_dest = FastImageData::<u8>::new(3, 4, 123).unwrap();
    let size = im_dest.size();

    {
        let im123 = FastImageData::<u8>::new(3, 4, 123).unwrap();

        let im0 = FastImageData::<u8>::new(3, 4, 0).unwrap();
        ripp::set_8u_c1mr(22, &mut im_dest, size, &im0).unwrap();
        assert!(im_dest.all_equal(&im123));
    }

    {
        let im1 = FastImageData::<u8>::new(3, 4, 1).unwrap();
        let im22 = FastImageData::<u8>::new(3, 4, 22).unwrap();
        ripp::set_8u_c1mr(22, &mut im_dest, size, &im1).unwrap();
        assert!(im_dest.all_equal(&im22));
    }
}

#[test]
fn test_sub() {
    ripp::init().unwrap();

    let w = 5;
    let h = 6;
    let im10 = FastImageData::<u8>::new(w, h, 10).unwrap();
    let im9 = FastImageData::<u8>::new(w, h, 9).unwrap();
    let im1 = FastImageData::<u8>::new(w, h, 1).unwrap();
    let im0 = FastImageData::<u8>::new(w, h, 0).unwrap();

    let mut im_dest = FastImageData::<u8>::new(w, h, 0).unwrap();

    ripp::init().unwrap();

    let size = im_dest.size();

    println!("im10 {:?}", im10);
    println!("im9 {:?}", im9);

    ripp::sub_8u_c1rsfs(&im9, &im10, &mut im_dest, size, 0).unwrap();
    println!("im_dest {:?}", im_dest);
    println!("im1 {:?}", im1);
    assert!(im_dest.all_equal(&im1));

    ripp::sub_8u_c1rsfs(&im10, &im9, &mut im_dest, size, 0).unwrap();
    println!("im_dest {:?}", im_dest);
    println!("im1 {:?}", im1);
    assert!(im_dest.all_equal(&im0));

    let im9_view = FastImageView::view(&im9);
    let im10_view = FastImageView::view(&im10);
    let mut im_dest_view = MutableFastImageView::view(&mut im_dest);

    ripp::sub_8u_c1rsfs(&im9_view, &im10_view, &mut im_dest_view, size, 0).unwrap();
    println!("im_dest {:?}", im_dest_view);
    println!("im1 {:?}", im1);
    assert!(im_dest_view.all_equal(im1));
}

#[test]
fn test_compare() {
    ripp::init().unwrap();

    let w = 5;
    let h = 6;
    let im10 = FastImageData::<u8>::new(w, h, 10).unwrap();
    let im0 = FastImageData::<u8>::new(w, h, 0).unwrap();
    let im255 = FastImageData::<u8>::new(w, h, 255).unwrap();
    let mut im_dest = FastImageData::<u8>::new(w, h, 99).unwrap();
    let size = im_dest.size();

    {
        println!("im_dest {:?}", im_dest);
        ripp::compare_c_8u_c1r(&im10, 10, &mut im_dest, size, CompareOp::Greater).unwrap();
        println!("im_dest {:?}", im_dest);
        println!("im0 {:?}", im0);
        assert!(im_dest.all_equal(&im0));
    }

    {
        println!("-----");
        println!("im_dest {:?}", im_dest);
        ripp::compare_c_8u_c1r(&im10, 9, &mut im_dest, size, CompareOp::Greater).unwrap();
        println!("im_dest {:?}", im_dest);
        println!("im255 {:?}", im255);
        assert!(im_dest.all_equal(&im255));
    }
}

#[test]
fn test_abs_diff() {
    ripp::init().unwrap();

    let w = 5;
    let h = 6;
    let im10 = FastImageData::<u8>::new(w, h, 10).unwrap();
    let im9 = FastImageData::<u8>::new(w, h, 9).unwrap();
    let im1 = FastImageData::<u8>::new(w, h, 1).unwrap();
    let im0 = FastImageData::<u8>::new(w, h, 0).unwrap();

    let mut im_dest = FastImageData::<u8>::new(w, h, 0).unwrap();

    let size = im_dest.size();

    ripp::abs_diff_8u_c1r(&im10, &im9, &mut im_dest, size).unwrap();
    assert!(im_dest.all_equal(&im1));

    ripp::abs_diff_8u_c1r(&im9, &im10, &mut im_dest, size).unwrap();
    assert!(im_dest.all_equal(&im1));

    ripp::abs_diff_8u_c1r(&im9, &im9, &mut im_dest, size).unwrap();
    assert!(im_dest.all_equal(&im0));

    ripp::abs_diff_8u_c1r(&im10, &im10, &mut im_dest, size).unwrap();
    assert!(im_dest.all_equal(&im0));
}

#[test]
fn test_add_weighted_in_place() {
    ripp::init().unwrap();

    let w = 5;
    let h = 6;
    {
        let mut im_dest = FastImageData::<f32>::new(w, h, 12.0).unwrap();
        let im4 = FastImageData::<u8>::new(w, h, 4).unwrap();

        ripp::add_weighted_8u32f_c1ir(&im4, &mut im_dest, im4.size(), 0.25).unwrap();

        let im10 = FastImageData::<f32>::new(w, h, 10.0).unwrap();
        assert!(im_dest.all_equal(im10));
    }

    {
        let mut im_dest = FastImageData::<f32>::new(w, h, 4.0).unwrap();
        let im0 = FastImageData::<u8>::new(w, h, 0).unwrap();

        ripp::add_weighted_8u32f_c1ir(&im0, &mut im_dest, im0.size(), 0.25).unwrap();

        let im3 = FastImageData::<f32>::new(w, h, 3.0).unwrap();
        assert!(im_dest.all_equal(im3));
    }

    {
        let mut im_dest = FastImageData::<f32>::new(w, h, 0.0).unwrap();
        let im4 = FastImageData::<u8>::new(w, h, 4).unwrap();

        ripp::add_weighted_8u32f_c1ir(&im4, &mut im_dest, im4.size(), 0.25).unwrap();

        let im1 = FastImageData::<f32>::new(w, h, 1.0).unwrap();
        assert!(im_dest.all_equal(im1));
    }

    {
        let mut im_dest = FastImageData::<f32>::new(w, h, 12.0).unwrap();
        let im4 = FastImageData::<f32>::new(w, h, 4.0).unwrap();

        ripp::add_weighted_32f_c1ir(&im4, &mut im_dest, im4.size(), 0.25).unwrap();

        let im10 = FastImageData::<f32>::new(w, h, 10.0).unwrap();
        assert!(im_dest.all_equal(im10));
    }
}

#[test]
fn test_min_max() {
    ripp::init().unwrap();

    let w = 20;
    let h = 20;

    let mut im = FastImageData::<u8>::new(w, h, 10).unwrap();
    im.pixel_slice_mut(4, 3)[0] = 20;
    im.pixel_slice_mut(14, 13)[0] = 9;

    let (min_value, loc) = ripp::min_indx_8u_c1r(&im, im.size()).unwrap();
    assert!(loc.x() == 13);
    assert!(loc.y() == 14);
    assert!(min_value == 9);

    let (max_value, loc) = ripp::max_indx_8u_c1r(&im, im.size()).unwrap();
    assert!(loc.x() == 3);
    assert!(loc.y() == 4);
    assert!(max_value == 20);
}

fn eigen_2x2_real(a: f64, b: f64, c: f64, d: f64) -> Result<(f64, f64, f64, f64), ()> {
    if c == 0.0 {
        return Err(()); // will face divide by zero
    }
    let inside = a * a + 4.0 * b * c - 2.0 * a * d + d * d;
    let inside = f64::sqrt(inside);
    let eval_a = 0.5 * (a + d - inside);
    let eval_b = 0.5 * (a + d + inside);
    let evec_a1 = (-a + d + inside) / (-2.0 * c);
    let evec_b1 = (-a + d - inside) / (-2.0 * c);
    Ok((eval_a, evec_a1, eval_b, evec_b1))
}

#[test]
fn test_threshold_val_8u_c1ir() {
    ripp::init().unwrap();

    let w = 5;
    let h = 1;
    let mut im = FastImageData::<u8>::new(w, h, 0).unwrap();
    im.pixel_slice_mut(0, 0)[0] = 20;
    im.pixel_slice_mut(0, 1)[0] = 21;
    im.pixel_slice_mut(0, 2)[0] = 22;
    im.pixel_slice_mut(0, 3)[0] = 23;
    im.pixel_slice_mut(0, 4)[0] = 24;

    let size = im.size().clone();
    ripp::threshold_val_8u_c1ir(&mut im, size, 22, 0, CompareOp::Less).unwrap();

    let mut expected = FastImageData::<u8>::new(w, h, 0).unwrap();
    expected.pixel_slice_mut(0, 0)[0] = 0;
    expected.pixel_slice_mut(0, 1)[0] = 0;
    expected.pixel_slice_mut(0, 2)[0] = 22;
    expected.pixel_slice_mut(0, 3)[0] = 23;
    expected.pixel_slice_mut(0, 4)[0] = 24;
    assert!(im.all_equal(expected));
}

#[test]
fn test_get_orientation() {
    ripp::init().unwrap();

    let w = 20;
    let h = 20;
    let mut im = FastImageData::<u8>::new(w, h, 0).unwrap();

    let expected_slope = 1.618; // TODO actually check that this has a slope of ~1.618
    im.pixel_slice_mut(4, 3)[0] = 1;
    im.pixel_slice_mut(5, 3)[0] = 1;
    im.pixel_slice_mut(5, 4)[0] = 1;
    im.pixel_slice_mut(6, 4)[0] = 1;

    let mut moments = MomentState::new(fastimage::AlgorithmHint::Fast).unwrap();
    ripp::moments_8u_c1r(&im, im.size(), &mut moments).unwrap();
    {
        let mu00 = moments
            .spatial(0, 0, 0, &fastimage::Point::new(0, 0))
            .unwrap();
        approx::assert_relative_eq!(mu00, 4.0);
        let mu10 = moments
            .spatial(1, 0, 0, &fastimage::Point::new(0, 0))
            .unwrap();
        approx::assert_relative_eq!(mu10, 14.0);
        let mu01 = moments
            .spatial(0, 1, 0, &fastimage::Point::new(0, 0))
            .unwrap();
        approx::assert_relative_eq!(mu01, 20.0);
    }

    {
        let mu00 = moments
            .spatial(0, 0, 0, &fastimage::Point::new(5, 10))
            .unwrap();
        approx::assert_relative_eq!(mu00, 4.0);
        let mu10 = moments
            .spatial(1, 0, 0, &fastimage::Point::new(5, 10))
            .unwrap();
        approx::assert_relative_eq!(mu10, 34.0);
        let mu01 = moments
            .spatial(0, 1, 0, &fastimage::Point::new(5, 10))
            .unwrap();
        approx::assert_relative_eq!(mu01, 60.0);
    }

    let uu11 = moments.central(1, 1, 0).unwrap();
    approx::assert_relative_eq!(uu11, 1.0);

    let uu20 = moments.central(2, 0, 0).unwrap();
    approx::assert_relative_eq!(uu20, 1.0);
    let uu02 = moments.central(0, 2, 0).unwrap();
    approx::assert_relative_eq!(uu02, 2.0);

    let (eval_a, evec_a1, eval_b, evec_b1) = eigen_2x2_real(uu20, uu11, uu11, uu02).unwrap();
    let rise = 1.0;
    let (run, _eccentricity) = if eval_a > eval_b {
        (evec_a1, eval_a / eval_b)
    } else {
        (evec_b1, eval_b / eval_a)
    };
    let slope = rise / run;
    approx::assert_relative_eq!(expected_slope, slope, epsilon = 1e-4);
}

macro_rules! gen_test_alloc {
    ($ty:ty, $pixel_val:expr_2021, $single_val:expr_2021, $name:ident) => {
        #[test]
        fn $name() {
            ripp::init().unwrap();

            let ws = vec![1, 2, 3, 31, 32, 33, 62, 63, 64, 65, 66];
            let h = 10;
            for w in ws.iter() {
                let im = fastimage::FastImageData::<$ty>::new(*w, h, $pixel_val).unwrap();
                println!("width: {}, stride: {}", w, im.stride());

                // Test the value of the last valid element.
                let last_valid_row = im.row_slice((im.height() - 1) as usize);
                let last_valid_element = last_valid_row[im.width() as usize * 1 as usize - 1];
                assert!(last_valid_element == $single_val);

                let full_slice = im.data();
                let bytes_per_element = std::mem::size_of::<$ty>();
                let elements_per_stride = im.stride() as usize / bytes_per_element;
                println!(
                    "bytes_per_element: {}, elements_per_stride: {}",
                    bytes_per_element, elements_per_stride
                );
                assert!(full_slice.len() == elements_per_stride * h as usize);

                // Check for out-of-bounds memory access, this could segfault if alloc were problematic.
                let last_element = full_slice[full_slice.len() - 1];
                println!("last_element: {}", last_element);
            }
        }
    };
}

gen_test_alloc!(u8, 1, 1, test_alloc_u8_c1);
gen_test_alloc!(f32, 1.0, 1.0, test_alloc_f32_c1);
