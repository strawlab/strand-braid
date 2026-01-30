/// The `portsimd` feature is now deprecated because SIMD is always enabled.
#[cfg(feature = "portsimd")]
const THE_PORTSIMD_FEATURE_IS_DEPRECATED__SIMD_IS_NOW_ALWAYS_ENABLED: () = ();

pub use std::os::raw as ipp_ctypes;

/// SIMD vector width, in bytes. Also used for alignment.
const VECWIDTH: usize = aligned_vec::CACHELINE_ALIGN;

// ---------------------------
// errors

pub type Result<M> = std::result::Result<M, Error>;

#[derive(thiserror::Error, Debug, PartialEq)]
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

#[inline(always)]
fn absdiff_u8x32(im1: wide::u8x32, im2: wide::u8x32) -> wide::u8x32 {
    let diff1 = im1.saturating_sub(im2);
    let diff2 = im2.saturating_sub(im1);
    use core::ops::BitOr;
    diff1.bitor(diff2)
}

#[test]
fn test_absdiff_u8x32() {
    use wide::u8x32;

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

// ------------------------------
// FastImageData
// ------------------------------

#[derive(Clone)]
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

#[test]
fn test_image_view_size() -> Result<()> {
    // Create a 3x3 image with a stride of 10 bytes
    let data = aligned_vec::avec![42; 30 as usize];
    let data = data.into_boxed_slice();
    let mut im = FastImageData::<u8> {
        data,
        stride_bytes: 10,
        size: FastImageSize::new(3, 3),
    };

    // Check that the image was created correctly.
    assert_eq!(im, FastImageData::new(3, 3, 42)?);

    // Test normal view
    let sz = FastImageSize::new(3, 3);
    let roi = FastImageRegion::new(Point::new(0, 0), sz);
    let im_view = FastImageView::view_region(&im, &roi)?;
    assert!(fi_equal(im_view, FastImageData::new(3, 3, 42)?));
    let im_view = MutableFastImageView::view_region(&mut im, &roi)?;
    assert!(fi_equal(im_view, FastImageData::new(3, 3, 42)?));

    // Now test a view in which the source has enough bytes but not the correct shape.
    let sz = FastImageSize::new(10, 3);
    let roi = FastImageRegion::new(Point::new(0, 0), sz);
    assert_eq!(
        FastImageView::view_region(&im, &roi),
        Err(Error::ROISizeError)
    );
    assert_eq!(
        MutableFastImageView::view_region(&mut im, &roi),
        Err(Error::ROISizeError)
    );

    Ok(())
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

impl<'a, D> PartialEq for FastImageView<'a, D>
where
    D: PixelType + std::fmt::Debug,
{
    fn eq(&self, rhs: &Self) -> bool {
        fi_equal(self, rhs)
    }
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
        if roi.left_bottom.y() + roi.size.height() > src.height()
            || roi.left_bottom.x() + roi.size.width() > src.width()
        {
            return Err(Error::ROISizeError);
        }
        FastImageView::view_raw(
            &src.image_slice()[i0..],
            src.stride(),
            roi.size.width(),
            roi.size.height(),
        )
    }

    fn view_raw(
        data: &'a [u8],
        stride: ipp_ctypes::c_int,
        width_pixels: ipp_ctypes::c_int,
        height_pixels: ipp_ctypes::c_int,
    ) -> Result<Self> {
        let width: usize = width_pixels.try_into().unwrap();
        let height: usize = height_pixels.try_into().unwrap();
        let strideu: usize = stride.try_into().unwrap();
        if height == 0 {
            Ok(Self {
                data: &data[..0],
                stride,
                size: FastImageSize::new(width_pixels, height_pixels),
            })
        } else {
            let min_size = (height - 1) * strideu + width;
            if data.len() >= min_size {
                Ok(Self {
                    data: &data[..min_size],
                    stride,
                    size: FastImageSize::new(width_pixels, height_pixels),
                })
            } else {
                Err(Error::ROISizeError)
            }
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

impl<'a, D> PartialEq for MutableFastImageView<'a, D>
where
    D: PixelType + std::fmt::Debug,
{
    fn eq(&self, rhs: &Self) -> bool {
        fi_equal(self, rhs)
    }
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
        if roi.left_bottom.y() + roi.size.height() > src.height()
            || roi.left_bottom.x() + roi.size.width() > src.width()
        {
            return Err(Error::ROISizeError);
        }
        let stride = src.stride();
        let i0 = roi.left_bottom.y() as usize * stride as usize + roi.left_bottom.x() as usize;
        let data = src.image_slice_mut();
        MutableFastImageView::view_raw(&mut data[i0..], stride, roi.size.width(), roi.size.height())
    }

    fn view_raw(
        data: &'a mut [u8],
        stride: ipp_ctypes::c_int,
        width_pixels: ipp_ctypes::c_int,
        height_pixels: ipp_ctypes::c_int,
    ) -> Result<Self> {
        let width: usize = width_pixels.try_into().unwrap();
        let height: usize = height_pixels.try_into().unwrap();
        let strideu: usize = stride.try_into().unwrap();
        if height == 0 {
            Ok(Self {
                data: &mut data[..0],
                stride,
                size: FastImageSize::new(width_pixels, height_pixels),
            })
        } else {
            let min_size = (height - 1) * strideu + width;
            if data.len() >= min_size {
                Ok(Self {
                    data: &mut data[..min_size],
                    stride,
                    size: FastImageSize::new(width_pixels, height_pixels),
                })
            } else {
                Err(Error::ROISizeError)
            }
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

impl<IM> FastImage for IM
where
    IM: machine_vision_formats::ImageStride<machine_vision_formats::pixel_format::Mono8>,
{
    type D = u8;

    fn image_slice(&self) -> &[u8] {
        machine_vision_formats::ImageData::image_data(self)
    }

    fn raw_ptr(&self) -> *const u8 {
        self.image_slice().as_ptr()
    }

    fn stride(&self) -> ipp_ctypes::c_int {
        machine_vision_formats::Stride::stride(self)
            .try_into()
            .unwrap()
    }

    fn size(&self) -> FastImageSize {
        FastImageSize::new(
            machine_vision_formats::ImageData::width(self) as ipp_ctypes::c_int,
            machine_vision_formats::ImageData::height(self) as ipp_ctypes::c_int,
        )
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
        // We cannot move the data because we are implementing for a reference.
        self.buffer_ref().to_buffer()
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
                        *dest_el = if *src_el < value { 0xFF } else { 0 };
                    }
                    CompareOp::Greater => {
                        *dest_el = if *src_el > value { 0xFF } else { 0 };
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

    #[inline]
    fn threshold_val_8u_c1ir_lt<SRCDST>(
        src_dest: &mut SRCDST,
        size: FastImageSize,
        threshold: u8,
        value: u8,
    ) -> Result<()>
    where
        SRCDST: MutableFastImage<D = u8>,
    {
        #[inline]
        fn scalar_threshlt(data: &mut [u8], threshold: u8, value: u8) {
            for v in data.iter_mut() {
                if *v < threshold {
                    *v = value;
                }
            }
        }

        let threshold_vec = wide::u8x32::splat(threshold);
        let value_vec = wide::u8x32::splat(value);

        for srcdest_row in src_dest.valid_row_iter_mut(size)? {
            let (head, body, tail): (&mut [u8], &mut [wide::u8x32], &mut [u8]) =
                wide::AlignTo::simd_align_to_mut(srcdest_row);

            scalar_threshlt(head, threshold, value);

            for body_vec in body.iter_mut() {
                let mask = wide::CmpLt::simd_lt(*body_vec, threshold_vec);
                *body_vec = mask.blend(value_vec, *body_vec);
            }

            scalar_threshlt(tail, threshold, value);
        }
        Ok(())
    }

    #[inline]
    fn threshold_val_8u_c1ir_gt<SRCDST>(
        src_dest: &mut SRCDST,
        size: FastImageSize,
        threshold: u8,
        value: u8,
    ) -> Result<()>
    where
        SRCDST: MutableFastImage<D = u8>,
    {
        #[inline]
        fn scalar_threshgt(data: &mut [u8], threshold: u8, value: u8) {
            for v in data.iter_mut() {
                if *v > threshold {
                    *v = value;
                }
            }
        }

        let threshold_vec = wide::u8x32::splat(threshold);
        let value_vec = wide::u8x32::splat(value);

        for srcdest_row in src_dest.valid_row_iter_mut(size)? {
            let (head, body, tail): (&mut [u8], &mut [wide::u8x32], &mut [u8]) =
                wide::AlignTo::simd_align_to_mut(srcdest_row);

            scalar_threshgt(head, threshold, value);

            for body_vec in body.iter_mut() {
                let mask = wide::CmpGt::simd_gt(*body_vec, threshold_vec);
                *body_vec = mask.blend(value_vec, *body_vec);
            }

            scalar_threshgt(tail, threshold, value);
        }
        Ok(())
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
        match cmp_op {
            CompareOp::Less => threshold_val_8u_c1ir_lt(src_dest, size, threshold, value),
            CompareOp::Greater => threshold_val_8u_c1ir_gt(src_dest, size, threshold, value),
        }
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
            let (im1_row_chunks, im1_row_remainder) = im1_row.as_chunks::<32>();
            let (im2_row_chunks, im2_row_remainder) = im2_row.as_chunks::<32>();
            let (out_row_chunks, out_row_remainder) = dest_row.as_chunks_mut::<32>();

            for ((a_chunk, b_chunk), c_chunk) in im1_row_chunks
                .iter()
                .zip(im2_row_chunks.iter())
                .zip(out_row_chunks.iter_mut())
            {
                // Unaligned loads to SIMD
                let a_vec = wide::u8x32::new(*a_chunk);
                let b_vec = wide::u8x32::new(*b_chunk);
                c_chunk.copy_from_slice(b_vec.saturating_sub(a_vec).as_array());
            }

            for ((i1, i2), out) in im1_row_remainder
                .iter()
                .zip(im2_row_remainder.iter())
                .zip(out_row_remainder.iter_mut())
            {
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
            for ((a, b), c) in aa.iter().zip(bb).zip(cc.iter_mut()) {
                *c = (*a as i16 - *b as i16).unsigned_abs() as u8;
            }
        }

        for ((rowdata_im1, rowdata_im2), outdata) in chunk_iter1.zip(chunk_iter2).zip(outchunk_iter)
        {
            let (im1_row_chunks, im1_row_remainder) = rowdata_im1.as_chunks::<32>();
            let (im2_row_chunks, im2_row_remainder) = rowdata_im2.as_chunks::<32>();
            let (out_row_chunks, out_row_remainder) = outdata.as_chunks_mut::<32>();
            assert_eq!(
                im1_row_chunks.len(),
                im2_row_chunks.len(),
                "Mismatched chunk lengths"
            );
            assert_eq!(
                im1_row_chunks.len(),
                out_row_chunks.len(),
                "Mismatched chunk lengths"
            );

            {
                for ((a_chunk, b_chunk), c_chunk) in im1_row_chunks
                    .iter()
                    .zip(im2_row_chunks.iter())
                    .zip(out_row_chunks.iter_mut())
                {
                    // Unaligned loads to SIMD
                    let a_vec = wide::u8x32::new(*a_chunk);
                    let b_vec = wide::u8x32::new(*b_chunk);
                    c_chunk.copy_from_slice(absdiff_u8x32(a_vec, b_vec).as_array());
                }
            }

            scalar_adsdiff(im1_row_remainder, im2_row_remainder, out_row_remainder);
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
        #[inline]
        fn scalar_blend(maskv: &[u8], value: u8, bb: &mut [u8]) {
            debug_assert_eq!(maskv.len(), bb.len());
            for (mask, b) in maskv.iter().zip(bb.iter_mut()) {
                debug_assert!(*mask == 0 || *mask == 0xFF);
                *b = if *mask != 0 { value } else { *b };
            }
        }

        let value_vec = wide::u8x32::splat(value);

        for (mask_row, dest_row) in mask
            .valid_row_iter(size)?
            .zip(dest.valid_row_iter_mut(size)?)
        {
            let (mask_row_chunks, mask_row_remainder) = mask_row.as_chunks::<32>();
            let (dest_row_chunks, dest_row_remainder) = dest_row.as_chunks_mut::<32>();

            {
                for (mask_chunk, dest_chunk) in
                    mask_row_chunks.iter().zip(&mut dest_row_chunks.iter_mut())
                {
                    // Unaligned loads to SIMD
                    let mask_vec = wide::u8x32::new(*mask_chunk);
                    let dest_vec = wide::u8x32::new(*dest_chunk);

                    let result_vec = mask_vec.blend(value_vec, dest_vec);
                    dest_chunk.copy_from_slice(result_vec.as_array());
                }
            }

            scalar_blend(mask_row_remainder, value, dest_row_remainder);
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
