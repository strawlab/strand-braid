use nalgebra as na;
use nalgebra::RealField;

use opencv_ros_camera::RosOpenCvIntrinsics;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirrorAxis {
    LeftRight,
    UpDown,
}

/// return a copy of this camera whose x coordinate is (image_width-x)
pub fn mirror<R: RealField + Copy>(
    self_: &RosOpenCvIntrinsics<R>,
    axis: MirrorAxis,
) -> Option<RosOpenCvIntrinsics<R>> {
    if !self_.rect.is_identity(na::convert(1.0e-7)) {
        None
    } else {
        let mut i2 = self_.clone();
        let x = match axis {
            MirrorAxis::LeftRight => {
                i2.k[(0, 0)] = -i2.k[(0, 0)];
                i2.k[(0, 1)] = -i2.k[(0, 1)];
                i2.p[(0, 0)] = -i2.p[(0, 0)];
                i2.p[(0, 1)] = -i2.p[(0, 1)];
                i2
            }
            MirrorAxis::UpDown => {
                i2.k[(1, 1)] = -i2.k[(1, 1)];
                i2.p[(1, 1)] = -i2.p[(1, 1)];
                i2
            }
        };
        // call new() to recompute cache
        Some(RosOpenCvIntrinsics::from_components(x.p, x.k, x.distortion, x.rect).unwrap())
    }
}

#[cfg(test)]
mod tests {
    use na::geometry::Point2;
    use nalgebra as na;
    use nalgebra::{allocator::Allocator, DefaultAllocator, U3, U7};

    #[test]
    #[cfg(feature = "serde-serialize")]
    fn test_serde() {
        let expected = crate::make_default_intrinsics();
        let buf = serde_json::to_string(&expected).unwrap();
        let actual: crate::RosOpenCvIntrinsics<f64> = serde_json::from_str(&buf).unwrap();
        assert!(expected == actual);
    }

    #[test]
    fn test_mirror()
    where
        DefaultAllocator: Allocator<f64, U7, U3>,
    {
        use cam_geom::{IntrinsicParameters, Points};
        use nalgebra::{OMatrix, U3, U7};

        use crate::intrinsics::{mirror, MirrorAxis::*};

        #[rustfmt::skip]
        let pts = Points::new(
            OMatrix::<f64, U7, U3>::from_row_slice(
                &[0.0,  0.0, 1.0,
                1.0,  0.0, 1.0,
                0.0,  1.0, 1.0,
                1.0,  1.0, 1.0,
                -1.0,  0.0, 1.0,
                0.0, -1.0, 1.0,
                -1.0, -1.0, 1.0]
            )
        );

        for axis in &[LeftRight, UpDown] {
            for (name, cam) in crate::tests::get_test_cameras().iter() {
                let cam = cam.intrinsics();
                let lr_mirror = mirror(cam, *axis).unwrap();

                let orig_pixels = cam.camera_to_pixel(&pts);
                let lr_pixels = lr_mirror.camera_to_pixel(&pts);

                println!("{}, axis: {:?}", name, axis);
                for i in 0..orig_pixels.data.nrows() {
                    let expected = match axis {
                        LeftRight => {
                            // TODO make comparison testing for symmetric
                            // reflection without getting cx from parameters.
                            let cx = cam.p[(0, 2)];
                            let expected_x = cx + (cx - orig_pixels.data[(i, 0)]);
                            let expected_y = orig_pixels.data[(i, 1)];
                            Point2::new(expected_x, expected_y)
                        }
                        UpDown => {
                            // TODO make comparison testing for symmetric
                            // reflection without getting cy from parameters.
                            let cy = cam.p[(1, 2)];
                            let expected_x = orig_pixels.data[(i, 0)];
                            let expected_y = cy + (cy - orig_pixels.data[(i, 1)]);
                            Point2::new(expected_x, expected_y)
                        }
                    };
                    println!(
                        "orig: {:?}, expected: {:?}, lr: {:?}",
                        orig_pixels.data.row(i),
                        expected,
                        lr_pixels.data.row(i)
                    );
                    let eps = 1e-10;
                    approx::assert_relative_eq!(expected[0], lr_pixels.data[(i, 0)], epsilon = eps);
                    approx::assert_relative_eq!(expected[1], lr_pixels.data[(i, 1)], epsilon = eps);
                }
            }
        }
    }
}
