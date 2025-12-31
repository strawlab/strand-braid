#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(feature = "simd", feature(portable_simd))]

#[cfg(feature = "simd")]
use std::simd::{
    cmp::{SimdPartialEq, SimdPartialOrd},
    num::{SimdFloat, SimdUint},
};

// The public functions are `#[inline]` because I have found with the benchmarks
// in this crate that this results in significant speedups.

use machine_vision_formats::{iter::HasRowChunksExact, pixel_format::Mono8, ImageMutData};

#[cfg(feature = "simd")]
pub const COMPILED_WITH_SIMD_SUPPORT: bool = true;

#[cfg(not(feature = "simd"))]
pub const COMPILED_WITH_SIMD_SUPPORT: bool = false;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Power {
    Zero,
    One,
    Two,
}

#[inline]
fn mypow(x: u32, exp: Power) -> f32 {
    match exp {
        Power::Zero => 1.0,
        Power::One => x as f32,
        Power::Two => x as f32 * x as f32,
    }
}

impl From<u8> for Power {
    fn from(orig: u8) -> Self {
        match orig {
            0 => Power::Zero,
            1 => Power::One,
            2 => Power::Two,
            _ => {
                unimplemented!();
            }
        }
    }
}

fn spatial_moment<IM>(im: &IM, m_ord: Power, n_ord: Power) -> f32
where
    IM: HasRowChunksExact<Mono8>,
{
    let mut accum: f32 = 0.0;

    let chunk_iter = im.rowchunks_exact();

    for (row, rowdata) in chunk_iter.enumerate() {
        for (col, element) in rowdata.iter().enumerate() {
            accum += mypow(row as u32, n_ord) * mypow(col as u32, m_ord) * *element as f32;
        }
    }
    accum
}

/// Compute spatial image moment 0,0
///
/// Panics: panics if the image data is smaller than stride*height and if stride
/// is smaller than width.
#[inline]
pub fn spatial_moment_00<IM>(im: &IM) -> f32
where
    IM: HasRowChunksExact<Mono8>,
{
    #[cfg(feature = "simd")]
    {
        use std::simd::f32x8;

        let mut accum: f32 = 0.0;

        let full_data = im.image_data();
        let datalen = im.height() as usize * im.stride();
        let data = &full_data[..datalen];
        let chunk_iter = data.chunks_exact(im.stride());

        for rowdata in chunk_iter {
            // trim from stride to width
            let rowdata = &rowdata[..im.width() as usize];

            let (prefix_data, main_row_data, remainder_data) = rowdata.as_simd::<8_usize>();

            for x in prefix_data {
                accum += *x as f32;
            }

            let mut rowsum = f32x8::splat(0.0);
            for x in main_row_data {
                rowsum += x.cast(); // converts u8 to f32
            }
            accum += rowsum.reduce_sum();

            for x in remainder_data {
                accum += *x as f32;
            }
        }
        accum
    }

    #[cfg(not(feature = "simd"))]
    {
        spatial_moment(im, Power::Zero, Power::Zero)
    }
}

/// Compute spatial image moment 0,1
///
/// Panics: panics if the image data is smaller than stride*height and if stride
/// is smaller than width.
#[inline]
pub fn spatial_moment_01<IM>(im: &IM) -> f32
where
    IM: HasRowChunksExact<Mono8>,
{
    #[cfg(feature = "simd")]
    {
        let mut accum: f32 = 0.0;
        use std::simd::f32x8;

        let full_data = im.image_data();
        let datalen = im.height() as usize * im.stride();
        let data = &full_data[..datalen];
        let chunk_iter = data.chunks_exact(im.stride());

        for (row, rowdata) in chunk_iter.enumerate() {
            // trim from stride to width
            let rowdata = &rowdata[..im.width() as usize];

            let (prefix_data, main_row_data, remainder_data) = rowdata.as_simd::<8_usize>();

            for x in prefix_data {
                accum += *x as f32 * row as f32;
            }

            let mut rowsum = f32x8::splat(0.0);
            let rowvec = f32x8::splat(row as f32);
            for x in main_row_data {
                rowsum += x.cast() * rowvec; // converts u8 to f32
            }
            accum += rowsum.reduce_sum();

            for x in remainder_data {
                accum += *x as f32 * row as f32;
            }
        }
        accum
    }

    #[cfg(not(feature = "simd"))]
    {
        spatial_moment(im, Power::Zero, Power::One)
    }
}

/// Compute spatial image moment 1,0
///
/// Panics: panics if the image data is smaller than stride*height and if stride
/// is smaller than width.
#[inline]
pub fn spatial_moment_10<IM>(im: &IM) -> f32
where
    IM: HasRowChunksExact<Mono8>,
{
    #[cfg(feature = "simd")]
    {
        let mut accum: f32 = 0.0;
        use std::simd::f32x8;

        let col_offset = f32x8::from_array([0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]);

        let full_data = im.image_data();
        let datalen = im.height() as usize * im.stride();
        let data = &full_data[..datalen];
        let chunk_iter = data.chunks_exact(im.stride());

        for rowdata in chunk_iter {
            // trim from stride to width
            let rowdata = &rowdata[..im.width() as usize];

            let (prefix_data, main_row_data, remainder_data) = rowdata.as_simd::<8_usize>();

            for (col, x) in prefix_data.iter().enumerate() {
                accum += *x as f32 * col as f32;
            }

            let start_idx = prefix_data.len();
            let mut rowsum = f32x8::splat(0.0);
            for (col_div_8, x) in main_row_data.iter().enumerate() {
                let col = f32x8::splat((col_div_8 * 8 + start_idx) as f32) + col_offset;
                rowsum += x.cast() * col;
            }
            accum += rowsum.reduce_sum();

            let start_idx = prefix_data.len() + main_row_data.len() * 8;
            for (i, x) in remainder_data.iter().enumerate() {
                let col = i + start_idx;
                accum += *x as f32 * col as f32;
            }
        }
        accum
    }

    #[cfg(not(feature = "simd"))]
    {
        spatial_moment(im, Power::One, Power::Zero)
    }
}

#[derive(Debug)]
pub struct Moments {
    pub centroid_x: f32,
    pub centroid_y: f32,

    pub m00: f32,
    pub m01: f32,
    pub m10: f32,
    pub u11: f32,
    pub u02: f32,
    pub u20: f32,
}

pub fn calculate_moments<IM>(im: &IM) -> Moments
where
    IM: HasRowChunksExact<Mono8>,
{
    let m00 = spatial_moment_00(im);
    let m01 = spatial_moment_01(im);
    let m10 = spatial_moment_10(im);

    let centroid_x = m01 / m00;
    let centroid_y = m10 / m00;

    let m11 = spatial_moment(im, Power::One, Power::One);
    let m02 = spatial_moment(im, Power::Zero, Power::Two);
    let m20 = spatial_moment(im, Power::Two, Power::Zero);

    let u11 = m11 - centroid_x * m10;
    let u02 = m02 - centroid_x * m01;
    let u20 = m20 - centroid_y * m10;

    // debug_assert_eq!(u11, m11 - centroid_y * m01);

    Moments {
        m00,
        m01,
        m10,
        centroid_x,
        centroid_y,
        u11,
        u02,
        u20,
    }
}

/// Set the minimum value of all pixels in the image to `low`.
///
/// Currently implemented only for `MONO8` pixel formats.
///
/// Panics: panics if the image data is smaller than stride*height and if stride
/// is smaller than width.
#[inline]
pub fn clip_low<IM>(mut im: IM, low: u8) -> IM
where
    IM: HasRowChunksExact<Mono8> + ImageMutData<Mono8>,
{
    let stride = im.stride();
    let width = im.width() as usize;

    let datalen = im.height() as usize * stride;
    let full_data = &mut *im.buffer_mut_ref().data;
    let data = &mut full_data[..datalen];
    let chunk_iter = data.chunks_exact_mut(stride);

    #[inline]
    fn scalar_clip_low(scalar_data: &mut [u8], low: u8) {
        for element in scalar_data.iter_mut() {
            if *element < low {
                *element = low;
            }
        }
    }

    #[cfg(feature = "simd")]
    {
        use std::simd::u8x32;

        let low_vec = u8x32::splat(low);

        for rowdata in chunk_iter {
            // trim from stride to width
            let rowdata = &mut rowdata[..width];

            let (prefix_data, main_row_data, remainder_data) = rowdata.as_simd_mut();
            scalar_clip_low(prefix_data, low);

            for y in main_row_data.iter_mut() {
                *y = u8x32::max(*y, low_vec);
            }

            scalar_clip_low(remainder_data, low);
        }
    }

    #[cfg(not(feature = "simd"))]
    {
        for rowdata in chunk_iter {
            scalar_clip_low(&mut rowdata[..width], low);
        }
    }
    im
}

#[derive(Debug, Clone, Copy)]
pub enum CmpOp {
    LessThan,
    LessEqual,
    Equal,
    GreaterEqual,
    GreaterThan,
}

/// Threshold the image so that all pixels compared with `op` to `thresh` are
/// set to `a` if true and otherwise to `b`.
///
/// Currently implemented only for `MONO8` pixel formats.
///
/// Panics: panics if the image data is smaller than stride*height and if stride
/// is smaller than width.
#[inline]
pub fn threshold<IM>(mut im: IM, op: CmpOp, thresh: u8, a: u8, b: u8) -> IM
where
    IM: HasRowChunksExact<Mono8> + ImageMutData<Mono8>,
{
    let stride = im.stride();
    let width = im.width() as usize;

    let datalen = im.height() as usize * stride;
    let full_data = im.buffer_mut_ref();

    let data = &mut full_data.data[..datalen];
    let chunk_iter = data.chunks_exact_mut(stride);

    #[inline]
    fn scalar_cmp(scalar_data: &mut [u8], thresh: u8, a: u8, b: u8, op: CmpOp) {
        for x in scalar_data {
            match op {
                CmpOp::LessThan => {
                    *x = if *x < thresh { a } else { b };
                }
                CmpOp::LessEqual => {
                    *x = if *x <= thresh { a } else { b };
                }
                CmpOp::Equal => {
                    *x = if *x == thresh { a } else { b };
                }
                CmpOp::GreaterEqual => {
                    *x = if *x >= thresh { a } else { b };
                }
                CmpOp::GreaterThan => {
                    *x = if *x > thresh { a } else { b };
                }
            }
        }
    }

    #[cfg(feature = "simd")]
    {
        use std::simd::u8x32;

        let avec = u8x32::splat(a);
        let bvec = u8x32::splat(b);
        let thresh_vec = u8x32::splat(thresh);

        for rowdata in chunk_iter {
            // trim from stride to width
            let rowdata = &mut rowdata[..width];

            let (prefix_data, main_row_data, remainder_data) = rowdata.as_simd_mut();

            scalar_cmp(prefix_data, thresh, a, b, op);

            for y in main_row_data.iter_mut() {
                let indicator = match op {
                    CmpOp::LessThan => y.simd_lt(thresh_vec),
                    CmpOp::LessEqual => y.simd_le(thresh_vec),
                    CmpOp::Equal => y.simd_eq(thresh_vec),
                    CmpOp::GreaterEqual => y.simd_ge(thresh_vec),
                    CmpOp::GreaterThan => y.simd_gt(thresh_vec),
                };
                *y = indicator.select(avec, bvec);
            }

            scalar_cmp(remainder_data, thresh, a, b, op);
        }
    }

    #[cfg(not(feature = "simd"))]
    {
        for rowdata in chunk_iter {
            scalar_cmp(&mut rowdata[..width], thresh, a, b, op);
        }
    }
    im
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_clip_low() {
        const STRIDE: usize = 24;
        const W: usize = 20;
        const H: usize = 20;
        const ALLOC_H: usize = 25;
        let mut image_data = vec![0u8; STRIDE * ALLOC_H];
        image_data[4 * STRIDE + 3] = 43;
        image_data[5 * STRIDE + 3] = 1;
        image_data[5 * STRIDE + 4] = 1;
        image_data[6 * STRIDE + 4] = 1;

        // Put some data in the buffer but outside the width and height. This
        // tests that strides and height limit are working correctly.
        image_data[4 * STRIDE + 23] = 1;
        image_data[5 * STRIDE + 23] = 1;
        image_data[H * STRIDE + 4] = 1;
        image_data[(H + 1) * STRIDE + 6] = 1;

        let im = machine_vision_formats::owned::OImage::new(W as u32, H as u32, STRIDE, image_data)
            .unwrap();

        let im = clip_low(im, 42);

        let image_data: Vec<u8> = im.into();

        assert_eq!(image_data[0], 42);
        assert_eq!(image_data[(H - 1) * STRIDE + (W - 1)], 42);
        assert_eq!(image_data[4 * STRIDE + 3], 43);
        assert_eq!(image_data[4 * STRIDE + 23], 1);
        assert_eq!(image_data[H * STRIDE + 4], 1);
        assert_eq!(image_data[(H + 1) * STRIDE + 6], 1);
    }

    macro_rules! gen_threshold_test {
        ($name:ident, $orig:expr, $op:path, $thresh:expr, $expected:expr) => {
            #[test]
            fn $name() {
                const W: usize = 33; // wider than u8x32

                let im = machine_vision_formats::owned::OImage::new(W as u32, 1, W, vec![$orig; W])
                    .unwrap();

                let im = threshold(im, $op, $thresh, 0, 255);
                let image_data: Vec<u8> = im.into();
                assert_eq!(image_data[0], $expected);
                assert_eq!(image_data[W - 1], $expected);
            }
        };
    }

    gen_threshold_test!(test_lt_1, 10, CmpOp::LessThan, 42, 0);
    gen_threshold_test!(test_lt_2, 10, CmpOp::LessThan, 10, 255);
    gen_threshold_test!(test_lt_3, 10, CmpOp::LessThan, 9, 255);

    gen_threshold_test!(test_le_1, 10, CmpOp::LessEqual, 42, 0);
    gen_threshold_test!(test_le_2, 10, CmpOp::LessEqual, 10, 0);
    gen_threshold_test!(test_le_3, 10, CmpOp::LessEqual, 9, 255);

    gen_threshold_test!(test_eq_1, 10, CmpOp::Equal, 42, 255);
    gen_threshold_test!(test_eq_2, 10, CmpOp::Equal, 10, 0);
    gen_threshold_test!(test_eq_3, 10, CmpOp::Equal, 9, 255);

    gen_threshold_test!(test_ge_1, 10, CmpOp::GreaterEqual, 42, 255);
    gen_threshold_test!(test_ge_2, 10, CmpOp::GreaterEqual, 10, 0);
    gen_threshold_test!(test_ge_3, 10, CmpOp::GreaterEqual, 9, 0);

    gen_threshold_test!(test_gt_1, 10, CmpOp::GreaterThan, 42, 255);
    gen_threshold_test!(test_gt_2, 10, CmpOp::GreaterThan, 10, 255);
    gen_threshold_test!(test_gt_3, 10, CmpOp::GreaterThan, 9, 0);

    #[test]
    fn test_threshold_less_than() {
        const STRIDE: usize = 24;
        const W: usize = 20;
        const H: usize = 20;
        const ALLOC_H: usize = 25;
        let mut image_data = vec![2u8; STRIDE * ALLOC_H];
        image_data[4 * STRIDE + 3] = 43;
        image_data[4 * STRIDE + 4] = 42;
        image_data[4 * STRIDE + 5] = 41;
        image_data[5 * STRIDE + 3] = 1;
        image_data[5 * STRIDE + 4] = 1;
        image_data[6 * STRIDE + 4] = 1;

        // Put some data in the buffer but outside the width and height. This
        // tests that strides and height limit are working correctly.
        image_data[4 * STRIDE + 23] = 1;
        image_data[5 * STRIDE + 23] = 1;
        image_data[H * STRIDE + 4] = 1;
        image_data[(H + 1) * STRIDE + 6] = 1;

        let im = machine_vision_formats::owned::OImage::new(W as u32, H as u32, STRIDE, image_data)
            .unwrap();

        let im = threshold(im, CmpOp::LessThan, 42, 0, 255);

        let image_data: Vec<u8> = im.into();

        assert_eq!(image_data[0], 0);
        assert_eq!(image_data[(H - 1) * STRIDE + (W - 1)], 0);
        assert_eq!(image_data[4 * STRIDE + 3], 255);
        assert_eq!(image_data[4 * STRIDE + 4], 255);
        assert_eq!(image_data[4 * STRIDE + 5], 0);
        assert_eq!(image_data[4 * STRIDE + 23], 1);
        assert_eq!(image_data[4 * STRIDE + 22], 2);
        assert_eq!(image_data[H * STRIDE + 4], 1);
        assert_eq!(image_data[(H + 1) * STRIDE + 6], 1);
    }

    #[test]
    fn test_central_moments() {
        const STRIDE: usize = 20;
        const W: usize = 20;
        const H: usize = 20;
        const ALLOC_H: usize = 20;
        let mut image_data = vec![0u8; STRIDE * ALLOC_H];

        image_data[4 * STRIDE + 3] = 1;
        image_data[5 * STRIDE + 3] = 1;
        image_data[5 * STRIDE + 4] = 1;
        image_data[6 * STRIDE + 4] = 1;

        let im = machine_vision_formats::owned::OImage::new(W as u32, H as u32, STRIDE, image_data)
            .unwrap();

        let mr = calculate_moments(&im);
        assert_eq!(mr.u11, 1.0);
        assert_eq!(mr.u20, 1.0);
        assert_eq!(mr.u02, 2.0);
    }

    #[test]
    fn test_image_moments() {
        const STRIDE: usize = 24;
        const W: usize = 20;
        const H: usize = 20;
        const ALLOC_H: usize = 25;
        let mut image_data = vec![0u8; STRIDE * ALLOC_H];
        image_data[4 * STRIDE + 3] = 1;
        image_data[5 * STRIDE + 3] = 1;
        image_data[5 * STRIDE + 4] = 1;
        image_data[6 * STRIDE + 4] = 1;

        // Put some data in the buffer but outside the width and height. This
        // tests that strides and height limit are working correctly.
        image_data[4 * STRIDE + 23] = 255;
        image_data[5 * STRIDE + 23] = 255;
        image_data[H * STRIDE + 4] = 255;
        image_data[(H + 1) * STRIDE + 6] = 255;

        let im = machine_vision_formats::owned::OImage::new(W as u32, H as u32, STRIDE, image_data)
            .unwrap();

        assert_eq!(spatial_moment_00(&im), 4.0);
        assert_eq!(spatial_moment_10(&im), 14.0);
        assert_eq!(spatial_moment_01(&im), 20.0);
    }

    #[test]
    fn test_image_moments_remainder() {
        // This tests that data at column 19 in a width 20 image get used. This
        // tests the case where the image width is not divisible by 8 and that
        // the final column gets correctly used.

        const STRIDE: usize = 24;
        const W: usize = 20;
        const H: usize = 20;
        let mut image_data = vec![0u8; STRIDE * H];
        image_data[4 * STRIDE + 3] = 20;
        image_data[5 * STRIDE + 3] = 21;
        image_data[5 * STRIDE + 4] = 22;
        image_data[6 * STRIDE + 4] = 23;

        image_data[4 * STRIDE + 19] = 1;
        image_data[5 * STRIDE + 19] = 1;
        image_data[6 * STRIDE + 19] = 1;

        // Put some data in the buffer but outside the width. This tests that
        // strides are working correctly.
        image_data[4 * STRIDE + 23] = 255;
        image_data[5 * STRIDE + 23] = 255;

        let im = machine_vision_formats::owned::OImage::new(W as u32, H as u32, STRIDE, image_data)
            .unwrap();

        assert_eq!(spatial_moment_00(&im), 89.0);
        assert_eq!(spatial_moment_01(&im), 448.0);
        assert_eq!(spatial_moment_10(&im), 360.0);
    }

    #[test]
    fn test_wide_image_moments_simd() {
        // Test very wide image to check case where temporary u16 wide vector would overflow.
        const STRIDE: usize = 10000;
        const W: usize = 9000;
        const H: usize = 20;
        // Use maximum value to maximize potential chance of overflow.
        let mut image_data = vec![u8::MAX; STRIDE * H];

        // Put some other values in the first row of the buffer but outside the
        // width to test that strides are working correctly.
        image_data[W + 23] = 0;
        image_data[W + 24] = 0;

        let im = machine_vision_formats::owned::OImage::new(W as u32, H as u32, STRIDE, image_data)
            .unwrap();

        // computed the expected value.
        let expected: f64 = H as f64 * u8::MAX as f64 * W as f64;
        assert_eq!(
            spatial_moment(&im, Power::Zero, Power::Zero) as f64,
            expected
        );
        assert_eq!(
            spatial_moment_00(&im),
            spatial_moment(&im, Power::Zero, Power::Zero)
        );
        assert_eq!(spatial_moment_00(&im) as f64, expected);
        assert_eq!(
            spatial_moment_01(&im),
            spatial_moment(&im, Power::Zero, Power::One)
        );
        assert_eq!(
            spatial_moment_10(&im),
            spatial_moment(&im, Power::One, Power::Zero)
        );
    }

    #[test]
    fn test_tall_image_moments_simd() {
        // Test very wide image to check case where temporary u16 wide vector would overflow.
        const STRIDE: usize = 32;
        const W: usize = 20;
        const H: usize = 10000;
        // Use maximum value to maximize potential chance of overflow.
        let mut image_data = vec![u8::MAX; STRIDE * H];

        // Put some other values in the first row of the buffer but outside the
        // width to test that strides are working correctly.
        image_data[W + 3] = 0;
        image_data[W + 4] = 0;

        let im = machine_vision_formats::owned::OImage::new(W as u32, H as u32, STRIDE, image_data)
            .unwrap();

        // computed the expected value.
        let expected: f64 = H as f64 * u8::MAX as f64 * W as f64;
        assert_eq!(
            spatial_moment(&im, Power::Zero, Power::Zero) as f64,
            expected
        );
        assert_eq!(
            spatial_moment_00(&im),
            spatial_moment(&im, Power::Zero, Power::Zero)
        );
        assert_eq!(spatial_moment_00(&im) as f64, expected);
        assert_eq!(
            spatial_moment_01(&im),
            spatial_moment(&im, Power::Zero, Power::One)
        );
        assert_eq!(
            spatial_moment_10(&im),
            spatial_moment(&im, Power::One, Power::Zero)
        );
    }
}
