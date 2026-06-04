//! Backend facade for chessboard detection and camera calibration.
//!
//! These route to the pure-Rust [`checkerboard_calibrate`] implementation.

/// A point with a known world (3D) and image (2D) location.
#[derive(Debug, Clone, Copy)]
pub struct CorrespondingPoint {
    pub object_point: (f64, f64, f64),
    pub image_point: (f64, f64),
}

/// Result of [`calibrate_camera`].
#[derive(Debug, Clone)]
pub struct CalibrationResult {
    /// Overall reprojection error in pixels (RMS).
    pub mean_reprojection_distance_pixels: f64,
    /// Camera matrix `[fx 0 cx; 0 fy cy; 0 0 1]`, row-major.
    pub camera_matrix: [f64; 9],
    /// Distortion coefficients `(k1, k2, p1, p2, k3)`.
    pub distortion_coeffs: [f64; 5],
    pub image_width: u32,
    pub image_height: u32,
}

/// Error from a calibration backend.
#[derive(Debug)]
pub enum Error {
    Calibration(String),
    Detection(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Calibration(s) => write!(f, "calibration error: {s}"),
            Error::Detection(s) => write!(f, "chessboard detection error: {s}"),
        }
    }
}

impl std::error::Error for Error {}

/// Convert an interleaved RGB buffer to grayscale. The buffer is interpreted as
/// BGR and reduced as OpenCV's `COLOR_BGR2GRAY` would, so that color inputs are
/// handled identically to historical behavior. For grayscale inputs (R == G ==
/// B) this is the identity.
fn rgb_to_gray(rgb: &[u8], width: u32, height: u32) -> Vec<u8> {
    let n = (width as usize) * (height as usize);
    let mut gray = vec![0u8; n];
    for i in 0..n {
        let r = rgb[3 * i] as i32;
        let g = rgb[3 * i + 1] as i32;
        let b = rgb[3 * i + 2] as i32;
        // OpenCV BGR2GRAY fixed point (yuv_shift = 14): Y = R*4899 + G*9617 +
        // B*1868, with the buffer read as B,G,R — i.e. our byte0 is OpenCV's B
        // and our byte2 is OpenCV's R.
        gray[i] = ((b * 4899 + g * 9617 + r * 1868 + 8192) >> 14) as u8;
    }
    gray
}

pub fn find_chessboard_corners(
    rgb: &[u8],
    width: u32,
    height: u32,
    pattern_width: usize,
    pattern_height: usize,
) -> Result<Option<Vec<(f32, f32)>>, Error> {
    use checkerboard_calibrate::{CornerSubPixParams, GrayImageRef, corner_subpix};

    let gray = rgb_to_gray(rgb, width, height);
    let (w, h) = (width as usize, height as usize);
    let corners = checkerboard_calibrate::chessboard::find_chessboard_corners(
        &gray,
        w,
        h,
        pattern_width,
        pattern_height,
    );
    // Sub-pixel refine before returning.
    Ok(corners.map(|raw| {
        corner_subpix(
            GrayImageRef::new(&gray, w, h),
            &raw,
            &CornerSubPixParams::default(),
        )
    }))
}

pub fn calibrate_camera(
    all_pts: &[Vec<CorrespondingPoint>],
    width: i32,
    height: i32,
) -> Result<CalibrationResult, Error> {
    let views: Vec<Vec<checkerboard_calibrate::calibrate::CorrespondingPoint>> = all_pts
        .iter()
        .map(|view| {
            view.iter()
                .map(|cp| checkerboard_calibrate::calibrate::CorrespondingPoint {
                    object_point: cp.object_point,
                    image_point: cp.image_point,
                })
                .collect()
        })
        .collect();

    let res =
        checkerboard_calibrate::calibrate::calibrate_camera(&views, width as u32, height as u32)
            .map_err(|e| Error::Calibration(e.to_string()))?;

    Ok(CalibrationResult {
        mean_reprojection_distance_pixels: res.rms_reprojection_error,
        camera_matrix: res.camera_matrix,
        distortion_coeffs: res.distortion_coeffs,
        image_width: res.image_width,
        image_height: res.image_height,
    })
}
