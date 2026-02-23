use std::os::raw::{c_char, c_double, c_int, c_uchar, c_void};

#[repr(C)]
pub(crate) struct cv_return_value_double {
    pub(crate) is_cv_exception: c_char,
    pub(crate) is_other_exception: c_char,
    pub(crate) result: c_double,
}

#[repr(C)]
pub(crate) struct cv_return_value_bool {
    pub(crate) is_cv_exception: c_char,
    pub(crate) is_other_exception: c_char,
    pub(crate) result: bool,
}

#[repr(C)]
pub(crate) struct cv_return_value_slice {
    pub(crate) is_cv_exception: c_char,
    pub(crate) is_other_exception: c_char,
    pub(crate) ptr: *mut c_void,
    pub(crate) num_elements: c_int,
}

unsafe extern "C" {
    pub(crate) fn calibrate_camera(
        image_count: c_int,
        object_points: *const c_double,
        image_points: *const c_double,
        point_counts: *const c_int,
        imgWidth: c_int,
        imgHeight: c_int,
        camera_matrix: *mut c_double,
        distortion_coeffs: *mut c_double,
        rotation_matrices: *mut c_double,
        translation_vectors: *mut c_double,
    ) -> cv_return_value_double;

    pub(crate) fn find_chessboard_corners_inner(
        frame_data_rgb: *const c_uchar,
        frame_width: c_int,
        frame_height: c_int,
        pattern_width: c_int,
        pattern_height: c_int,
        result: *mut c_void,
    ) -> cv_return_value_bool;

    pub(crate) fn vec_point2f_new() -> *mut c_void;
    pub(crate) fn vec_point2f_delete(result: *mut c_void);
    pub(crate) fn vec_point2f_slice(result: *mut c_void) -> cv_return_value_slice;
}
