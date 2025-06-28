use nalgebra::RealField;
use serde::{Deserialize, Serialize};

type Coords3D = (f64, f64, f64);
type Coords2D = (f64, f64);

#[derive(Serialize, Deserialize)]
pub struct CheckerBoardData {
    n_rows: usize,
    n_cols: usize,
    points: Vec<Coords2D>,
}

impl CheckerBoardData {
    pub fn new(n_rows: usize, n_cols: usize, points: &[Coords2D]) -> Self {
        let points = points.to_vec();
        Self {
            n_rows,
            n_cols,
            points,
        }
    }
}

fn to_image_points(board: &CheckerBoardData) -> Vec<Coords2D> {
    board.points.clone()
}

#[derive(Debug, Clone)]
pub struct PixelSize {
    width: usize,
    height: usize,
}

impl PixelSize {
    pub fn new(width: usize, height: usize) -> Self {
        Self { width, height }
    }
}

pub fn save_yaml<P: AsRef<std::path::Path>>(
    cam_info_file_stamped: P,
    pkg_name: &str,
    local: chrono::DateTime<chrono::Local>,
    raw_opencv_cal: &opencv_calibrate::CalibrationResult,
    raw_cam_name: &str,
) -> eyre::Result<()> {
    let intrinsics = convert_to_cam_geom::<f64>(&raw_opencv_cal);

    // Convert from braid_mvg to ROS format.
    let ci: opencv_ros_camera::RosCameraInfo<_> = opencv_ros_camera::NamedIntrinsicParameters {
        intrinsics,
        width: raw_opencv_cal.image_width as usize,
        height: raw_opencv_cal.image_height as usize,
        name: raw_cam_name.into(),
    }
    .into();

    let mut f = std::fs::File::create(&cam_info_file_stamped)?;
    std::io::Write::write_all(
        &mut f,
        format!(
            "# Saved by {} at {}\n\
                        # Mean reprojection distance: {:.2}\n",
            pkg_name, local, raw_opencv_cal.mean_reprojection_distance_pixels
        )
        .as_bytes(),
    )?;
    serde_yaml::to_writer(f, &ci)?;
    Ok(())
}

/// Given some checkerboard corner locations, compute intrinsics using OpenCV.
pub fn compute_intrinsics_with_raw_opencv<R: RealField>(
    size: PixelSize,
    data: &[CheckerBoardData],
) -> Result<opencv_calibrate::CalibrationResult, opencv_calibrate::Error> {
    /*
    cal = camera_calibration.calibrator.MonoCalibrator([])
    cal.size = (width,height)
    r = cal.cal_fromcorners(goodcorners)
    msg = cal.as_message()
    */

    let object_points: Vec<Vec<Coords3D>> = mk_object_points(data);
    let image_points: Vec<Vec<Coords2D>> = data.iter().map(to_image_points).collect();

    debug_assert!(object_points.len() == image_points.len());

    use opencv_calibrate::CorrespondingPoint;
    let pts: Vec<Vec<CorrespondingPoint>> = object_points
        .into_iter()
        .zip(image_points.into_iter())
        .map(|(obj_pts, im_pts)| {
            obj_pts
                .into_iter()
                .zip(im_pts.into_iter())
                .map(|(obj_pt, im_pt)| CorrespondingPoint {
                    object_point: obj_pt,
                    image_point: im_pt,
                })
                .collect()
        })
        .collect();

    Ok(opencv_calibrate::calibrate_camera(
        &pts,
        size.width as i32,
        size.height as i32,
    )?)
}

pub fn convert_to_cam_geom<R: RealField>(
    opencv_results: &opencv_calibrate::CalibrationResult,
) -> opencv_ros_camera::RosOpenCvIntrinsics<R> {
    let fx = nalgebra::convert(opencv_results.camera_matrix[0]);
    let skew = nalgebra::convert(opencv_results.camera_matrix[1]);
    let fy = nalgebra::convert(opencv_results.camera_matrix[4]);
    let cx = nalgebra::convert(opencv_results.camera_matrix[2]);
    let cy = nalgebra::convert(opencv_results.camera_matrix[5]);
    let dist = nalgebra::Vector5::new(
        nalgebra::convert(opencv_results.distortion_coeffs[0]),
        nalgebra::convert(opencv_results.distortion_coeffs[1]),
        nalgebra::convert(opencv_results.distortion_coeffs[2]),
        nalgebra::convert(opencv_results.distortion_coeffs[3]),
        nalgebra::convert(opencv_results.distortion_coeffs[4]),
    );
    let dist = opencv_ros_camera::Distortion::from_opencv_vec(dist);

    opencv_ros_camera::RosOpenCvIntrinsics::from_params_with_distortion(fx, skew, fy, cx, cy, dist)
}

/// Given some checkerboard corner locations, compute intrinsics
///
/// This is based on ROS camera_calibration.calibrator.MonoCalibrator. Note
/// that unlike ROS, which scales the image so that k and p matrices are
/// different, the code here does not. ROS does this so that undistorted images
/// fill the entire image area.
pub fn compute_intrinsics<R: RealField>(
    size: PixelSize,
    data: &[CheckerBoardData],
) -> Result<opencv_ros_camera::RosOpenCvIntrinsics<R>, opencv_calibrate::Error> {
    let opencv_results = compute_intrinsics_with_raw_opencv::<R>(size, data)?;
    Ok(convert_to_cam_geom(&opencv_results))
}

fn mk_object_points(data: &[CheckerBoardData]) -> Vec<Vec<Coords3D>> {
    /*

    def mk_object_points(self, boards, use_board_size = False):
        opts = []
        for i, b in enumerate(boards):
            num_pts = b.n_cols * b.n_rows
            opts_loc = numpy.zeros((num_pts, 1, 3), numpy.float32)
            for j in range(num_pts):
                opts_loc[j, 0, 0] = (j / b.n_cols)
                if self.pattern == Patterns.ACircles:
                    opts_loc[j, 0, 1] = 2*(j % b.n_cols) + (opts_loc[j, 0, 0] % 2)
                else:
                    opts_loc[j, 0, 1] = (j % b.n_cols)
                opts_loc[j, 0, 2] = 0
                if use_board_size:
                    opts_loc[j, 0, :] = opts_loc[j, 0, :] * b.dim
            opts.append(opts_loc)
        return opts

    */
    let mut result = Vec::with_capacity(data.len());
    for b in data.iter() {
        let num_pts = b.n_cols * b.n_rows;
        let mut opts_loc: Vec<Coords3D> = Vec::with_capacity(num_pts);
        for j in 0..num_pts {
            let x = (j as f64 / b.n_cols as f64).trunc();
            let y = j as f64 % b.n_cols as f64;
            let z = 0.0;
            opts_loc.push((x, y, z));
        }
        result.push(opts_loc);
    }
    result
}
