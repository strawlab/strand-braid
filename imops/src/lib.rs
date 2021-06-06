#![cfg_attr(not(feature = "std"), no_std)]

// The public functions are `#[inline]` because I have found with the benchmarks
// in this crate that this results in significant speedups.

use machine_vision_formats::{pixel_format::Mono8, ImageMutData, ImageStride};

// #[derive(Debug, Clone)]
// #[cfg_attr(feature = "std", derive(thiserror::Error))]
// pub enum Error {
//     #[cfg_attr(feature = "std", error("Invalid Format: {0}"))]
// InvalidFormat(machine_vision_formats::PixFmt),
// }

#[derive(Clone, Copy, Debug, PartialEq)]
enum Power {
    Zero,
    One,
    Two,
}

#[cfg(not(feature = "packed_simd"))]
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

#[cfg(not(feature = "packed_simd"))]
fn spatial_moment<IM>(im: &IM, m_ord: Power, n_ord: Power) -> f32
where
    IM: ImageStride<Mono8>,
{
    let mut accum: f32 = 0.0;

    let full_data = im.image_data();
    let datalen = im.height() as usize * im.stride();
    let data = &full_data[..datalen];
    let chunk_iter = data.chunks_exact(im.stride());

    for (row, rowdata) in chunk_iter.enumerate() {
        for (col, element) in rowdata[..im.width() as usize].iter().enumerate() {
            accum += mypow(row as u32, m_ord) * mypow(col as u32, n_ord) * *element as f32;
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
    IM: ImageStride<Mono8>,
{
    #[cfg(feature = "packed_simd")]
    {
        use packed_simd::f32x8;

        let mut accum: f32 = 0.0;

        let full_data = im.image_data();
        let datalen = im.height() as usize * im.stride();
        let data = &full_data[..datalen];
        let chunk_iter = data.chunks_exact(im.stride());

        for rowdata in chunk_iter {
            // trim from stride to width
            let rowdata = &rowdata[..im.width() as usize];
            let mut rowsum = f32x8::splat(0.0);

            let row_chunk_iter = rowdata.chunks_exact(8);
            let remainder = row_chunk_iter.remainder();
            for x in row_chunk_iter {
                rowsum += f32x8::new(
                    x[0] as f32,
                    x[1] as f32,
                    x[2] as f32,
                    x[3] as f32,
                    x[4] as f32,
                    x[5] as f32,
                    x[6] as f32,
                    x[7] as f32,
                );
            }

            accum += rowsum.sum();
            for x in remainder {
                accum += *x as f32;
            }
        }
        accum
    }

    #[cfg(not(feature = "packed_simd"))]
    {
        spatial_moment(im, Power::Zero, Power::Zero)
    }
}

/// Compute spatial image moment 1,0
///
/// Panics: panics if the image data is smaller than stride*height and if stride
/// is smaller than width.
#[inline]
pub fn spatial_moment_10<IM>(im: &IM) -> f32
where
    IM: ImageStride<Mono8>,
{
    #[cfg(feature = "packed_simd")]
    {
        let mut accum: f32 = 0.0;
        use packed_simd::f32x8;

        let full_data = im.image_data();
        let datalen = im.height() as usize * im.stride();
        let data = &full_data[..datalen];
        let chunk_iter = data.chunks_exact(im.stride());

        for (row, rowdata) in chunk_iter.enumerate() {
            // trim from stride to width
            let rowdata = &rowdata[..im.width() as usize];
            let mut rowsum = f32x8::splat(0.0);

            let row_chunk_iter = rowdata.chunks_exact(8);
            let remainder = row_chunk_iter.remainder();
            for x in row_chunk_iter {
                let tmp = f32x8::new(
                    x[0] as f32,
                    x[1] as f32,
                    x[2] as f32,
                    x[3] as f32,
                    x[4] as f32,
                    x[5] as f32,
                    x[6] as f32,
                    x[7] as f32,
                );
                rowsum += tmp * row as f32;
            }
            accum += rowsum.sum();
            for x in remainder {
                accum += *x as f32 * row as f32;
            }
        }
        accum
    }

    #[cfg(not(feature = "packed_simd"))]
    {
        spatial_moment(im, Power::One, Power::Zero)
    }
}

/// Compute spatial image moment 0,1
///
/// Panics: panics if the image data is smaller than stride*height and if stride
/// is smaller than width.
#[inline]
pub fn spatial_moment_01<IM>(im: &IM) -> f32
where
    IM: ImageStride<Mono8>,
{
    #[cfg(feature = "packed_simd")]
    {
        let mut accum: f32 = 0.0;
        use packed_simd::f32x8;

        let col_offset = f32x8::new(0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0);

        let full_data = im.image_data();
        let datalen = im.height() as usize * im.stride();
        let data = &full_data[..datalen];
        let chunk_iter = data.chunks_exact(im.stride());

        for rowdata in chunk_iter {
            // trim from stride to width
            let rowdata = &rowdata[..im.width() as usize];
            let n_chunks = rowdata.len() / 8; // integer division
            let chunk_end = n_chunks * 8;
            let mut rowsum = f32x8::splat(0.0);

            let row_chunk_iter = rowdata.chunks_exact(8);
            let remainder = row_chunk_iter.remainder();

            for (col_div_8, x) in row_chunk_iter.enumerate() {
                let tmp1 = f32x8::splat((col_div_8 * 8) as f32);
                let tmp2 = f32x8::new(
                    x[0] as f32,
                    x[1] as f32,
                    x[2] as f32,
                    x[3] as f32,
                    x[4] as f32,
                    x[5] as f32,
                    x[6] as f32,
                    x[7] as f32,
                );
                rowsum += (tmp1 + col_offset) * tmp2;
            }
            accum += rowsum.sum();

            let mut i = 0;
            for x in remainder {
                let col = chunk_end + i;
                i += 1; // why can I not do remainder.enumerate()?
                accum += *x as f32 * col as f32;
            }
        }
        accum
    }

    #[cfg(not(feature = "packed_simd"))]
    {
        spatial_moment(im, Power::Zero, Power::One)
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
    IM: ImageStride<Mono8> + ImageMutData<Mono8>,
{
    let stride = im.stride();
    let width = im.width() as usize;

    let datalen = im.height() as usize * stride;
    let full_data = &mut im.buffer_mut_ref().data[..];
    let data = &mut full_data[..datalen];
    let chunk_iter = data.chunks_exact_mut(stride);

    #[cfg(feature = "packed_simd")]
    {
        use packed_simd::u8x32;

        let low_vec = u8x32::splat(low);

        for rowdata in chunk_iter {
            // trim from stride to width
            let rowdata = &mut rowdata[..width];

            let mut row_chunk_iter = rowdata.chunks_exact_mut(32);

            while let Some(x) = row_chunk_iter.next() {
                let mut y = u8x32::from_slice_unaligned(x);
                y = y.max(low_vec);
                y.write_to_slice_unaligned(x);
            }

            let remainder = row_chunk_iter.into_remainder();
            for x in remainder {
                if *x < low {
                    *x = low;
                }
            }
        }
    }

    #[cfg(not(feature = "packed_simd"))]
    {
        for rowdata in chunk_iter {
            for element in rowdata[..width].iter_mut() {
                if *element < low {
                    *element = low;
                }
            }
        }
    }
    im
}

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
    IM: ImageStride<Mono8> + ImageMutData<Mono8>,
{
    let stride = im.stride();
    let width = im.width() as usize;

    let datalen = im.height() as usize * stride;
    let full_data = im.buffer_mut_ref();

    let data = &mut full_data.data[..datalen];
    let chunk_iter = data.chunks_exact_mut(stride);

    #[cfg(feature = "packed_simd")]
    {
        use packed_simd::u8x32;

        let avec = u8x32::splat(a);
        let bvec = u8x32::splat(b);
        let thresh_vec = u8x32::splat(thresh);

        for rowdata in chunk_iter {
            // trim from stride to width
            let rowdata = &mut rowdata[..width];

            let mut row_chunk_iter = rowdata.chunks_exact_mut(32);

            while let Some(x) = row_chunk_iter.next() {
                let mut y = u8x32::from_slice_unaligned(x);
                let indicator = match op {
                    CmpOp::LessThan => y.lt(thresh_vec),
                    CmpOp::LessEqual => y.le(thresh_vec),
                    CmpOp::Equal => y.eq(thresh_vec),
                    CmpOp::GreaterEqual => y.ge(thresh_vec),
                    CmpOp::GreaterThan => y.gt(thresh_vec),
                };
                y = indicator.select(avec, bvec);
                y.write_to_slice_unaligned(x);
            }

            let remainder = row_chunk_iter.into_remainder();
            for x in remainder {
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
    }

    #[cfg(not(feature = "packed_simd"))]
    {
        for rowdata in chunk_iter {
            for x in rowdata[..width].iter_mut() {
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

        let im = simple_frame::SimpleFrame {
            width: W as u32,
            height: H as u32,
            stride: STRIDE as u32,
            image_data,
            fmt: std::marker::PhantomData,
        };

        let im = clip_low(im, 42);

        let image_data: Vec<u8> = im.into();

        assert_eq!(image_data[0 * STRIDE + 0], 42);
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
                let im = simple_frame::SimpleFrame {
                    width: W as u32,
                    height: 1,
                    stride: W as u32,
                    image_data: vec![$orig; W],
                    fmt: std::marker::PhantomData,
                };

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

        let im = simple_frame::SimpleFrame {
            width: W as u32,
            height: H as u32,
            stride: STRIDE as u32,
            image_data,
            fmt: std::marker::PhantomData,
        };

        let im = threshold(im, CmpOp::LessThan, 42, 0, 255);

        let image_data: Vec<u8> = im.into();

        assert_eq!(image_data[0 * STRIDE + 0], 0);
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

        let im = simple_frame::SimpleFrame {
            width: W as u32,
            height: H as u32,
            stride: STRIDE as u32,
            image_data,
            fmt: std::marker::PhantomData,
        };

        assert_eq!(spatial_moment_00(&im), 4.0);
        assert_eq!(spatial_moment_01(&im), 14.0);
        assert_eq!(spatial_moment_10(&im), 20.0);
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

        let im = simple_frame::SimpleFrame {
            width: W as u32,
            height: H as u32,
            stride: STRIDE as u32,
            image_data,
            fmt: std::marker::PhantomData,
        };

        assert_eq!(spatial_moment_00(&im), 89.0);
        assert_eq!(spatial_moment_01(&im), 360.0);
        assert_eq!(spatial_moment_10(&im), 448.0);
    }
}
