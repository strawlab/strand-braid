mod ffi;

use std::os::raw::{c_int, c_void};

#[derive(Debug)]
pub enum Error {
    CvError,
    GenericError,
    NoExtrinsicsFound,
}

impl std::error::Error for Error {}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl From<ffi::cv_return_value_double> for Result<f64, Error> {
    fn from(orig: ffi::cv_return_value_double) -> Result<f64, Error> {
        if orig.is_cv_exception != 0 {
            Err(Error::CvError)
        } else if orig.is_other_exception != 0 {
            Err(Error::GenericError)
        } else {
            Ok(orig.result)
        }
    }
}

impl From<ffi::cv_return_value_bool> for Result<bool, Error> {
    fn from(orig: ffi::cv_return_value_bool) -> Result<bool, Error> {
        if orig.is_cv_exception != 0 {
            Err(Error::CvError)
        } else if orig.is_other_exception != 0 {
            Err(Error::GenericError)
        } else {
            Ok(orig.result)
        }
    }
}

struct SliceData {
    ptr: *mut c_void,
    num_elements: usize,
}

impl From<ffi::cv_return_value_slice> for Result<SliceData, Error> {
    fn from(orig: ffi::cv_return_value_slice) -> Result<SliceData, Error> {
        if orig.is_cv_exception != 0 {
            Err(Error::CvError)
        } else if orig.is_other_exception != 0 {
            Err(Error::GenericError)
        } else {
            Ok(SliceData {
                ptr: orig.ptr,
                num_elements: orig.num_elements as usize,
            })
        }
    }
}

#[derive(Debug)]
pub struct CalibrationResult {
    /// mean reprojection error
    pub mean_reprojection_error: f64,
    /// camera calibration matrix, row major order
    pub camera_matrix: [f64; 9],
    /// non-linear distortion coefficients (k1, k2, p1, p2, k3)
    pub distortion_coeffs: [f64; 5],
    /// rotation matrices, row major order
    pub rotation_matrices: Vec<[f64; 9]>,
    /// rotation vectors
    pub translation_vectors: Vec<[f64; 3]>,
}

/// A point with a view in image (2D) and world (3D)
#[derive(Debug)]
pub struct CorrespondingPoint {
    pub object_point: (f64, f64, f64),
    pub image_point: (f64, f64),
}

pub fn calibrate_camera(
    all_pts: &Vec<Vec<CorrespondingPoint>>,
    width: i32,
    height: i32,
) -> Result<CalibrationResult, Error> {
    let point_counts: Vec<i32> = all_pts
        .iter()
        .map(|image_pts| image_pts.len() as i32)
        .collect();
    let flat_all_pts: Vec<&CorrespondingPoint> = all_pts.iter().flatten().collect();
    let total = flat_all_pts.len();

    let mut object_points = Vec::with_capacity(total * 3);
    let mut image_points = Vec::with_capacity(total * 2);

    for pt in flat_all_pts.iter() {
        object_points.push(pt.object_point.0);
        object_points.push(pt.object_point.1);
        object_points.push(pt.object_point.2);
        image_points.push(pt.image_point.0);
        image_points.push(pt.image_point.1);
    }
    let num_images = point_counts.len();
    debug_assert!(total * 3 == object_points.len());
    debug_assert!(total * 2 == image_points.len());

    let mut camera_matrix = [0.0; 9];
    camera_matrix[0] = 1.0;
    camera_matrix[4] = 1.0;
    camera_matrix[8] = 0.0;
    let mut distortion_coeffs = [0.0; 5];

    let mut rotation_matrices: Vec<[f64; 9]> = (0..num_images).map(|_| [0.0; 9]).collect();
    let mut translation_vectors: Vec<[f64; 3]> = (0..num_images).map(|_| [0.0; 3]).collect();

    let r1: Result<f64, Error> = unsafe {
        ffi::calibrate_camera(
            num_images as i32,
            object_points.as_ptr(),
            image_points.as_ptr(),
            point_counts.as_ptr(),
            width,
            height,
            camera_matrix.as_mut_ptr(),
            distortion_coeffs.as_mut_ptr(),
            rotation_matrices[0].as_mut_ptr(),
            translation_vectors[0].as_mut_ptr(),
        )
    }
    .into();
    let mean_reprojection_error = r1?;

    debug_assert!(rotation_matrices.len() == all_pts.len());
    debug_assert!(translation_vectors.len() == all_pts.len());
    Ok(CalibrationResult {
        mean_reprojection_error,
        camera_matrix,
        distortion_coeffs,
        rotation_matrices,
        translation_vectors,
    })
}

// TODO Port thi python code (from image_pipeline/camera_calibration/calibrator.py )
// def _get_corners(img, board, refine = True, checkerboard_flags=0):
//     """
//     Get corners for a particular chessboard for an image
//     """
//     h = img.shape[0]
//     w = img.shape[1]
//     if len(img.shape) == 3 and img.shape[2] == 3:
//         mono = cv2.cvtColor(img, cv2.COLOR_BGR2GRAY)
//     else:
//         mono = img

//     checkerboard_flags=cv2.CALIB_CB_FAST_CHECK
//     (ok, corners) = cv2.findChessboardCorners(mono, (board.n_cols, board.n_rows), flags = cv2.CALIB_CB_ADAPTIVE_THRESH |
//                                               cv2.CALIB_CB_NORMALIZE_IMAGE | checkerboard_flags)
//     if not ok:
//         return (ok, corners)

//     # If any corners are within BORDER pixels of the screen edge, reject the detection by setting ok to false
//     # NOTE: This may cause problems with very low-resolution cameras, where 8 pixels is a non-negligible fraction
//     # of the image size. See http://answers.ros.org/question/3155/how-can-i-calibrate-low-resolution-cameras
//     BORDER = 8
//     if not all([(BORDER < corners[i, 0, 0] < (w - BORDER)) and (BORDER < corners[i, 0, 1] < (h - BORDER)) for i in range(corners.shape[0])]):
//         ok = False

//     # Ensure that all corner-arrays are going from top to bottom.
//     if board.n_rows!=board.n_cols:
//         if corners[0, 0, 1] > corners[-1, 0, 1]:
//             corners = numpy.copy(numpy.flipud(corners))
//     else:
//         direction_corners=(corners[-1]-corners[0])>=numpy.array([[0.0,0.0]])

//         if not numpy.all(direction_corners):
//             if not numpy.any(direction_corners):
//                 corners = numpy.copy(numpy.flipud(corners))
//             elif direction_corners[0][0]:
//                 corners=numpy.rot90(corners.reshape(board.n_rows,board.n_cols,2)).reshape(board.n_cols*board.n_rows,1,2)
//             else:
//                 corners=numpy.rot90(corners.reshape(board.n_rows,board.n_cols,2),3).reshape(board.n_cols*board.n_rows,1,2)

//     if refine and ok:
//         # Use a radius of half the minimum distance between corners. This should be large enough to snap to the
//         # correct corner, but not so large as to include a wrong corner in the search window.
//         min_distance = float("inf")
//         for row in range(board.n_rows):
//             for col in range(board.n_cols - 1):
//                 index = row*board.n_rows + col
//                 min_distance = min(min_distance, _pdist(corners[index, 0], corners[index + 1, 0]))
//         for row in range(board.n_rows - 1):
//             for col in range(board.n_cols):
//                 index = row*board.n_rows + col
//                 min_distance = min(min_distance, _pdist(corners[index, 0], corners[index + board.n_cols, 0]))
//         radius = int(math.ceil(min_distance * 0.5))
//         cv2.cornerSubPix(mono, corners, (radius,radius), (-1,-1),
//                                       ( cv2.TERM_CRITERIA_EPS + cv2.TERM_CRITERIA_MAX_ITER, 30, 0.1 ))

//     return (ok, corners)

struct VecPoint2f {
    inner: *mut c_void,
}

impl VecPoint2f {
    fn new() -> Self {
        let inner = unsafe { ffi::vec_point2f_new() };
        Self { inner }
    }

    fn as_slice(&self) -> &[(f32, f32)] {
        let data: Result<SliceData, Error> = unsafe { ffi::vec_point2f_slice(self.inner) }.into();
        let data: SliceData = data.expect("slice");
        let result =
            unsafe { std::slice::from_raw_parts(data.ptr as *const (f32, f32), data.num_elements) };
        result
    }

    fn inner(&mut self) -> *mut c_void {
        self.inner
    }
}

impl Drop for VecPoint2f {
    fn drop(&mut self) {
        unsafe { ffi::vec_point2f_delete(self.inner) }
    }
}

pub fn find_chessboard_corners(
    rgb_data: &[u8],
    im_width: u32,
    im_height: u32,
    pattern_width: usize,
    pattern_height: usize,
) -> Result<Option<Vec<(f32, f32)>>, Error> {
    let mut corners = VecPoint2f::new();
    let r1: Result<bool, Error> = unsafe {
        ffi::find_chessboard_corners_inner(
            rgb_data.as_ptr(),
            im_width as c_int,
            im_height as c_int,
            pattern_width as c_int,
            pattern_height as c_int,
            corners.inner(),
        )
    }
    .into();
    let success: bool = r1?;
    if success {
        let cv_view: &[(f32, f32)] = corners.as_slice();
        Ok(Some(cv_view.to_vec()))
    } else {
        Ok(None)
    }
}

#[derive(Debug, Clone)]
pub struct Extrinsics {
    pub rvec: [f64; 3],
    pub tvec: [f64; 3],
}

pub enum PoseMethod {
    /// Infinitesimal Plane-Based Pose Estimation
    ///
    /// Object points must be coplanar.
    Ippe,
    /// Efficient Perspective-n-Point Camera Pose Estimation
    Epnp,
}

impl PoseMethod {
    fn to_c(&self) -> c_int {
        match self {
            Self::Ippe => unsafe { ffi::ippe() },
            Self::Epnp => unsafe { ffi::epnp() },
        }
    }
}

/// Finds an object pose from 3D-2D point correspondences.
pub fn solve_pnp(
    all_pts: &[CorrespondingPoint],
    camera_matrix: &[f64; 9],
    distortion_coeffs: &[f64; 5],
    method: PoseMethod,
) -> Result<Extrinsics, Error> {
    let n_points = all_pts.len();

    let mut object_points = Vec::with_capacity(n_points * 3);
    let mut image_points = Vec::with_capacity(n_points * 2);

    for pt in all_pts.iter() {
        object_points.push(pt.object_point.0);
        object_points.push(pt.object_point.1);
        object_points.push(pt.object_point.2);
        image_points.push(pt.image_point.0);
        image_points.push(pt.image_point.1);
    }

    let mut extrinsics = Extrinsics {
        rvec: [0.0f64; 3],
        tvec: [0.0f64; 3],
    };

    let r1: Result<bool, Error> = unsafe {
        ffi::solve_pnp(
            n_points.try_into().unwrap(),
            object_points.as_ptr(),
            image_points.as_ptr(),
            camera_matrix.as_ptr(),
            distortion_coeffs.as_ptr(),
            extrinsics.rvec.as_mut_ptr(),
            extrinsics.tvec.as_mut_ptr(),
            method.to_c(),
        )
    }
    .into();

    if !(r1?) {
        return Err(Error::NoExtrinsicsFound);
    }
    Ok(extrinsics)
}

#[test]
#[should_panic]
fn test_linking() {
    let rgb: &[u8] = b"12345678901234567890";
    println!("It is expected to see 'OpenCV Error: ...' below here");
    find_chessboard_corners(rgb, 4, 5, 1, 1).unwrap().unwrap();
}
