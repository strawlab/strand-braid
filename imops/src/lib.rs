#![cfg_attr(not(feature = "std"), no_std)]

/// The `simd` feature is now deprecated because SIMD is always enabled.
#[cfg(feature = "simd")]
const THE_SIMD_FEATURE_IS_DEPRECATED__SIMD_IS_NOW_ALWAYS_ENABLED: () = ();

// The public functions are `#[inline]` because I have found with the benchmarks
// in this crate that this results in significant speedups.

use machine_vision_formats::{
    iter::HasRowChunksExact, iter::HasRowChunksExactMut, pixel_format::Mono8, ImageMutData,
};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Power {
    Zero,
    One,
    Two,
}

#[inline]
fn mypow(x: u32, exp: Power) -> f64 {
    match exp {
        Power::Zero => 1.0,
        Power::One => x as f64,
        Power::Two => x as f64 * x as f64,
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
    let mut accum: f64 = 0.0;

    let chunk_iter = im.rowchunks_exact();

    for (row, rowdata) in chunk_iter.enumerate() {
        for (col, element) in rowdata.iter().enumerate() {
            accum += mypow(row as u32, n_ord) * mypow(col as u32, m_ord) * *element as f64;
        }
    }
    accum as f32
}

/// Compute spatial image moment 0,0
///
/// Panics: panics on image shape or stride problems.
#[inline]
pub fn spatial_moment_00<IM>(im: &IM) -> f32
where
    IM: HasRowChunksExact<Mono8>,
{
    let mut accum: f64 = 0.0;

    let chunk_iter = im.rowchunks_exact();

    for rowdata in chunk_iter {
        // trim from stride to width
        let rowdata = &rowdata[..im.width() as usize];

        let (head, body, tail): (&[u8], &[wide::u8x16], &[u8]) =
            wide::AlignTo::simd_align_to(rowdata);

        for x in head {
            accum += *x as f64;
        }

        let mut tmpsum: wide::u16x16 = wide::u16x16::ZERO;
        for (i, x) in body.iter().enumerate() {
            if i % 256 == 0 {
                // prevent overflow of u16 accumulator
                for xi in tmpsum.as_array() {
                    accum += *xi as f64;
                }
                tmpsum = wide::u16x16::ZERO;
            }
            let wide_x = wide::u16x16::from(*x);
            tmpsum += wide_x;
        }
        for xi in tmpsum.as_array() {
            accum += *xi as f64;
        }

        for x in tail {
            accum += *x as f64;
        }
    }
    accum as f32
}

/// Compute spatial image moment 0,1
///
/// Panics: panics on image shape or stride problems.
#[inline]
pub fn spatial_moment_01<IM>(im: &IM) -> f32
where
    IM: HasRowChunksExact<Mono8>,
{
    let mut accum: f64 = 0.0;
    use wide::f32x8;

    let chunk_iter = im.rowchunks_exact();

    for (row, rowdata) in chunk_iter.enumerate() {
        // trim from stride to width
        let rowdata = &rowdata[..im.width() as usize];

        let mut row_chunk_iter = rowdata.chunks_exact(8);

        let mut rowsum = f32x8::splat(0.0);
        let rowvec = f32x8::splat(row as f32);
        for x in &mut row_chunk_iter {
            let x = f32x8::new([
                x[0] as f32,
                x[1] as f32,
                x[2] as f32,
                x[3] as f32,
                x[4] as f32,
                x[5] as f32,
                x[6] as f32,
                x[7] as f32,
            ]);
            rowsum += x * rowvec;
        }
        accum += rowsum.reduce_add() as f64;

        for x in row_chunk_iter.remainder() {
            accum += *x as f64 * row as f64;
        }
    }
    accum as f32
}

/// Compute spatial image moment 1,0
///
/// Panics: panics on image shape or stride problems.
#[inline]
pub fn spatial_moment_10<IM>(im: &IM) -> f32
where
    IM: HasRowChunksExact<Mono8>,
{
    let mut accum: f64 = 0.0;
    use wide::f64x4;

    let col_offset = f64x4::new([0.0, 1.0, 2.0, 3.0]);

    let chunk_iter = im.rowchunks_exact();

    let start_idx = im.width() as usize / 4 * 4;

    for rowdata in chunk_iter {
        // trim from stride to width
        let rowdata = &rowdata[..im.width() as usize];

        let mut row_chunk_iter = rowdata.chunks_exact(4);

        let mut rowsum = f64x4::splat(0.0);
        for (col_div_4, x) in (&mut row_chunk_iter).enumerate() {
            let x = f64x4::new([x[0] as f64, x[1] as f64, x[2] as f64, x[3] as f64]);
            let col = f64x4::splat((col_div_4 * 4) as f64) + col_offset;
            rowsum += x * col;
        }

        accum += rowsum.reduce_add() as f64;

        for (i, x) in row_chunk_iter.remainder().iter().enumerate() {
            let col = i + start_idx;
            accum += *x as f64 * col as f64;
        }
    }
    accum as f32
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
/// Panics: panics on image shape or stride problems.
#[inline]
pub fn clip_low<IM>(mut im: IM, low: u8) -> IM
where
    IM: HasRowChunksExact<Mono8> + ImageMutData<Mono8>,
{
    let width = im.width() as usize;

    let chunk_iter = im.rowchunks_exact_mut();

    #[inline]
    fn scalar_clip_low(scalar_data: &mut [u8], low: u8) {
        for element in scalar_data.iter_mut() {
            if *element < low {
                *element = low;
            }
        }
    }

    {
        use wide::u8x32;

        let low_vec = u8x32::splat(low);

        for rowdata in chunk_iter {
            // trim from stride to width
            let rowdata = &mut rowdata[..width];

            let (head, body, tail): (&mut [u8], &mut [wide::u8x32], &mut [u8]) =
                wide::AlignTo::simd_align_to_mut(rowdata);

            scalar_clip_low(head, low);

            for y in body {
                *y = u8x32::max(*y, low_vec);
            }

            scalar_clip_low(tail, low);
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
/// Panics: panics on image shape or stride problems.
#[inline]
pub fn threshold<IM>(mut im: IM, op: CmpOp, thresh: u8, a: u8, b: u8) -> IM
where
    IM: HasRowChunksExact<Mono8> + ImageMutData<Mono8>,
{
    let width = im.width() as usize;
    let chunk_iter = im.rowchunks_exact_mut();

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

    use wide::u8x32;

    let avec = u8x32::splat(a);
    let bvec = u8x32::splat(b);
    let thresh_vec = u8x32::splat(thresh);

    for rowdata in chunk_iter {
        // trim from stride to width
        let rowdata = &mut rowdata[..width];

        let (head, body, tail): (&mut [u8], &mut [wide::u8x32], &mut [u8]) =
            wide::AlignTo::simd_align_to_mut(rowdata);

        scalar_cmp(head, thresh, a, b, op);

        for y in body.iter_mut() {
            use wide::{CmpEq, CmpGe, CmpGt, CmpLe, CmpLt};
            let indicator = match op {
                CmpOp::LessThan => y.simd_lt(thresh_vec),
                CmpOp::LessEqual => y.simd_le(thresh_vec),
                CmpOp::Equal => y.simd_eq(thresh_vec),
                CmpOp::GreaterEqual => y.simd_ge(thresh_vec),
                CmpOp::GreaterThan => y.simd_gt(thresh_vec),
            };
            *y = indicator.blend(avec, bvec);
        }

        scalar_cmp(tail, thresh, a, b, op);
    }

    im
}

#[cfg(feature = "std")]
#[cfg(test)]
mod tests {

    use std::u8;

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

        for row in 0..ALLOC_H {
            print!("row {row:2}: ");
            for col in 0..STRIDE {
                print!("{:3} ", image_data[row * STRIDE + col]);
            }
            println!();
        }

        assert_eq!(image_data[0], 42);
        assert_eq!(image_data[(H - 1) * STRIDE + (W - 1)], 42);
        assert_eq!(image_data[4 * STRIDE + 3], 43);
        assert_eq!(image_data[4 * STRIDE + 23], 1);
        assert_eq!(image_data[H * STRIDE + 4], 1);
        assert_eq!(image_data[(H + 1) * STRIDE + 6], 1);
    }

    macro_rules! gen_threshold_test {
        ($name:ident, $orig:expr_2021, $op:path, $thresh:expr_2021, $expected:expr_2021) => {
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
