use machine_vision_formats::{pixel_format, PixFmt};
use strand_dynamic_frame::DynamicFrame;

use opencv_ros_camera::RosOpenCvIntrinsics;

#[derive(Clone)]
pub struct UndistortionCache {
    mapx: kornia_tensor::CpuTensor2<f32>,
    mapy: kornia_tensor::CpuTensor2<f32>,
}

impl UndistortionCache {
    pub fn new(
        intrinsics: &RosOpenCvIntrinsics<f64>,
        width: usize,
        height: usize,
    ) -> eyre::Result<Self> {
        use kornia_imgproc::interpolation::grid::meshgrid_from_fn;

        let (mapx, mapy) = meshgrid_from_fn(width, height, |u, v| {
            let undist = opencv_ros_camera::UndistortedPixels {
                data: nalgebra::RowVector2::<f64>::new(u as f64, v as f64),
            };
            let dist = intrinsics.distort(&undist).data;
            Ok((dist[(0, 0)] as f32, dist[(0, 1)] as f32))
        })
        .unwrap();

        Ok(Self { mapx, mapy })
    }
}

pub fn undistort_image(
    decoded: DynamicFrame,
    undist_cache: &UndistortionCache,
) -> eyre::Result<DynamicFrame> {
    let width = decoded.width().try_into().unwrap();
    let height = decoded.height().try_into().unwrap();

    match decoded.pixel_format() {
        PixFmt::Mono8 => {
            // let mono8 = decoded.as_basic::<pixel_format::Mono8>().unwrap();

            // let data_u8: Vec<u8> = mono8.into();
            // let data_f32: Vec<f32> = data_u8.iter().map(|x| *x as f32).collect();

            // let image = kornia_image::image::Image::<f32, 1>::new(
            //     kornia_image::image::ImageSize { width, height },
            //     data_f32,
            // )?;

            // let undistorted_img = kornia_image::image::Image::<f32, 1>::from_size_val(
            //     kornia_image::image::ImageSize { width, height },
            //     0.0,
            // )?;

            todo!();
        }
        _ => {
            let rgb8 = decoded.into_pixel_format::<pixel_format::RGB8>().unwrap();
            let data_u8: Vec<u8> = rgb8.into();
            let data_f32: Vec<f32> = data_u8.iter().map(|x| *x as f32).collect();
            let image = kornia_image::image::Image::<f32, 3>::new(
                kornia_image::image::ImageSize { width, height },
                data_f32,
            )?;
            let mut undistorted_img = kornia_image::image::Image::<f32, 3>::from_size_val(
                kornia_image::image::ImageSize { width, height },
                0.0,
            )?;
            kornia_imgproc::interpolation::remap(
                &image,
                &mut undistorted_img,
                &undist_cache.mapx,
                &undist_cache.mapy,
                kornia_imgproc::interpolation::InterpolationMode::Bilinear,
            )?;
            let tensor: kornia_tensor::Tensor<f32, 3, _> = undistorted_img.0;
            if tensor.shape[2] != 3 {
                eyre::bail!("expected exactly 3 channels");
            }
            let data_f32 = tensor.into_vec();
            let data_u8: Vec<_> = data_f32.into_iter().map(|x| x as u8).collect();
            if data_u8.len() != width * height * 3 {
                eyre::bail!("unexpected output image size");
            }

            let basic = machine_vision_formats::owned::OImage::<
                machine_vision_formats::pixel_format::RGB8,
            >::new(
                width.try_into().unwrap(),
                height.try_into().unwrap(),
                (width * 3).try_into().unwrap(),
                data_u8,
            )
            .unwrap();
            Ok(DynamicFrame::from(basic))
        }
    }
}
