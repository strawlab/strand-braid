//! Provides fast image analysis operations
#![cfg_attr(feature = "portsimd", feature(portable_simd))]

pub use std::os::raw as ipp_ctypes;

/// SIMD vector width, in bytes. Also used for alignment.
const VECWIDTH: usize = aligned_vec::CACHELINE_ALIGN;
#[cfg(feature = "portsimd")]
use std::simd::{cmp::SimdPartialOrd, u8x32};

#[cfg(feature = "portsimd")]
pub const COMPILED_WITH_SIMD_SUPPORT: bool = true;

#[cfg(not(feature = "portsimd"))]
pub const COMPILED_WITH_SIMD_SUPPORT: bool = false;

// ---------------------------
// errors

pub type Result<M> = std::result::Result<M, Error>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("SizeError")]
    SizeError,
    #[error("MomentStateNotInitialized")]
    MomentStateNotInitialized,
    #[error("NotImplemented")]
    NotImplemented,
    #[error("MismatchedCompileRuntimeVersions (compiled: {0}, runtime: {1}, level: {2}")]
    MismatchedCompileRuntimeVersions(ipp_ctypes::c_int, ipp_ctypes::c_int, &'static str),
    #[error("LayoutError ({source})")]
    LayoutError {
        #[from]
        source: std::alloc::LayoutError,
    },
    #[error("ROI size error")]
    ROISizeError,
}

// ---------------------------

#[cfg(feature = "portsimd")]
#[inline]
fn absdiff_u8x32(im1: u8x32, im2: u8x32) -> u8x32 {
    // see V6 of https://stackoverflow.com/a/35779655/1633026

    let one = u8x32::splat(1);
    let two = u8x32::splat(2);

    let a = im1 - im2;
    let b_mask_i8 = im1.simd_lt(im2); // 0 false, -1 true
    let b: u8x32 = unsafe { std::mem::transmute(b_mask_i8) }; // 0 false, 255 true
    let b = b * two; // 0 false, 254 true
    let b = b + one; // 1 false, 255 true

    a * b
}

#[cfg(feature = "portsimd")]
#[test]
fn test_absdiff_u8x32() {
    use u8x32;
    let val = u8x32::splat;

    assert_eq!(absdiff_u8x32(val(0), val(0)), val(0));

    assert_eq!(absdiff_u8x32(val(8), val(10)), val(2));
    assert_eq!(absdiff_u8x32(val(10), val(8)), val(2));

    assert_eq!(absdiff_u8x32(val(255), val(0)), val(255));
    assert_eq!(absdiff_u8x32(val(0), val(255)), val(255));

    assert_eq!(absdiff_u8x32(val(255), val(1)), val(254));
    assert_eq!(absdiff_u8x32(val(1), val(255)), val(254));

    assert_eq!(absdiff_u8x32(val(254), val(0)), val(254));
    assert_eq!(absdiff_u8x32(val(0), val(254)), val(254));

    assert_eq!(absdiff_u8x32(val(254), val(1)), val(253));
    assert_eq!(absdiff_u8x32(val(1), val(254)), val(253));
}

#[cfg(feature = "portsimd")]
mod simd_generic {
    use super::*;

    pub fn abs_diff_8u_c1r<S1, S2, D>(
        src1: &S1,
        src2: &S2,
        dest: &mut D,
        size: FastImageSize,
    ) -> Result<()>
    where
        S1: FastImage<D = u8>,
        S2: FastImage<D = u8>,
        D: MutableFastImage<D = u8>,
    {
        let chunk_iter1 = src1.valid_row_iter(size)?;
        let chunk_iter2 = src2.valid_row_iter(size)?;
        let outchunk_iter = dest.valid_row_iter_mut(size)?;

        #[inline]
        fn scalar_adsdiff(aa: &[u8], bb: &[u8], cc: &mut [u8]) {
            debug_assert_eq!(aa.len(), bb.len());
            debug_assert_eq!(aa.len(), cc.len());
            for ((a, b), c) in aa.iter().zip(bb).zip(cc.iter_mut()) {
                *c = (*a as i16 - *b as i16).unsigned_abs() as u8;
            }
        }

        for ((rowdata_im1, rowdata_im2), outdata) in chunk_iter1.zip(chunk_iter2).zip(outchunk_iter)
        {
            {
                let mut im1_chunk_iter = rowdata_im1.chunks_exact(32);
                let mut im2_chunk_iter = rowdata_im2.chunks_exact(32);
                let mut out_chunk_iter = outdata.chunks_exact_mut(32);

                for ((a,b), c) in (&mut im1_chunk_iter).zip(&mut im2_chunk_iter).zip(&mut out_chunk_iter)
                {
                    let vec_im1 = u8x32::from_slice(a);
                    let vec_im2 = u8x32::from_slice(b);
                    let out_vec = absdiff_u8x32(vec_im1, vec_im2);
                    c.copy_from_slice(&out_vec.to_array());
                }

                scalar_adsdiff(im1_chunk_iter.remainder(), im2_chunk_iter.remainder(), out_chunk_iter.into_remainder());
            }
        }

        Ok(())
    }
}

// ------------------------------
// FastImageData
// ------------------------------

pub struct FastImageData<D>
where
    D: PixelType,
{
    data: aligned_vec::ABox<[D]>,
    stride_bytes: ipp_ctypes::c_int,
    size: FastImageSize,
}

fn _test_fast_image_data_is_send() {
    // Compile-time test to ensure FastImageData implements Send trait.
    fn implements<T: Send>() {}
    implements::<FastImageData<u8>>();
}

impl<D> FastImageData<D>
where
    D: PixelType,
{
    pub fn new(
        width_pixels: ipp_ctypes::c_int,
        height_pixels: ipp_ctypes::c_int,
        value: D,
    ) -> Result<Self> {
        let min_row_size_bytes = width_pixels as usize * std::mem::size_of::<D>();
        let mut n_simd_vectors_per_row = min_row_size_bytes / VECWIDTH;
        if n_simd_vectors_per_row * VECWIDTH < min_row_size_bytes {
            // There was some remainder, so add another simd vector.
            n_simd_vectors_per_row += 1;
        }
        let stride_bytes = n_simd_vectors_per_row * VECWIDTH;
        let n_pixels_per_row = stride_bytes / std::mem::size_of::<D>();
        debug_assert_eq!(n_pixels_per_row * std::mem::size_of::<D>(), stride_bytes);

        let data = aligned_vec::avec![value; n_pixels_per_row*height_pixels as usize];
        let data = data.into_boxed_slice();

        Ok(Self {
            data,
            stride_bytes: stride_bytes as i32,
            size: FastImageSize::new(width_pixels, height_pixels),
        })
    }
}

impl<D> PartialEq for FastImageData<D>
where
    D: PixelType,
{
    fn eq(&self, rhs: &Self) -> bool {
        fi_equal(self, rhs)
    }
}

impl FastImageData<u8> {
    pub fn copy_from_8u_c1<S>(src: &S) -> Result<Self>
    where
        S: FastImage<D = u8>,
    {
        let mut data = Self::new(src.width(), src.height(), 0)?;
        let size = data.size();
        ripp::copy_8u_c1r(src, &mut data, size)?;
        Ok(data)
    }

    pub fn copy_from_32f8u_c1<S>(src: &S, round_mode: RoundMode) -> Result<Self>
    where
        S: FastImage<D = f32>,
    {
        let mut data = Self::new(src.width(), src.height(), 0)?;
        let size = data.size();
        ripp::convert_32f8u_c1r(src, &mut data, size, round_mode)?;
        Ok(data)
    }
}

impl FastImageData<f32> {
    pub fn copy_from_8u32f_c1<S>(src: &S) -> Result<Self>
    where
        S: FastImage<D = u8>,
    {
        let mut data = Self::new(src.width(), src.height(), 0.0)?;
        let size = data.size();
        ripp::convert_8u32f_c1r(src, &mut data, size)?;
        Ok(data)
    }

    pub fn copy_from_32f_c1<S>(src: &S) -> Result<Self>
    where
        S: FastImage<D = f32>,
    {
        let mut data = Self::new(src.width(), src.height(), 0.0)?;
        let size = data.size();
        ripp::copy_32f_c1r(src, &mut data, size)?;
        Ok(data)
    }
}

impl<D> std::fmt::Debug for FastImageData<D>
where
    D: PixelType,
{
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("FastImageData")
            .field("size", &self.size)
            .field("stride_bytes", &self.stride_bytes)
            .finish_non_exhaustive()
    }
}

impl<D> FastImage for FastImageData<D>
where
    D: PixelType,
{
    type D = D;

    #[inline]
    fn raw_ptr(&self) -> *const Self::D {
        core::ptr::from_ref(&self.data.as_ref()[0])
    }

    #[inline]
    fn image_slice(&self) -> &[Self::D] {
        self.data.as_ref()
    }

    #[inline]
    fn stride(&self) -> ipp_ctypes::c_int {
        self.stride_bytes
    }

    #[inline]
    fn size(&self) -> FastImageSize {
        self.size
    }
}

impl<D> FastImage for &FastImageData<D>
where
    D: PixelType,
{
    type D = D;

    #[inline]
    fn raw_ptr(&self) -> *const Self::D {
        core::ptr::from_ref(&self.data.as_ref()[0])
    }

    #[inline]
    fn image_slice(&self) -> &[Self::D] {
        self.data.as_ref()
    }

    #[inline]
    fn stride(&self) -> ipp_ctypes::c_int {
        self.stride_bytes
    }

    #[inline]
    fn size(&self) -> FastImageSize {
        self.size
    }
}

impl<D> MutableFastImage for FastImageData<D>
where
    D: PixelType,
{
    #[inline]
    fn raw_mut_ptr(&mut self) -> *mut Self::D {
        core::ptr::from_mut(&mut self.data.as_mut()[0])
    }

    #[inline]
    fn image_slice_mut(&mut self) -> &mut [Self::D] {
        self.data.as_mut()
    }
}

// ------------------------------
// FastImageView
// ------------------------------

/// A view into existing image data.
pub struct FastImageView<'a, D>
where
    D: PixelType,
{
    data: &'a [D],
    stride: ipp_ctypes::c_int,
    size: FastImageSize,
}

impl<'a> FastImageView<'a, u8> {
    pub fn view<S: FastImage<D = u8>>(src: &'a S) -> Self {
        FastImageView::view_raw(
            src.image_slice(),
            src.stride(),
            src.width() as ipp_ctypes::c_int,
            src.height() as ipp_ctypes::c_int,
        )
        .unwrap()
    }

    pub fn view_region<S: FastImage<D = u8>>(src: &'a S, roi: &FastImageRegion) -> Result<Self> {
        let i0 =
            roi.left_bottom.y() as usize * src.stride() as usize + roi.left_bottom.x() as usize;
        FastImageView::view_raw(
            &src.image_slice()[i0..],
            src.stride(),
            roi.size.width(),
            roi.size.height(),
        )
    }

    pub fn view_raw(
        data: &'a [u8],
        stride: ipp_ctypes::c_int,
        width_pixels: ipp_ctypes::c_int,
        height_pixels: ipp_ctypes::c_int,
    ) -> Result<Self> {
        let width: usize = width_pixels.try_into().unwrap();
        let height: usize = height_pixels.try_into().unwrap();
        let strideu: usize = stride.try_into().unwrap();
        let min_size = (height - 1) * strideu + width;
        if data.len() >= min_size {
            Ok(Self {
                data,
                stride,
                size: FastImageSize::new(width_pixels, height_pixels),
            })
        } else {
            Err(Error::ROISizeError)
        }
    }
}

impl<D> FastImage for FastImageView<'_, D>
where
    D: PixelType,
{
    type D = D;

    #[inline]
    fn raw_ptr(&self) -> *const Self::D {
        self.data.as_ptr()
    }

    #[inline]
    fn image_slice(&self) -> &[Self::D] {
        self.data
    }

    #[inline]
    fn stride(&self) -> ipp_ctypes::c_int {
        self.stride
    }

    #[inline]
    fn size(&self) -> FastImageSize {
        self.size
    }
}

impl<D> std::fmt::Debug for FastImageView<'_, D>
where
    D: PixelType + std::fmt::Debug,
{
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(
            fmt,
            "FastImageView (width: {}, height: {})",
            self.width(),
            self.height()
        )?;
        for (i, row) in self.valid_row_iter(self.size).unwrap().enumerate() {
            writeln!(fmt, "  row {i} slice: {row:?}")?;
        }
        Ok(())
    }
}

impl<D> FastImage for &FastImageView<'_, D>
where
    D: PixelType + std::fmt::Debug,
{
    type D = D;

    #[inline]
    fn raw_ptr(&self) -> *const Self::D {
        self.data.as_ptr()
    }

    #[inline]
    fn image_slice(&self) -> &[Self::D] {
        self.data
    }

    #[inline]
    fn stride(&self) -> ipp_ctypes::c_int {
        self.stride
    }

    #[inline]
    fn size(&self) -> FastImageSize {
        self.size
    }
}

// ------------------------------
// MutableFastImageView
// ------------------------------

/// A mutable view into existing image data.
pub struct MutableFastImageView<'a, D>
where
    D: PixelType,
{
    data: &'a mut [D],
    stride: ipp_ctypes::c_int,
    size: FastImageSize,
}

impl<'a> MutableFastImageView<'a, u8> {
    pub fn view<S: MutableFastImage<D = u8>>(src: &'a mut S) -> Self {
        let (stride, width, height) = (src.stride(), src.width(), src.height());
        MutableFastImageView::view_raw(src.image_slice_mut(), stride, width, height).unwrap()
    }

    pub fn view_region<S: MutableFastImage<D = u8>>(
        src: &'a mut S,
        roi: &FastImageRegion,
    ) -> Result<Self> {
        let stride = src.stride();
        let i0 = roi.left_bottom.y() as usize * stride as usize + roi.left_bottom.x() as usize;
        let data = src.image_slice_mut();
        MutableFastImageView::view_raw(&mut data[i0..], stride, roi.size.width(), roi.size.height())
    }

    pub fn view_raw(
        data: &'a mut [u8],
        stride: ipp_ctypes::c_int,
        width_pixels: ipp_ctypes::c_int,
        height_pixels: ipp_ctypes::c_int,
    ) -> Result<Self> {
        let width: usize = width_pixels.try_into().unwrap();
        let height: usize = height_pixels.try_into().unwrap();
        let strideu: usize = stride.try_into().unwrap();
        let min_size = (height - 1) * strideu + width;
        if data.len() >= min_size {
            Ok(Self {
                data,
                stride,
                size: FastImageSize::new(width_pixels, height_pixels),
            })
        } else {
            Err(Error::ROISizeError)
        }
    }
}

impl<D> FastImage for MutableFastImageView<'_, D>
where
    D: PixelType + std::fmt::Debug,
{
    type D = D;

    #[inline]
    fn raw_ptr(&self) -> *const Self::D {
        self.data.as_ptr()
    }

    #[inline]
    fn image_slice(&self) -> &[Self::D] {
        self.data
    }

    #[inline]
    fn stride(&self) -> ipp_ctypes::c_int {
        self.stride
    }

    #[inline]
    fn size(&self) -> FastImageSize {
        self.size
    }
}

impl<D> FastImage for &MutableFastImageView<'_, D>
where
    D: PixelType + std::fmt::Debug,
{
    type D = D;

    #[inline]
    fn raw_ptr(&self) -> *const Self::D {
        self.data.as_ptr()
    }

    #[inline]
    fn image_slice(&self) -> &[Self::D] {
        self.data
    }

    #[inline]
    fn stride(&self) -> ipp_ctypes::c_int {
        self.stride
    }

    #[inline]
    fn size(&self) -> FastImageSize {
        self.size
    }
}

impl<D> MutableFastImage for MutableFastImageView<'_, D>
where
    D: PixelType + std::fmt::Debug,
{
    #[inline]
    fn raw_mut_ptr(&mut self) -> *mut Self::D {
        self.data.as_mut_ptr()
    }

    #[inline]
    fn image_slice_mut(&mut self) -> &mut [Self::D] {
        self.data
    }
}

impl<D> std::fmt::Debug for MutableFastImageView<'_, D>
where
    D: PixelType + std::fmt::Debug,
{
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(
            fmt,
            "MutableFastImageView (width: {}, height: {})",
            self.width(),
            self.height()
        )?;
        for (i, row) in self.valid_row_iter(self.size).unwrap().enumerate() {
            writeln!(fmt, "  row {i} slice: {row:?}")?;
        }
        Ok(())
    }
}

// ------------------------------
// ValidChunksExact
// ------------------------------

/// An iterator over strided data in which only some is "valid".
///
/// This is modeled after [std::slice::ChunksExact].
pub struct ValidChunksExact<'a, T: 'a> {
    padded_chunk_iter: Option<std::slice::ChunksExact<'a, T>>,
    valid_n_elements: usize,
}

impl<'a, T> ValidChunksExact<'a, T> {
    fn new(slice: &'a [T], row_stride_n_elements: usize, valid_n_elements: usize) -> Self {
        assert!(valid_n_elements <= row_stride_n_elements);
        let padded_chunk_iter = Some(slice.chunks_exact(row_stride_n_elements));
        Self {
            padded_chunk_iter,
            valid_n_elements,
        }
    }
}

impl<'a, T> Iterator for ValidChunksExact<'a, T> {
    type Item = &'a [T];
    fn next(&mut self) -> Option<<Self as Iterator>::Item> {
        // Next iteration through chunked iterator.
        if let Some(mut padded_chunk_iter) = self.padded_chunk_iter.take() {
            if let Some(exact_chunk) = padded_chunk_iter
                .next()
                .map(|padded| &padded[0..self.valid_n_elements])
            {
                // Store the iterator and return the result.
                self.padded_chunk_iter = Some(padded_chunk_iter);
                Some(exact_chunk)
            } else {
                // The exact-size chunks are done, now do the remainder.
                let last_elements = padded_chunk_iter.remainder();
                if last_elements.is_empty() {
                    None
                } else {
                    Some(&last_elements[0..self.valid_n_elements])
                }
            }
        } else {
            None
        }
    }
}

#[test]
fn test_padded_chunks() {
    {
        // f32
        let avec = vec![1.0, 2.0, 3.0, 4.0, -1.0, 1.1, 2.1, 3.1, 4.1, -1.0];
        let a1: &[f32] = avec.as_slice();

        let mut myiter = ValidChunksExact::new(a1, 5, 4);
        assert_eq!(myiter.next(), Some(&avec[0..4]));
        assert_eq!(myiter.next(), Some(&avec[5..9]));
        assert_eq!(myiter.next(), None);
    }

    {
        // u8
        let avec = vec![10, 20, 30, 40, 255, 11, 21, 31, 41, 25];
        let a1: &[u8] = avec.as_slice();

        let mut myiter = ValidChunksExact::new(a1, 5, 4);
        assert_eq!(myiter.next(), Some(&avec[0..4]));
        assert_eq!(myiter.next(), Some(&avec[5..9]));
        assert_eq!(myiter.next(), None);
    }
}

#[test]
fn test_padded_chunks_short() {
    // This is the same as the f32 case above and is a 2x4 valid matrix, but is
    // missing the last padding.

    // f32
    let avec = vec![1.0, 2.0, 3.0, 4.0, -1.0, 1.1, 2.1, 3.1, 4.1];
    let a1: &[f32] = avec.as_slice();

    let mut myiter = ValidChunksExact::new(a1, 5, 4);
    assert_eq!(myiter.next(), Some(&avec[0..4]));
    assert_eq!(myiter.next(), Some(&avec[5..9]));
    assert_eq!(myiter.next(), None);
}

// ------------------------------
// ValidChunksExactMut
// ------------------------------

/// An iterator over strided, mutable data in which only some is "valid".
///
/// This is modeled after [std::slice::ChunksExactMut].
pub struct ValidChunksExactMut<'a, T: 'a> {
    padded_chunk_iter_mut: Option<std::slice::ChunksExactMut<'a, T>>,
    valid_n_elements: usize,
}

impl<'a, T> ValidChunksExactMut<'a, T> {
    fn new(slice: &'a mut [T], padded_n_elements: usize, valid_n_elements: usize) -> Self {
        assert!(valid_n_elements <= padded_n_elements);
        let padded_chunk_iter_mut = Some(slice.chunks_exact_mut(padded_n_elements));
        Self {
            padded_chunk_iter_mut,
            valid_n_elements,
        }
    }
}

impl<'a, T> Iterator for ValidChunksExactMut<'a, T> {
    type Item = &'a mut [T];
    fn next(&mut self) -> Option<<Self as Iterator>::Item> {
        // Next iterattion through chunked iterator.
        if let Some(mut padded_chunk_iter_mut) = self.padded_chunk_iter_mut.take() {
            if let Some(exact_chunk) = padded_chunk_iter_mut
                .next()
                .map(|padded| &mut padded[0..self.valid_n_elements])
            {
                // Store the iterator and return the result.
                self.padded_chunk_iter_mut = Some(padded_chunk_iter_mut);
                Some(exact_chunk)
            } else {
                // The exact-size chunks are done, now do the remainder.
                let last_elements = padded_chunk_iter_mut.into_remainder();
                if last_elements.is_empty() {
                    None
                } else {
                    Some(&mut last_elements[0..self.valid_n_elements])
                }
            }
        } else {
            None
        }
    }
}

#[test]
fn test_padded_chunks_mut() {
    {
        let mut avec: Vec<f32> = vec![1.0, 2.0, 3.0, 4.0, -1.0, 1.1, 2.1, 3.1, 4.1, -1.0];
        let a1 = avec.as_mut_slice();

        {
            let myiter = ValidChunksExactMut::new(a1, 5, 4);

            let mut n_rows = 0;
            for (row_num, row) in myiter.enumerate() {
                let mut n_cols = 0;
                for el in row.iter_mut() {
                    *el += (row_num + 1) as f32 * 100.0;
                    n_cols += 1;
                }
                assert_eq!(n_cols, 4);
                n_rows += 1;
            }
            assert_eq!(n_rows, 2);
        }

        assert_eq!(
            &avec,
            &[101.0, 102.0, 103.0, 104.0, -1.0, 201.1, 202.1, 203.1, 204.1, -1.0]
        );
    }
}

#[test]
fn test_padded_chunks_short_mut() {
    {
        // This is the same as above and is a 2x4 valid matrix, but is missing
        // the last padding.
        let mut avec: Vec<f32> = vec![1.0, 2.0, 3.0, 4.0, -1.0, 1.1, 2.1, 3.1, 4.1];
        let a1 = avec.as_mut_slice();

        {
            let myiter = ValidChunksExactMut::new(a1, 5, 4);

            let mut n_rows = 0;
            for (row_num, row) in myiter.enumerate() {
                let mut n_cols = 0;
                for el in row.iter_mut() {
                    *el += (row_num + 1) as f32 * 100.0;
                    n_cols += 1;
                }
                assert_eq!(n_cols, 4);
                n_rows += 1;
            }
            assert_eq!(n_rows, 2);
        }

        assert_eq!(
            &avec,
            &[101.0, 102.0, 103.0, 104.0, -1.0, 201.1, 202.1, 203.1, 204.1]
        );
    }
}

pub trait PixelType: 'static + Copy + PartialEq {
    type PIXFMT;
}

impl PixelType for u8 {
    type PIXFMT = machine_vision_formats::pixel_format::Mono8;
}
impl PixelType for f32 {
    type PIXFMT = machine_vision_formats::pixel_format::Mono32f;
}

// ------------------------------
// FastImage
// ------------------------------

/// This trait allows working with image data with stride and size information.
///
/// It is conceptually similar to [machine_vision_formats::ImageStride]. There
/// are a few differences however:
/// * [Self::valid_row_iter] takes size information.
/// * [Self::stride], [Self::width], and [Self::height] return
///   [ipp_ctypes::c_int].
///
/// This trait was originally implemented to wrap Intel IPP image data
/// structures.
pub trait FastImage {
    /// Pixel data type (e.g. [u8] or [f32])
    type D: PixelType;

    /// Get the raw data for the entire image, including padding.
    fn image_slice(&self) -> &[Self::D];

    fn raw_ptr(&self) -> *const Self::D;

    /// Get the image stride in number of bytes.
    fn stride(&self) -> ipp_ctypes::c_int;

    /// Get the image width in number of pixels.
    #[inline]
    fn width(&self) -> ipp_ctypes::c_int {
        self.size().width()
    }
    /// Get the image height in number of pixels.
    #[inline]
    fn height(&self) -> ipp_ctypes::c_int {
        self.size().height()
    }

    /// Get the image size in number of pixels.
    fn size(&self) -> FastImageSize;

    /// Iterate over elements in each image row. Returns valid slices.
    #[inline]
    fn valid_row_iter(&self, size: FastImageSize) -> Result<ValidChunksExact<'_, Self::D>> {
        if size.width() > self.size().width() || size.height > self.size().height() {
            return Err(Error::SizeError);
        }
        let stride_n_pixels = self.stride() as usize / std::mem::size_of::<Self::D>();
        let pixel_width = size.width() as usize;
        let mut slice = self.image_slice();
        let max_n_pixels = stride_n_pixels * size.height() as usize;
        if max_n_pixels < slice.len() {
            slice = &slice[..max_n_pixels];
        }
        Ok(ValidChunksExact::new(slice, stride_n_pixels, pixel_width))
    }

    /// Get the raw data for a pixel.
    #[inline]
    fn pixel_slice(&self, row: usize, col: usize) -> &[Self::D] {
        let n_elements_per_row = self.stride() as usize / std::mem::size_of::<Self::D>();
        let idx = row * n_elements_per_row + col;
        &self.image_slice()[idx..idx + 1]
    }
}

/// Check if two FastImages have same size and values.
pub fn fi_equal<D, SRC1, SRC2>(self_: SRC1, other: SRC2) -> bool
where
    D: PixelType,
    SRC1: FastImage<D = D>,
    SRC2: FastImage<D = D>,
{
    if self_.size() != other.size() {
        return false;
    }
    // check row-by row
    for (self_row, other_row) in self_
        .valid_row_iter(self_.size())
        .unwrap()
        .zip(other.valid_row_iter(self_.size()).unwrap())
    {
        if self_row != other_row {
            return false;
        }
    }
    true
}

impl<D> machine_vision_formats::ImageData<D::PIXFMT> for &dyn FastImage<D = D>
where
    D: PixelType,
{
    fn width(&self) -> u32 {
        self.size().width as u32
    }
    fn height(&self) -> u32 {
        self.size().height as u32
    }
    fn buffer_ref(&self) -> machine_vision_formats::ImageBufferRef<'_, D::PIXFMT> {
        machine_vision_formats::ImageBufferRef::new(unsafe {
            std::mem::transmute::<&[D], &[u8]>(self.image_slice())
        })
    }
    fn buffer(self) -> machine_vision_formats::ImageBuffer<D::PIXFMT> {
        // Ideally we would just move the data, but that is tricky here. So we copy.
        let data = unsafe { std::mem::transmute::<&[D], &[u8]>(self.image_slice()) }.to_vec();
        machine_vision_formats::ImageBuffer::new(data)
    }
}

impl<D> machine_vision_formats::Stride for &dyn FastImage<D = D>
where
    D: PixelType,
{
    fn stride(&self) -> usize {
        FastImage::stride(*self) as usize
    }
}

pub trait MutableFastImage: FastImage {
    fn raw_mut_ptr(&mut self) -> *mut Self::D;

    /// Get the mutable raw data for the entire image, including padding.
    fn image_slice_mut(&mut self) -> &mut [Self::D];

    /// Iterate over elements in each image row. Returns mutable valid slices.
    #[inline]
    fn valid_row_iter_mut(
        &mut self,
        size: FastImageSize,
    ) -> Result<ValidChunksExactMut<'_, Self::D>> {
        if size.width() > self.size().width() || size.height() > self.size().height() {
            return Err(Error::SizeError);
        }
        let stride_n_pixels = self.stride() as usize / std::mem::size_of::<Self::D>();
        let pixel_width = size.width() as usize;
        let mut slice = self.image_slice_mut();
        let max_n_pixels = stride_n_pixels * size.height() as usize;
        if max_n_pixels < slice.len() {
            slice = &mut slice[..max_n_pixels];
        }
        Ok(ValidChunksExactMut::new(
            slice,
            stride_n_pixels,
            pixel_width,
        ))
    }

    #[inline]
    fn pixel_slice_mut(&mut self, row: usize, col: usize) -> &mut [Self::D] {
        let n_elements_per_row = self.stride() as usize / std::mem::size_of::<Self::D>();
        let idx = row * n_elements_per_row + col;
        &mut self.image_slice_mut()[idx..idx + 1]
    }
}

/// Size (in pixels) of a region
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct FastImageSize {
    width: ipp_ctypes::c_int,
    height: ipp_ctypes::c_int,
}

impl FastImageSize {
    pub fn new(width: ipp_ctypes::c_int, height: ipp_ctypes::c_int) -> Self {
        Self { width, height }
    }
    #[inline]
    pub fn width(&self) -> ipp_ctypes::c_int {
        self.width
    }
    #[inline]
    pub fn height(&self) -> ipp_ctypes::c_int {
        self.height
    }
}

#[derive(Debug, Clone)]
pub struct FastImageRegion {
    left_bottom: Point,
    size: FastImageSize,
}

impl FastImageRegion {
    #[inline]
    pub fn new(left_bottom: Point, size: FastImageSize) -> Self {
        Self { left_bottom, size }
    }

    #[inline]
    pub fn left(&self) -> ipp_ctypes::c_int {
        self.left_bottom.x()
    }

    #[inline]
    pub fn bottom(&self) -> ipp_ctypes::c_int {
        self.left_bottom.y()
    }

    #[inline]
    pub fn width(&self) -> ipp_ctypes::c_int {
        self.size.width()
    }

    #[inline]
    pub fn height(&self) -> ipp_ctypes::c_int {
        self.size.height()
    }

    #[inline]
    pub fn right(&self) -> ipp_ctypes::c_int {
        self.left() + self.size.width()
    }

    #[inline]
    pub fn top(&self) -> ipp_ctypes::c_int {
        self.bottom() + self.size.height()
    }

    #[inline]
    pub fn size(&self) -> FastImageSize {
        self.size
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Point {
    x: ipp_ctypes::c_int,
    y: ipp_ctypes::c_int,
}

impl Point {
    #[inline]
    pub fn new(x: ipp_ctypes::c_int, y: ipp_ctypes::c_int) -> Self {
        Self { x, y }
    }
    #[inline]
    pub fn x(&self) -> ipp_ctypes::c_int {
        self.x
    }
    #[inline]
    pub fn y(&self) -> ipp_ctypes::c_int {
        self.y
    }
}

#[derive(Clone, Copy, Debug)]
pub enum RoundMode {
    Near,
}

impl RoundMode {
    #[inline]
    fn f32_to_u8(&self, src: f32) -> u8 {
        src.round().clamp(0.0, 255.0) as u8
    }
}

#[test]
fn test_rounding() {
    let rm = RoundMode::Near;
    assert_eq!(rm.f32_to_u8(0.9), 1);
    assert_eq!(rm.f32_to_u8(1.1), 1);
    assert_eq!(rm.f32_to_u8(-1.1), 0);
    assert_eq!(rm.f32_to_u8(256.0), 255);
}

pub mod ripp {
    use super::*;

    pub fn copy_8u_c1r<SRC, DST>(src: &SRC, dest: &mut DST, size: FastImageSize) -> Result<()>
    where
        SRC: FastImage<D = u8>,
        DST: MutableFastImage<D = u8>,
    {
        for (src_row, dest_row) in src
            .valid_row_iter(size)?
            .zip(dest.valid_row_iter_mut(size)?)
        {
            for (src_el, dest_el) in src_row.iter().zip(dest_row.iter_mut()) {
                *dest_el = *src_el;
            }
        }
        Ok(())
    }

    pub fn copy_32f_c1r<SRC, DST>(src: &SRC, dest: &mut DST, size: FastImageSize) -> Result<()>
    where
        SRC: FastImage<D = f32>,
        DST: MutableFastImage<D = f32>,
    {
        for (src_row, dest_row) in src
            .valid_row_iter(size)?
            .zip(dest.valid_row_iter_mut(size)?)
        {
            for (src_el, dest_el) in src_row.iter().zip(dest_row.iter_mut()) {
                *dest_el = *src_el;
            }
        }
        Ok(())
    }

    pub fn convert_8u32f_c1r<S, D>(src: &S, dest: &mut D, size: FastImageSize) -> Result<()>
    where
        S: FastImage<D = u8>,
        D: MutableFastImage<D = f32>,
    {
        for (src_row, dest_row) in src
            .valid_row_iter(size)?
            .zip(dest.valid_row_iter_mut(size)?)
        {
            for (src_el, dest_el) in src_row.iter().zip(dest_row.iter_mut()) {
                *dest_el = (*src_el).into();
            }
        }
        Ok(())
    }

    pub fn convert_32f8u_c1r<SRC, DST>(
        src: &SRC,
        dest: &mut DST,
        size: FastImageSize,
        round_mode: RoundMode,
    ) -> Result<()>
    where
        SRC: FastImage<D = f32>,
        DST: MutableFastImage<D = u8>,
    {
        for (src_row, dest_row) in src
            .valid_row_iter(size)?
            .zip(dest.valid_row_iter_mut(size)?)
        {
            for (src_el, dest_el) in src_row.iter().zip(dest_row.iter_mut()) {
                {
                    *dest_el = round_mode.f32_to_u8(*src_el);
                }
            }
        }
        Ok(())
    }

    pub fn compare_c_8u_c1r<SRC, DST>(
        src: &SRC,
        value: u8,
        dest: &mut DST,
        size: FastImageSize,
        cmp_op: CompareOp,
    ) -> Result<()>
    where
        SRC: FastImage<D = u8>,
        DST: MutableFastImage<D = u8>,
    {
        for (src_row, dest_row) in src
            .valid_row_iter(size)?
            .zip(dest.valid_row_iter_mut(size)?)
        {
            for (src_el, dest_el) in src_row.iter().zip(dest_row.iter_mut()) {
                match cmp_op {
                    CompareOp::Less => {
                        *dest_el = if *src_el < value { 255 } else { 0 };
                    }
                    CompareOp::Greater => {
                        *dest_el = if *src_el > value { 255 } else { 0 };
                    }
                }
            }
        }
        Ok(())
    }

    pub fn min_indx_8u_c1r<S>(src: &S, size: FastImageSize) -> Result<(u8, Point)>
    where
        S: FastImage<D = u8>,
    {
        let mut value = 255;
        let mut loc = Point::new(0, 0);

        for (row, src_row) in src.valid_row_iter(size)?.enumerate() {
            for (col, src_el) in src_row.iter().enumerate() {
                if *src_el < value {
                    value = *src_el;
                    loc.x = col as i32;
                    loc.y = row as i32;
                }
            }
        }
        Ok((value, loc))
    }

    pub fn max_indx_8u_c1r<S>(src: &S, size: FastImageSize) -> Result<(u8, Point)>
    where
        S: FastImage<D = u8>,
    {
        let mut max_all = 0;
        let mut loc = Point::new(0, 0);

        // For each row
        for (row, src_row) in src.valid_row_iter(size)?.enumerate() {
            // find maximum value of each row
            let mut max_row = 0;
            for src_el in src_row.iter() {
                if *src_el > max_row {
                    max_row = *src_el;
                }
            }
            // and store if this maximum per-row value is the max overall.
            if max_row > max_all {
                max_all = max_row;
                loc.y = row as i32;
            }
        }

        // Now take the row with the maximum value
        let src_row = src.valid_row_iter(size)?.nth(loc.y as usize).unwrap();
        // and find the column with the maximum value.
        for (col, src_el) in src_row.iter().enumerate() {
            if max_all == *src_el {
                loc.x = col as i32;
                break;
            }
        }

        Ok((max_all, loc))
    }

    pub fn threshold_val_8u_c1ir<SRCDST>(
        src_dest: &mut SRCDST,
        size: FastImageSize,
        threshold: u8,
        value: u8,
        cmp_op: CompareOp,
    ) -> Result<()>
    where
        SRCDST: MutableFastImage<D = u8>,
    {
        const SIMD_SIZE: usize = 32;
        match cmp_op {
            CompareOp::Less => {
                for srcdest_row in src_dest.valid_row_iter_mut(size)? {
                    let mut my_iter = srcdest_row.chunks_exact_mut(SIMD_SIZE);
                    for srcdest_chunk in my_iter.by_ref() {
                        for srcdest in srcdest_chunk.iter_mut() {
                            if *srcdest < threshold {
                                *srcdest = value;
                            }
                        }
                    }
                    for srcdest in my_iter.into_remainder() {
                        if *srcdest < threshold {
                            *srcdest = value;
                        }
                    }
                }
            }
            CompareOp::Greater => {
                for srcdest_row in src_dest.valid_row_iter_mut(size)? {
                    let mut my_iter = srcdest_row.chunks_exact_mut(SIMD_SIZE);
                    for srcdest_chunk in my_iter.by_ref() {
                        for srcdest in srcdest_chunk.iter_mut() {
                            if *srcdest > threshold {
                                *srcdest = value;
                            }
                        }
                    }
                    for srcdest in my_iter.into_remainder() {
                        if *srcdest > threshold {
                            *srcdest = value;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Subtract `src1` from `src2` and put results in `dest`.
    /// In other words, `dest = src2 - src` for each pixel.
    pub fn sub_8u_c1rsfs<S1, S2, D>(
        src1: &S1,
        src2: &S2,
        dest: &mut D,
        size: FastImageSize,
        scale_factor: ipp_ctypes::c_int,
    ) -> Result<()>
    where
        S1: FastImage<D = u8>,
        S2: FastImage<D = u8>,
        D: MutableFastImage<D = u8>,
    {
        if scale_factor != 0 {
            return Err(Error::NotImplemented);
        }
        for ((im1_row, im2_row), dest_row) in src1
            .valid_row_iter(size)?
            .zip(src2.valid_row_iter(size)?)
            .zip(dest.valid_row_iter_mut(size)?)
        {
            for ((i1, i2), out) in im1_row.iter().zip(im2_row.iter()).zip(dest_row.iter_mut()) {
                *out = i2.saturating_sub(*i1);
            }
        }
        Ok(())
    }

    /// Subtract `src1` from `src2` and put results in `dest`.
    /// In other words, `dest = src2 - src` for each pixel.
    pub fn sub_32f_c1r<S1, S2, D>(
        src1: &S1,
        src2: &S2,
        dest: &mut D,
        size: FastImageSize,
    ) -> Result<()>
    where
        S1: FastImage<D = f32>,
        S2: FastImage<D = f32>,
        D: MutableFastImage<D = f32>,
    {
        for ((im1_row, im2_row), dest_row) in src1
            .valid_row_iter(size)?
            .zip(src2.valid_row_iter(size)?)
            .zip(dest.valid_row_iter_mut(size)?)
        {
            for ((i1, i2), out) in im1_row.iter().zip(im2_row.iter()).zip(dest_row.iter_mut()) {
                *out = *i2 - *i1;
            }
        }
        Ok(())
    }

    pub fn abs_32f_c1r<S, D>(src: &S, dest: &mut D, size: FastImageSize) -> Result<()>
    where
        S: FastImage<D = f32>,
        D: MutableFastImage<D = f32>,
    {
        for (src_row, dest_row) in src
            .valid_row_iter(size)?
            .zip(dest.valid_row_iter_mut(size)?)
        {
            for (src_el, dest_el) in src_row.iter().zip(dest_row.iter_mut()) {
                *dest_el = src_el.abs();
            }
        }
        Ok(())
    }

    pub fn sqrt_32f_c1ir<SRCDST>(src_dest: &mut SRCDST, size: FastImageSize) -> Result<()>
    where
        SRCDST: MutableFastImage<D = f32>,
    {
        for srcdest_row in src_dest.valid_row_iter_mut(size)? {
            for srcdest in srcdest_row.iter_mut() {
                *srcdest = srcdest.sqrt();
            }
        }
        Ok(())
    }

    pub fn mul_c_32f_c1ir<SD>(k: f32, src_dest: &mut SD, size: FastImageSize) -> Result<()>
    where
        SD: MutableFastImage<D = f32>,
    {
        for srcdest_row in src_dest.valid_row_iter_mut(size)? {
            for srcdest in srcdest_row.iter_mut() {
                *srcdest *= k;
            }
        }
        Ok(())
    }

    #[cfg(feature = "portsimd")]
    pub use super::simd_generic::abs_diff_8u_c1r;

    #[cfg(not(feature = "portsimd"))]
    pub fn abs_diff_8u_c1r<S1, S2, D>(
        src1: &S1,
        src2: &S2,
        dest: &mut D,
        size: FastImageSize,
    ) -> Result<()>
    where
        S1: FastImage<D = u8>,
        S2: FastImage<D = u8>,
        D: MutableFastImage<D = u8>,
    {
        for ((im1_row, im2_row), dest_row) in src1
            .valid_row_iter(size)?
            .zip(src2.valid_row_iter(size)?)
            .zip(dest.valid_row_iter_mut(size)?)
        {
            for ((i1, i2), out) in im1_row.iter().zip(im2_row.iter()).zip(dest_row.iter_mut()) {
                *out = (*i1 as i16 - *i2 as i16).unsigned_abs() as u8;
            }
        }
        Ok(())
    }

    pub fn add_weighted_8u32f_c1ir<SRC, SRCDST>(
        src: &SRC,
        src_dst: &mut SRCDST,
        size: FastImageSize,
        alpha: f32,
    ) -> Result<()>
    where
        SRC: FastImage<D = u8>,
        SRCDST: MutableFastImage<D = f32>,
    {
        let one_minus_alpha = 1.0 - alpha;
        for (src_row, src_dst_row) in src
            .valid_row_iter(size)?
            .zip(src_dst.valid_row_iter_mut(size)?)
        {
            for (src_el, src_dst_el) in src_row.iter().zip(src_dst_row.iter_mut()) {
                *src_dst_el = (*src_dst_el * one_minus_alpha) + (*src_el as f32 * alpha);
            }
        }
        Ok(())
    }

    pub fn add_weighted_32f_c1ir<SRC, SRCDST>(
        src: &SRC,
        src_dst: &mut SRCDST,
        size: FastImageSize,
        alpha: f32,
    ) -> Result<()>
    where
        SRC: FastImage<D = f32>,
        SRCDST: MutableFastImage<D = f32>,
    {
        let one_minus_alpha = 1.0 - alpha;
        for (src_row, src_dst_row) in src
            .valid_row_iter(size)?
            .zip(src_dst.valid_row_iter_mut(size)?)
        {
            for (src_el, src_dst_el) in src_row.iter().zip(src_dst_row.iter_mut()) {
                *src_dst_el = (*src_dst_el * one_minus_alpha) + (*src_el * alpha);
            }
        }
        Ok(())
    }

    pub fn moments_8u_c1r<S>(src: &S, size: FastImageSize, result: &mut MomentState) -> Result<()>
    where
        S: FastImage<D = u8>,
    {
        let roi = FastImageRegion::new(Point::new(0, 0), size);

        let im_view1 = FastImageView::view_region(src, &roi);
        let im_view: &dyn FastImage<D = u8> = &im_view1?;

        result.results = Some(imops::calculate_moments(&im_view));
        Ok(())
    }

    pub fn set_8u_c1r<DST>(value: u8, dest: &mut DST, size: FastImageSize) -> Result<()>
    where
        DST: MutableFastImage<D = u8>,
    {
        for dest_row in dest.valid_row_iter_mut(size)? {
            for dest_el in dest_row.iter_mut() {
                *dest_el = value;
            }
        }
        Ok(())
    }

    pub fn set_32f_c1r<DST>(value: f32, dest: &mut DST, size: FastImageSize) -> Result<()>
    where
        DST: MutableFastImage<D = f32>,
    {
        for dest_row in dest.valid_row_iter_mut(size)? {
            for dest_el in dest_row.iter_mut() {
                *dest_el = value;
            }
        }
        Ok(())
    }

    pub fn set_8u_c1mr<D, M>(value: u8, dest: &mut D, size: FastImageSize, mask: &M) -> Result<()>
    where
        D: MutableFastImage<D = u8>,
        M: FastImage<D = u8>,
    {
        for (mask_row, dest_row) in mask
            .valid_row_iter(size)?
            .zip(dest.valid_row_iter_mut(size)?)
        {
            for (mask_el, dest_el) in mask_row.iter().zip(dest_row.iter_mut()) {
                if *mask_el != 0 {
                    *dest_el = value
                }
            }
        }
        Ok(())
    }

    pub fn sqr_32f_c1ir<SRCDST>(src_dest: &mut SRCDST, size: FastImageSize) -> Result<()>
    where
        SRCDST: MutableFastImage<D = f32>,
    {
        for srcdest_row in src_dest.valid_row_iter_mut(size)? {
            for srcdest in srcdest_row.iter_mut() {
                *srcdest = *srcdest * *srcdest;
            }
        }
        Ok(())
    }
}

#[derive(Copy, Clone, Debug)]
pub enum AlgorithmHint {
    // NoHint,
    Fast,
    // Accurate,
}

#[derive(Copy, Clone, Debug)]
pub enum CompareOp {
    Less,
    // LessEqual,
    // Equal,
    // GreaterEqual,
    Greater,
}

pub struct MomentState {
    results: Option<imops::Moments>,
}

impl MomentState {
    pub fn new(_hint_algorithm: AlgorithmHint) -> Result<MomentState> {
        Ok(MomentState { results: None })
    }
    // fn as_mut_ptr(&mut self) -> *mut ipp_sys::MomentState64f {
    //     self.data.as_mut_ptr() as *mut ipp_sys::MomentState64f
    // }
    // fn as_ptr(&self) -> *const ipp_sys::MomentState64f {
    //     self.data.as_ptr() as *const ipp_sys::MomentState64f
    // }
    pub fn spatial(
        &self,
        m_ord: ipp_ctypes::c_int,
        n_ord: ipp_ctypes::c_int,
        n_channel: ipp_ctypes::c_int,
        roi_offset: &Point,
    ) -> Result<f64> {
        if roi_offset != &Point::new(0, 0) {
            return Err(Error::NotImplemented);
        }
        if n_channel != 0 {
            return Err(Error::NotImplemented);
        }
        if let Some(results) = self.results.as_ref() {
            match (m_ord, n_ord) {
                (0, 0) => Ok(results.m00.into()),
                (0, 1) => Ok(results.m01.into()),
                (1, 0) => Ok(results.m10.into()),
                _ => Err(Error::MomentStateNotInitialized),
            }
        } else {
            Err(Error::MomentStateNotInitialized)
        }
    }
    pub fn central(
        &self,
        m_ord: ipp_ctypes::c_int,
        n_ord: ipp_ctypes::c_int,
        n_channel: ipp_ctypes::c_int,
    ) -> Result<f64> {
        if n_channel != 0 {
            return Err(Error::NotImplemented);
        }
        if let Some(results) = self.results.as_ref() {
            match (m_ord, n_ord) {
                (1, 1) => Ok(results.u11.into()),
                (0, 2) => Ok(results.u02.into()),
                (2, 0) => Ok(results.u20.into()),
                _ => Err(Error::MomentStateNotInitialized),
            }
        } else {
            Err(Error::MomentStateNotInitialized)
        }
    }
}
