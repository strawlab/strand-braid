use basic_frame::DynamicFrame;
use eyre::{self as anyhow};
use machine_vision_formats::{pixel_format, PixFmt};

use opencv::core::{self, Mat};
use opencv::prelude::{MatTraitConst, MatTraitConstManual};
use std::os::raw::c_void;

use opencv_ros_camera::RosOpenCvIntrinsics;

#[derive(Clone, Debug)]
pub(crate) struct UndistortionCache {
    mapx: Mat,
    mapy: Mat,
}

impl UndistortionCache {
    pub(crate) fn new(
        intrinsics: &RosOpenCvIntrinsics<f64>,
        width: usize,
        height: usize,
    ) -> anyhow::Result<Self> {
        let mut mapx = Mat::default();
        let mut mapy = Mat::default();

        let (camera_matrix, dist_coeffs) = to_opencv(intrinsics)?;

        // leave as empty, will be treated as identity 3x3 matrix.
        let rectify = Mat::default();

        // not sure about this
        let new_cam_matrix = camera_matrix.clone();

        let size = core::Size2i {
            width: width.try_into().unwrap(),
            height: height.try_into().unwrap(),
        };

        opencv::calib3d::init_undistort_rectify_map(
            &camera_matrix,
            &dist_coeffs,
            &rectify,
            &new_cam_matrix,
            size,
            core::CV_16SC2,
            &mut mapx,
            &mut mapy,
        )?;

        Ok(Self { mapx, mapy })
    }
}

pub(crate) fn undistort_image(
    decoded: DynamicFrame,
    undist_cache: &UndistortionCache,
) -> anyhow::Result<DynamicFrame> {
    let w = i32::try_from(decoded.width()).unwrap();
    let h = i32::try_from(decoded.height()).unwrap();
    let (rows, cols) = (
        decoded.height().try_into().unwrap(),
        decoded.width().try_into().unwrap(),
    );

    // convert to opencv::core::Mat
    let distorted_img = match decoded.pixel_format() {
        PixFmt::Mono8 => {
            let mono8 = decoded.into_pixel_format::<pixel_format::Mono8>()?;
            Mat::from_slice_rows_cols(&mono8.image_data, rows, cols)?
        }
        _ => {
            let rgb8 = decoded.into_pixel_format::<machine_vision_formats::pixel_format::RGB8>()?;
            let data_slice = rgb8.image_data.as_slice();
            let stride = rgb8.stride;
            unsafe {
                Mat::new_rows_cols_with_data(
                    h,
                    w,
                    core::CV_8UC3,
                    data_slice.as_ptr().cast::<c_void>().cast_mut(),
                    stride.try_into().unwrap(),
                )
            }?
            .try_clone()?
        }
    };

    let mut undistorted_img = Mat::default();

    opencv::imgproc::remap(
        &distorted_img,
        &mut undistorted_img,
        &undist_cache.mapx,
        &undist_cache.mapy,
        opencv::imgproc::INTER_LINEAR,
        core::BORDER_CONSTANT,
        core::Scalar::default(),
    )?;

    let dynamic_frame = match undistorted_img.typ() {
        core::CV_8U => {
            todo!("support for mono8 not yet implemented");
        }
        core::CV_8UC3 => {
            let stride = usize::try_from(w).unwrap() * 3;
            let nbytes = usize::try_from(stride).unwrap() * rows;
            let image_data = undistorted_img.data_bytes()?.to_vec();
            assert!(image_data.len() == nbytes);

            let basic = basic_frame::BasicFrame::<machine_vision_formats::pixel_format::RGB8> {
                width: w.try_into().unwrap(),
                height: h.try_into().unwrap(),
                stride: u32::try_from(stride).unwrap(),
                image_data,
                pixel_format: std::marker::PhantomData,
            };
            DynamicFrame::from(basic)
        }
        typ => {
            anyhow::bail!("unsupported opencv type {}", typ);
        }
    };
    Ok(dynamic_frame)
}

fn to_opencv(intrinsics: &RosOpenCvIntrinsics<f64>) -> anyhow::Result<(Mat, Mat)> {
    use opencv::core::Scalar;
    use opencv::prelude::*;
    let k = intrinsics.k;

    let mut camera_matrix =
        Mat::new_rows_cols_with_default(3, 3, f64::opencv_type(), Scalar::all(0.0))?;
    for i in 0usize..3 {
        for j in 0usize..3 {
            *(camera_matrix.at_2d_mut(i.try_into().unwrap(), j.try_into().unwrap())?) = k[(i, j)];
        }
    }

    let d = &intrinsics.distortion;
    let dvec: [f64; 5] = [
        d.radial1(),
        d.radial2(),
        d.tangential1(),
        d.tangential2(),
        d.radial3(),
    ];
    let dist_coeffs = Mat::from_slice(&dvec)?;
    Ok((camera_matrix, dist_coeffs))
}
