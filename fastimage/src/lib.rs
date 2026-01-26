//! Provides interface for Intel IPP

pub use std::os::raw as ipp_ctypes;

pub type IppStatusType = ipp_ctypes::c_int;
pub const NO_IPP_ERR: IppStatusType = ipp_sys::ippStsNoErr as IppStatusType;

// ---------------------------
// errors

pub fn ipp_status_string(status: IppStatusType) -> &'static str {
    // Intel manual says this is a "pointer to internal static buffer,
    // need not be released".
    let cstr = unsafe { ipp_sys::ippGetStatusString(status) };
    assert!(!cstr.is_null());
    let slice = unsafe { std::ffi::CStr::from_ptr(cstr) };
    slice.to_str().unwrap()
}

pub type Result<M> = std::result::Result<M, Error>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("IPP status error (code: {0}, {1})")]
    IppStatusError(IppStatusType, &'static str),
    #[error("MismatchedTypes")]
    MismatchedTypes,
    #[error("NoFastImageImplementation")]
    NoFastImageImplementation,
    #[error("SizeError")]
    SizeError,
    #[error("MomentStateNotInitialized")]
    MomentStateNotInitialized,
    #[error("NotImplemented")]
    NotImplemented,
    #[error("UnsupportedDataType")]
    UnsupportedDataType,
    #[error("UnsupportedChannelType")]
    UnsupportedChannelType,
    #[error("FailedAlloc")]
    FailedAlloc,
    #[error("ROISizeError")]
    ROISizeError,
    #[error("MismatchedCompileRuntimeVersions (compiled: {0}, runtime: {1}, level: {2}")]
    MismatchedCompileRuntimeVersions(ipp_ctypes::c_int, ipp_ctypes::c_int, &'static str),
}

// ---------------------------

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub mod simd_sse2 {
    use core::arch::x86_64::*;

    #[inline]
    #[target_feature(enable = "sse2")]
    unsafe fn _mm_abd_epu8(a: __m128i, b: __m128i) -> __m128i {
        // requires sse2
        _mm_sub_epi8(_mm_max_epu8(a, b), _mm_min_epu8(a, b))
    }

    /// Compute the absolute difference between img1 and img2, using SSE2
    /// instruction set.
    ///
    /// # Safety
    ///
    /// This unconditionally generates code that depends on the SSE2 instruction
    /// set. The caller must ensure that the SSE2 feature is available.
    #[target_feature(enable = "sse2")]
    pub unsafe fn abs_diff_8u_c1r(img1: &[u8], img2: &[u8], output: &mut [u8]) {
        assert_eq!(img1.len(), img2.len());
        assert_eq!(img1.len(), output.len());

        // TODO: use aligned load/store versions and use `align_to_mut()`.

        let i1 = img1.as_ptr();
        let i2 = img2.as_ptr();
        let o = output.as_mut_ptr();

        let mut start = 0;
        while start + 16 <= img1.len() {
            let i1s = i1.add(start);
            let i2s = i2.add(start);
            let os = o.add(start);

            let a = _mm_loadu_si128(i1s as *const __m128i);
            let b = _mm_loadu_si128(i2s as *const __m128i);

            let tmp = _mm_abd_epu8(a, b);

            _mm_storeu_si128(os as *mut __m128i, tmp);

            start += 16;
        }

        while start < img1.len() {
            let i1s = i1.add(start);
            let i2s = i2.add(start);
            let os = o.add(start);

            *os = std::cmp::max(*i1s, *i2s) - std::cmp::min(*i1s, *i2s);
            start += 1;
        }
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub mod simd_avx2 {
    use core::arch::x86_64::*;

    #[inline]
    #[target_feature(enable = "avx2")]
    unsafe fn _mm256_abd_epu8(a: __m256i, b: __m256i) -> __m256i {
        // requires avx2
        _mm256_sub_epi8(_mm256_max_epu8(a, b), _mm256_min_epu8(a, b))
    }

    /// Compute the absolute difference between img1 and img2, using AVX2
    /// instruction set.
    ///
    /// # Safety
    ///
    /// This unconditionally generates code that depends on the AVX2 instruction
    /// set. The caller must ensure that the AVX2 feature is available.
    #[target_feature(enable = "avx2")]
    pub unsafe fn abs_diff_8u_c1r(img1: &[u8], img2: &[u8], output: &mut [u8]) {
        assert_eq!(img1.len(), img2.len());
        assert_eq!(img1.len(), output.len());

        // TODO: use aligned load/store versions and use `align_to_mut()`.

        let i1 = img1.as_ptr();
        let i2 = img2.as_ptr();
        let o = output.as_mut_ptr();

        let mut start = 0;
        while start + 32 <= img1.len() {
            let i1s = i1.add(start);
            let i2s = i2.add(start);
            let os = o.add(start);

            let a = _mm256_loadu_si256(i1s as *const __m256i);
            let b = _mm256_loadu_si256(i2s as *const __m256i);

            let tmp = _mm256_abd_epu8(a, b);

            _mm256_storeu_si256(os as *mut __m256i, tmp);

            start += 32;
        }

        while start < img1.len() {
            let i1s = i1.add(start);
            let i2s = i2.add(start);
            let os = o.add(start);

            *os = std::cmp::max(*i1s, *i2s) - std::cmp::min(*i1s, *i2s);
            start += 1;
        }
    }
}

macro_rules! itry {
    ($x:expr) => {
        match unsafe { $x } {
            NO_IPP_ERR => {}
            e => {
                let s = ipp_status_string(e);
                return Err(Error::IppStatusError(e, s));
            }
        }
    };
}

// ------------------------------
// FastImageData
// ------------------------------

pub struct FastImageData<D>
where
    D: PixelType,
{
    data: Box<[D]>,
    stride: ipp_ctypes::c_int,
    size: FastImageSize,
}

impl<D> PartialEq for FastImageData<D>
where
    D: PixelType,
{
    fn eq(&self, rhs: &Self) -> bool {
        fi_equal(self, rhs)
    }
}

fn _test_partial_eq() {
    // Compile-time test that FastImageData implements PartialEq
    fn implements_partial_eq<T: PartialEq>() {}
    implements_partial_eq::<FastImageData<u8>>();
}

impl<D> FastImageData<D>
where
    D: PixelType,
{
    /// Allocate uninitialized memory. Unsafe because the memory contents are not defined.
    fn empty(
        value: D,
        width_pixels: ipp_ctypes::c_int,
        height_pixels: ipp_ctypes::c_int,
    ) -> Result<Self> {
        // TODO: use aligned alloc in rust rather than IPP allocator.
        // See https://github.com/rust-lang/rust/issues/32838#issuecomment-313843020
        // Layout::from_size_align

        let (dest_stride, data) = {
            let w = width_pixels as usize;
            let h = height_pixels as usize;
            let len = w * h * 1;
            let data = vec![value; len].into_boxed_slice();
            let stride = w * 1 * std::mem::size_of::<D>();
            (stride as ipp_ctypes::c_int, data)
        };

        Ok(Self {
            data,
            stride: dest_stride,
            size: FastImageSize::new(width_pixels, height_pixels),
        })
    }

    pub fn data(&self) -> &[D] {
        &self.data
    }
}

impl FastImageData<u8> {
    pub fn new(
        width_pixels: ipp_ctypes::c_int,
        height_pixels: ipp_ctypes::c_int,
        value: u8,
    ) -> Result<Self> {
        let data = Self::empty(value, width_pixels, height_pixels)?;
        Ok(data)
    }

    pub fn copy_from_8u_c1<S>(src: &S) -> Result<Self>
    where
        S: FastImage<D = u8>,
    {
        let mut data = Self::empty(0, src.width(), src.height())?;
        let size = *data.size();
        ripp::copy_8u_c1r(src, &mut data, &size)?;
        Ok(data)
    }

    pub fn copy_from_32f8u_c1<S>(src: &S, round_mode: RoundMode) -> Result<Self>
    where
        S: FastImage<D = f32>,
    {
        let mut data = Self::empty(0, src.width(), src.height())?;
        let size = *data.size();
        ripp::convert_32f8u_c1r(src, &mut data, &size, round_mode)?;
        Ok(data)
    }
}

impl FastImageData<f32> {
    pub fn new(
        width_pixels: ipp_ctypes::c_int,
        height_pixels: ipp_ctypes::c_int,
        value: f32,
    ) -> Result<Self> {
        let data = Self::empty(value, width_pixels, height_pixels)?;
        Ok(data)
    }

    pub fn copy_from_8u32f_c1<S>(src: &S) -> Result<Self>
    where
        S: FastImage<D = u8>,
    {
        let mut data = Self::empty(0.0, src.width(), src.height())?;
        let size = *data.size();
        ripp::convert_8u32f_c1r(src, &mut data, &size)?;
        Ok(data)
    }

    pub fn copy_from_32f_c1<S>(src: &S) -> Result<Self>
    where
        S: FastImage<D = f32>,
    {
        let mut data = Self::empty(0.0, src.width(), src.height())?;
        let size = *data.size();
        ripp::copy_32f_c1r(src, &mut data, &size)?;
        Ok(data)
    }
}

impl<D> std::fmt::Debug for FastImageData<D>
where
    D: PixelType + std::fmt::Debug,
{
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(
            fmt,
            "FastImageData (width: {}, height: {})",
            self.width(),
            self.height()
        )?;
        for row in 0..self.height() as usize {
            writeln!(fmt, "  row {} slice: {:?}", row, self.row_slice(row))?;
        }
        Ok(())
    }
}

impl<D> FastImage for FastImageData<D>
where
    D: PixelType,
{
    type D = D;

    #[inline]
    fn raw_ptr(&self) -> *const Self::D {
        self.data.as_ptr() as *const <FastImageData<D> as FastImage>::D
    }

    #[inline]
    fn stride(&self) -> ipp_ctypes::c_int {
        self.stride
    }

    #[inline]
    fn size(&self) -> &FastImageSize {
        &self.size
    }
}

impl<'a, D> FastImage for &'a FastImageData<D>
where
    D: PixelType,
{
    type D = D;

    #[inline]
    fn raw_ptr(&self) -> *const Self::D {
        self.data.as_ptr() as *const <FastImageData<D> as FastImage>::D
    }

    #[inline]
    fn stride(&self) -> ipp_ctypes::c_int {
        self.stride
    }

    #[inline]
    fn size(&self) -> &FastImageSize {
        &self.size
    }
}

impl<'a, D> PrivateFastImage for &'a FastImageData<D> where D: PixelType {}

impl<'a, D> FastImage for &'a mut FastImageData<D>
where
    D: PixelType,
{
    type D = D;

    #[inline]
    fn raw_ptr(&self) -> *const Self::D {
        self.data.as_ptr() as *const <FastImageData<D> as FastImage>::D
    }

    #[inline]
    fn stride(&self) -> ipp_ctypes::c_int {
        self.stride
    }

    #[inline]
    fn size(&self) -> &FastImageSize {
        &self.size
    }
}

impl<D> MutableFastImage for FastImageData<D>
where
    D: PixelType,
{
    #[inline]
    fn raw_mut_ptr(&mut self) -> *mut <FastImageData<D> as FastImage>::D {
        self.data.as_mut_ptr() as *mut <FastImageData<D> as FastImage>::D
    }
}

impl<'a, D> MutableFastImage for &'a mut FastImageData<D>
where
    D: PixelType,
{
    #[inline]
    fn raw_mut_ptr(&mut self) -> *mut <FastImageData<D> as FastImage>::D {
        self.data.as_mut_ptr() as *mut <FastImageData<D> as FastImage>::D
    }
}

// ------------------------------
// FastImageView
// ------------------------------

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

impl<'a, D> FastImage for FastImageView<'a, D>
where
    D: PixelType,
{
    type D = D;

    #[inline]
    fn raw_ptr(&self) -> *const Self::D {
        self.data.as_ptr()
    }

    #[inline]
    fn stride(&self) -> ipp_ctypes::c_int {
        self.stride
    }

    #[inline]
    fn size(&self) -> &FastImageSize {
        &self.size
    }
}

impl<'a, D> std::fmt::Debug for FastImageView<'a, D>
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
        for row in 0..self.height() as usize {
            writeln!(fmt, "  row {} slice: {:?}", row, self.row_slice(row))?;
        }
        Ok(())
    }
}

// ------------------------------
// MutableFastImageView
// ------------------------------

pub struct MutableFastImageView<'a, D>
where
    D: 'static + Copy,
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

impl<'a, D> FastImage for MutableFastImageView<'a, D>
where
    D: PixelType,
{
    type D = D;

    #[inline]
    fn raw_ptr(&self) -> *const Self::D {
        self.data.as_ptr()
    }

    #[inline]
    fn stride(&self) -> ipp_ctypes::c_int {
        self.stride
    }

    #[inline]
    fn size(&self) -> &FastImageSize {
        &self.size
    }
}

impl<'a, D> MutableFastImage for MutableFastImageView<'a, D>
where
    D: PixelType,
{
    #[inline]
    fn raw_mut_ptr(&mut self) -> *mut Self::D {
        self.data.as_mut_ptr()
    }
}

impl<'a, D> std::fmt::Debug for MutableFastImageView<'a, D>
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
        for row in 0..self.height() as usize {
            writeln!(fmt, "  row {} slice: {:?}", row, self.row_slice(row))?;
        }
        Ok(())
    }
}

// ------------------------------
// ValidChunksExactMut
// ------------------------------

/// An iterator over strided, mutable data in which only some is "valid".
///
/// This is modeled after [std::slice::ChunksExactMut].
pub struct ValidChunksExactMut<'a, T: 'a> {
    padded_chunk_iter_mut: std::slice::ChunksExactMut<'a, T>,
    valid_n_elements: usize,
}

impl<'a, T> ValidChunksExactMut<'a, T> {
    fn new(slice: &'a mut [T], padded_n_elements: usize, valid_n_elements: usize) -> Self {
        assert!(valid_n_elements <= padded_n_elements);
        let padded_chunk_iter_mut = slice.chunks_exact_mut(padded_n_elements);
        Self {
            padded_chunk_iter_mut,
            valid_n_elements,
        }
    }
}

impl<'a, T> Iterator for ValidChunksExactMut<'a, T> {
    type Item = &'a mut [T];
    fn next(&mut self) -> Option<<Self as Iterator>::Item> {
        self.padded_chunk_iter_mut
            .next()
            .map(|padded| &mut padded[0..self.valid_n_elements])
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

pub trait FastImage {
    type D: PixelType;

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
    fn size(&self) -> &FastImageSize;

    /// Get the raw data for the entire image, including padding.
    #[inline]
    fn image_slice(&self) -> &[Self::D] {
        let n_elements =
            (self.stride() as usize * self.height() as usize * 1) / std::mem::size_of::<Self::D>();
        unsafe { std::slice::from_raw_parts(self.raw_ptr(), n_elements) }
    }

    /// Get the raw data for an image row (does not include padding).
    #[inline]
    fn row_slice(&self, row: usize) -> &[Self::D] {
        if row >= self.height() as usize {
            panic!("out of bounds");
        }
        let row_start = row * self.stride() as usize; // bytes to start of row
        let raw_bytes_ptr = self.raw_ptr() as *const u8; // raw byte pointer
                                                         // Get pointer of type <Self::D> to start of row.
        let row_start_ptr = unsafe { raw_bytes_ptr.add(row_start) } as *const Self::D;
        // Make a slice of it.
        unsafe { std::slice::from_raw_parts(row_start_ptr, (self.width() * 1) as usize) }
    }

    /// Get the raw data for a pixel.
    #[inline]
    fn pixel_slice(&self, row: usize, col: usize) -> &[Self::D] {
        let row = self.row_slice(row);
        let chan = 1;
        let start = col * chan;
        &row[start..start + chan]
    }

    /// Check if self has same size and values as other image.
    fn all_equal<O>(&self, other: O) -> bool
    where
        O: FastImage<D = Self::D>,
    {
        if self.size() != other.size() {
            return false;
        }
        // check row-by row
        for row in 0..(self.height() as usize) {
            let self_row = self.row_slice(row);
            let other_row = other.row_slice(row);
            if self_row != other_row {
                return false;
            }
        }
        true
    }
}

trait PrivateFastImage: FastImage {
    /// Iterate over elements in each image row. Returns valid slices.
    #[inline]
    fn valid_row_iter(&self, size: &FastImageSize) -> Result<ValidChunksExact<'_, Self::D>> {
        if size.width() > self.size().width() || size.height() > self.size().height() {
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
}

pub trait MutableFastImage: FastImage {
    fn raw_mut_ptr(&mut self) -> *mut Self::D;

    /// Get the mutable raw data for the entire image, including padding.
    #[inline]
    fn image_slice_mut(&mut self) -> &mut [Self::D] {
        let n_elements = (self.stride() * self.height()) as usize / std::mem::size_of::<Self::D>();
        unsafe { std::slice::from_raw_parts_mut(self.raw_mut_ptr(), n_elements) }
    }

    /// Iterate over elements in each image row. Returns mutable valid slices.
    #[inline]
    fn valid_row_iter_mut(
        &mut self,
        size: &FastImageSize,
    ) -> Result<ValidChunksExactMut<'_, Self::D>> {
        if size.width() > self.size().width() || size.height() > self.size().height() {
            return Err(Error::SizeError);
        }
        let stride_n_pixels = self.stride() as usize / std::mem::size_of::<Self::D>();
        let pixel_width = size.width() as usize;
        let max_n_pixels = stride_n_pixels * size.height() as usize;
        Ok(ValidChunksExactMut::new(
            &mut self.image_slice_mut()[..max_n_pixels],
            stride_n_pixels,
            pixel_width,
        ))
    }

    #[inline]
    fn row_slice_mut(&mut self, row: usize) -> &mut [Self::D] {
        if row >= self.height() as usize {
            panic!("out of bounds");
        }
        let row_start = row * self.stride() as usize; // bytes to start of row
        let raw_bytes_ptr = self.raw_mut_ptr() as *mut u8; // raw byte pointer
                                                           // Get pointer of type <Self::D> to start of row.
        let row_start_ptr = unsafe { raw_bytes_ptr.add(row_start) } as *mut Self::D;
        // Make a mutable slice of it.
        unsafe { std::slice::from_raw_parts_mut(row_start_ptr, self.width() as usize) }
    }

    #[inline]
    fn pixel_slice_mut(&mut self, row: usize, col: usize) -> &mut [Self::D] {
        let row_slice = self.row_slice_mut(row);
        let chan = 1;
        let start = col * chan;
        &mut row_slice[start..start + chan]
    }
}

// // Print the raw memory values of a FastImage.
// macro_rules! print_mem {
//     ($dest: expr, $size: expr) => {{
//         unsafe {
//             let pre_ptr = $dest.raw_ptr() as *const u8;
//             for i in 0..$size.height() {
//                 let row_ptr = pre_ptr.offset( (i*$dest.stride()) as isize );
//                 for j in 0..$dest.stride() as isize {
//                     print!("{} ",*row_ptr.offset(j));
//                 }
//                 println!("");
//             }
//         }
//     }}
// }

/// Size (in pixels) of a region
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct FastImageSize {
    inner: ipp_sys::IppiSize,
}

impl FastImageSize {
    pub fn new(width: ipp_ctypes::c_int, height: ipp_ctypes::c_int) -> FastImageSize {
        FastImageSize {
            inner: ipp_sys::IppiSize { width, height },
        }
    }
    #[inline]
    pub fn width(&self) -> ipp_ctypes::c_int {
        self.inner.width
    }
    #[inline]
    pub fn height(&self) -> ipp_ctypes::c_int {
        self.inner.height
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
    pub fn size(&self) -> &FastImageSize {
        &self.size
    }
}

#[derive(Debug, Clone)]
pub struct Point {
    inner: ipp_sys::IppiPoint,
}

impl Point {
    #[inline]
    pub fn new(x: ipp_ctypes::c_int, y: ipp_ctypes::c_int) -> Self {
        Self {
            inner: ipp_sys::IppiPoint { x, y },
        }
    }
    #[inline]
    pub fn x(&self) -> ipp_ctypes::c_int {
        self.inner.x
    }
    #[inline]
    pub fn y(&self) -> ipp_ctypes::c_int {
        self.inner.y
    }
}

#[derive(Clone, Copy, Debug)]
pub enum RoundMode {
    Zero,
    Near,
    Financial,
    HintAccurate,
}

fn round_mode_to_ipp(round_mode: RoundMode) -> ipp_sys::IppRoundMode::Type {
    match round_mode {
        RoundMode::Zero => ipp_sys::IppRoundMode::ippRndZero,
        RoundMode::Near => ipp_sys::IppRoundMode::ippRndNear,
        RoundMode::Financial => ipp_sys::IppRoundMode::ippRndFinancial,
        RoundMode::HintAccurate => ipp_sys::IppRoundMode::ippRndHintAccurate,
    }
}

macro_rules! version_assert {
    ($compiled:expr, $runtime:expr, $level:expr) => {{
        if $compiled != $runtime {
            return Err(Error::MismatchedCompileRuntimeVersions(
                $compiled, $runtime, $level,
            ));
        }
    }};
}

pub mod ripp {
    use super::*;

    pub fn init() -> Result<()> {
        itry!(ipp_sys::ippInit());
        // check that compile-time headers match runtime version
        let version = IppVersion::new();
        version_assert!(
            ipp_sys::IPP_VERSION_MAJOR as ipp_ctypes::c_int,
            version.major(),
            "major"
        );
        version_assert!(
            ipp_sys::IPP_VERSION_MINOR as ipp_ctypes::c_int,
            version.minor(),
            "minor"
        );
        // version_assert!(ipp_sys::IPP_VERSION_UPDATE as ipp_ctypes::c_int, version.major_build(), "build");
        Ok(())
    }

    pub fn copy_8u_c1r<S, D>(src: &S, dest: &mut D, size: &FastImageSize) -> Result<()>
    where
        S: FastImage<D = u8>,
        D: MutableFastImage<D = u8>,
    {
        itry!(ipp_sys::ippiCopy_8u_C1R(
            src.raw_ptr(),
            src.stride(),
            dest.raw_mut_ptr(),
            dest.stride(),
            size.inner
        ));
        Ok(())
    }

    pub fn copy_32f_c1r<S, D>(src: &S, dest: &mut D, size: &FastImageSize) -> Result<()>
    where
        S: FastImage<D = f32>,
        D: MutableFastImage<D = f32>,
    {
        itry!(ipp_sys::ippiCopy_32f_C1R(
            src.raw_ptr(),
            src.stride(),
            dest.raw_mut_ptr(),
            dest.stride(),
            size.inner
        ));
        Ok(())
    }

    pub fn convert_8u32f_c1r<S, D>(src: &S, dest: &mut D, size: &FastImageSize) -> Result<()>
    where
        S: FastImage<D = u8>,
        D: MutableFastImage<D = f32>,
    {
        itry!(ipp_sys::ippiConvert_8u32f_C1R(
            src.raw_ptr(),
            src.stride(),
            dest.raw_mut_ptr(),
            dest.stride(),
            size.inner
        ));
        Ok(())
    }

    pub fn convert_32f8u_c1r<S, D>(
        src: &S,
        dest: &mut D,
        size: &FastImageSize,
        round_mode: RoundMode,
    ) -> Result<()>
    where
        S: FastImage<D = f32>,
        D: MutableFastImage<D = u8>,
    {
        itry!(ipp_sys::ippiConvert_32f8u_C1R(
            src.raw_ptr(),
            src.stride(),
            dest.raw_mut_ptr(),
            dest.stride(),
            size.inner,
            round_mode_to_ipp(round_mode)
        ));
        Ok(())
    }

    pub fn compare_c_8u_c1r<S, D>(
        src: &S,
        value: u8,
        dest: &mut D,
        size: &FastImageSize,
        cmp_op: CompareOp,
    ) -> Result<()>
    where
        S: FastImage<D = u8>,
        D: MutableFastImage<D = u8>,
    {
        itry!(ipp_sys::ippiCompareC_8u_C1R(
            src.raw_ptr(),
            src.stride(),
            value,
            dest.raw_mut_ptr(),
            dest.stride(),
            size.inner,
            get_compare_op(cmp_op)
        ));
        Ok(())
    }

    pub fn min_indx_8u_c1r<S>(src: &S, size: &FastImageSize) -> Result<(u8, Point)>
    where
        S: FastImage<D = u8>,
    {
        let mut value = 0;
        let mut loc = Point::new(-1, -1);

        itry!(ipp_sys::ippiMinIndx_8u_C1R(
            src.raw_ptr(),
            src.stride(),
            size.inner,
            &mut value,
            &mut loc.inner.x,
            &mut loc.inner.y,
        ));
        Ok((value, loc))
    }

    pub fn max_indx_8u_c1r<S>(src: &S, size: &FastImageSize) -> Result<(u8, Point)>
    where
        S: FastImage<D = u8>,
    {
        let mut value = 0;
        let mut loc = Point::new(-1, -1);

        itry!(ipp_sys::ippiMaxIndx_8u_C1R(
            src.raw_ptr(),
            src.stride(),
            size.inner,
            &mut value,
            &mut loc.inner.x,
            &mut loc.inner.y,
        ));
        Ok((value, loc))
    }

    pub fn threshold_val_8u_c1ir<SD>(
        src_dest: &mut SD,
        size: &FastImageSize,
        threshold: u8,
        value: u8,
        cmp_op: CompareOp,
    ) -> Result<()>
    where
        SD: MutableFastImage<D = u8>,
    {
        itry!(ipp_sys::ippiThreshold_Val_8u_C1IR(
            src_dest.raw_mut_ptr(),
            src_dest.stride(),
            size.inner,
            threshold,
            value,
            get_compare_op(cmp_op)
        ));
        Ok(())
    }

    /// Subtract `src1` from `src2` and put results in `dest`.
    /// In other words, `dest = src2 - src` for each pixel.
    pub fn sub_8u_c1rsfs<S1, S2, D>(
        src1: &S1,
        src2: &S2,
        dest: &mut D,
        size: &FastImageSize,
        scale_factor: ipp_ctypes::c_int,
    ) -> Result<()>
    where
        S1: FastImage<D = u8>,
        S2: FastImage<D = u8>,
        D: MutableFastImage<D = u8>,
    {
        itry!(ipp_sys::ippiSub_8u_C1RSfs(
            src1.raw_ptr(),
            src1.stride(),
            src2.raw_ptr(),
            src2.stride(),
            dest.raw_mut_ptr(),
            dest.stride(),
            size.inner,
            scale_factor
        ));
        Ok(())
    }

    /// Subtract `src1` from `src2` and put results in `dest`.
    /// In other words, `dest = src2 - src` for each pixel.
    pub fn sub_32f_c1r<S1, S2, D>(
        src1: &S1,
        src2: &S2,
        dest: &mut D,
        size: &FastImageSize,
    ) -> Result<()>
    where
        S1: FastImage<D = f32>,
        S2: FastImage<D = f32>,
        D: MutableFastImage<D = f32>,
    {
        itry!(ipp_sys::ippiSub_32f_C1R(
            src1.raw_ptr(),
            src1.stride(),
            src2.raw_ptr(),
            src2.stride(),
            dest.raw_mut_ptr(),
            dest.stride(),
            size.inner
        ));
        Ok(())
    }

    pub fn abs_32f_c1r<S, D>(src: &S, dest: &mut D, size: &FastImageSize) -> Result<()>
    where
        S: FastImage<D = f32>,
        D: MutableFastImage<D = f32>,
    {
        itry!(ipp_sys::ippiAbs_32f_C1R(
            src.raw_ptr(),
            src.stride(),
            dest.raw_mut_ptr(),
            dest.stride(),
            size.inner
        ));
        Ok(())
    }

    pub fn sqrt_32f_c1ir<SD>(src_dest: &mut SD, size: &FastImageSize) -> Result<()>
    where
        SD: MutableFastImage<D = f32>,
    {
        itry!(ipp_sys::ippiSqrt_32f_C1IR(
            src_dest.raw_mut_ptr(),
            src_dest.stride(),
            size.inner
        ));
        Ok(())
    }

    pub fn mul_c_32f_c1ir<SD>(k: f32, src_dest: &mut SD, size: &FastImageSize) -> Result<()>
    where
        SD: MutableFastImage<D = f32>,
    {
        itry!(ipp_sys::ippiMulC_32f_C1IR(
            k,
            src_dest.raw_mut_ptr(),
            src_dest.stride(),
            size.inner
        ));
        Ok(())
    }

    pub fn abs_diff_8u_c1r<S1, S2, D>(
        src1: &S1,
        src2: &S2,
        dest: &mut D,
        size: &FastImageSize,
    ) -> Result<()>
    where
        S1: FastImage<D = u8>,
        S2: FastImage<D = u8>,
        D: MutableFastImage<D = u8>,
    {
        itry!(ipp_sys::ippiAbsDiff_8u_C1R(
            src1.raw_ptr(),
            src1.stride(),
            src2.raw_ptr(),
            src2.stride(),
            dest.raw_mut_ptr(),
            dest.stride(),
            size.inner
        ));
        Ok(())
    }

    pub fn add_weighted_8u32f_c1ir<S, D>(
        src: &S,
        src_dst: &mut D,
        size: &FastImageSize,
        alpha: f32,
    ) -> Result<()>
    where
        S: FastImage<D = u8>,
        D: MutableFastImage<D = f32>,
    {
        itry!(ipp_sys::ippiAddWeighted_8u32f_C1IR(
            src.raw_ptr(),
            src.stride(),
            src_dst.raw_mut_ptr(),
            src_dst.stride(),
            size.inner,
            alpha
        ));
        Ok(())
    }

    pub fn add_weighted_32f_c1ir<S, D>(
        src: &S,
        src_dst: &mut D,
        size: &FastImageSize,
        alpha: f32,
    ) -> Result<()>
    where
        S: FastImage<D = f32>,
        D: MutableFastImage<D = f32>,
    {
        itry!(ipp_sys::ippiAddWeighted_32f_C1IR(
            src.raw_ptr(),
            src.stride(),
            src_dst.raw_mut_ptr(),
            src_dst.stride(),
            size.inner,
            alpha
        ));
        Ok(())
    }

    pub fn moments_8u_c1r<S>(src: &S, size: &FastImageSize, result: &mut MomentState) -> Result<()>
    where
        S: FastImage<D = u8>,
    {
        itry!(ipp_sys::ippiMoments64f_8u_C1R(
            src.raw_ptr(),
            src.stride(),
            size.inner,
            result.as_mut_ptr()
        ));
        result.valid = true;
        Ok(())
    }

    pub fn set_8u_c1r<D>(value: u8, dest: &mut D, size: &FastImageSize) -> Result<()>
    where
        D: MutableFastImage<D = u8>,
    {
        itry!(ipp_sys::ippiSet_8u_C1R(
            value,
            dest.raw_mut_ptr(),
            dest.stride(),
            size.inner
        ));
        Ok(())
    }

    pub fn set_32f_c1r<D>(value: f32, dest: &mut D, size: &FastImageSize) -> Result<()>
    where
        D: MutableFastImage<D = f32>,
    {
        itry!(ipp_sys::ippiSet_32f_C1R(
            value,
            dest.raw_mut_ptr(),
            dest.stride(),
            size.inner
        ));
        Ok(())
    }

    pub fn set_8u_c1mr<D, M>(value: u8, dest: &mut D, size: &FastImageSize, mask: &M) -> Result<()>
    where
        D: MutableFastImage<D = u8>,
        M: FastImage<D = u8>,
    {
        itry!(ipp_sys::ippiSet_8u_C1MR(
            value,
            dest.raw_mut_ptr(),
            dest.stride(),
            size.inner,
            mask.raw_ptr(),
            mask.stride()
        ));
        Ok(())
    }

    pub fn sqr_32f_c1ir<SD>(src_dest: &mut SD, size: &FastImageSize) -> Result<()>
    where
        SD: MutableFastImage<D = f32>,
    {
        itry!(ipp_sys::ippiSqr_32f_C1IR(
            src_dest.raw_mut_ptr(),
            src_dest.stride(),
            size.inner
        ));
        Ok(())
    }
}

#[derive(Copy, Clone, Debug)]
pub enum AlgorithmHint {
    NoHint,
    Fast,
    Accurate,
}

#[derive(Copy, Clone, Debug)]
pub enum CompareOp {
    Less,
    LessEqual,
    Equal,
    GreaterEqual,
    Greater,
}

#[inline]
fn hint_to_ipp(hint: AlgorithmHint) -> ipp_sys::IppHintAlgorithm::Type {
    match hint {
        AlgorithmHint::NoHint => ipp_sys::IppHintAlgorithm::ippAlgHintNone,
        AlgorithmHint::Fast => ipp_sys::IppHintAlgorithm::ippAlgHintFast,
        AlgorithmHint::Accurate => ipp_sys::IppHintAlgorithm::ippAlgHintAccurate,
    }
}

#[inline]
fn get_compare_op(cmp: CompareOp) -> ipp_sys::IppCmpOp::Type {
    match cmp {
        CompareOp::Less => ipp_sys::IppCmpOp::ippCmpLess,
        CompareOp::LessEqual => ipp_sys::IppCmpOp::ippCmpLessEq,
        CompareOp::Equal => ipp_sys::IppCmpOp::ippCmpEq,
        CompareOp::GreaterEqual => ipp_sys::IppCmpOp::ippCmpGreaterEq,
        CompareOp::Greater => ipp_sys::IppCmpOp::ippCmpGreater,
    }
}

pub struct MomentState {
    data: Box<[u8]>,
    valid: bool,
}

impl MomentState {
    pub fn new(hint_algorithm: AlgorithmHint) -> Result<MomentState> {
        let mut size = -1;
        itry!(ipp_sys::ippiMomentGetStateSize_64f(
            hint_to_ipp(hint_algorithm),
            &mut size
        ));
        let mut data = vec![0; size as usize].into_boxed_slice();
        itry!(ipp_sys::ippiMomentInit_64f(
            data.as_mut_ptr() as *mut ipp_sys::MomentState64f,
            hint_to_ipp(hint_algorithm)
        ));
        Ok(MomentState { data, valid: false })
    }
    fn as_mut_ptr(&mut self) -> *mut ipp_sys::MomentState64f {
        self.data.as_mut_ptr() as *mut ipp_sys::MomentState64f
    }
    fn as_ptr(&self) -> *const ipp_sys::MomentState64f {
        self.data.as_ptr() as *const ipp_sys::MomentState64f
    }
    pub fn spatial(
        &self,
        m_ord: ipp_ctypes::c_int,
        n_ord: ipp_ctypes::c_int,
        n_channel: ipp_ctypes::c_int,
        roi_offset: &Point,
    ) -> Result<f64> {
        if !self.valid {
            return Err(Error::MomentStateNotInitialized);
        }
        let mut result = 0.0;
        itry!(ipp_sys::ippiGetSpatialMoment_64f(
            self.as_ptr(),
            m_ord,
            n_ord,
            n_channel,
            roi_offset.inner,
            &mut result
        ));
        Ok(result)
    }
    pub fn central(
        &self,
        m_ord: ipp_ctypes::c_int,
        n_ord: ipp_ctypes::c_int,
        n_channel: ipp_ctypes::c_int,
    ) -> Result<f64> {
        if !self.valid {
            return Err(Error::MomentStateNotInitialized);
        }
        let mut result = 0.0;
        itry!(ipp_sys::ippiGetCentralMoment_64f(
            self.as_ptr(),
            m_ord,
            n_ord,
            n_channel,
            &mut result
        ));
        Ok(result)
    }
}

pub struct IppVersion {
    version: *const ipp_sys::IppLibraryVersion,
}

impl Default for IppVersion {
    fn default() -> Self {
        Self::new()
    }
}

impl IppVersion {
    pub fn new() -> IppVersion {
        let mut version: *const ipp_sys::IppLibraryVersion = std::ptr::null_mut();
        assert!(version.is_null());
        unsafe {
            version = ipp_sys::ippGetLibVersion();
        }
        assert!(!version.is_null());
        IppVersion { version }
    }

    pub fn major(&self) -> ipp_ctypes::c_int {
        let inner = unsafe { *self.version };
        inner.major
    }

    pub fn minor(&self) -> ipp_ctypes::c_int {
        let inner = unsafe { *self.version };
        inner.minor
    }

    pub fn major_build(&self) -> ipp_ctypes::c_int {
        let inner = unsafe { *self.version };
        inner.majorBuild
    }

    pub fn build(&self) -> ipp_ctypes::c_int {
        let inner = unsafe { *self.version };
        inner.build
    }

    pub fn name(&self) -> &str {
        let inner = unsafe { *self.version };
        let slice = unsafe { std::ffi::CStr::from_ptr(inner.Name) };
        slice.to_str().unwrap()
    }

    pub fn version(&self) -> &str {
        let inner = unsafe { *self.version };
        let slice = unsafe { std::ffi::CStr::from_ptr(inner.Version) };
        slice.to_str().unwrap()
    }

    pub fn build_date(&self) -> &str {
        let inner = unsafe { *self.version };
        let slice = unsafe { std::ffi::CStr::from_ptr(inner.BuildDate) };
        slice.to_str().unwrap()
    }
}

impl std::fmt::Debug for IppVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let inner: &ipp_sys::IppLibraryVersion = unsafe { &*self.version };
        std::fmt::Debug::fmt(inner, f)
    }
}

// ----

/// Check if two FastImages have same size and values.
fn fi_equal<D, SRC1, SRC2>(self_: SRC1, other: SRC2) -> bool
where
    D: std::cmp::PartialEq,
    SRC1: FastImage<D = D> + PrivateFastImage,
    SRC2: FastImage<D = D> + PrivateFastImage,
{
    if self_.size() != other.size() {
        return false;
    }
    // check row-by row
    for (self_row, other_row) in self_
        .valid_row_iter(&self_.size())
        .unwrap()
        .zip(other.valid_row_iter(&self_.size()).unwrap())
    {
        if self_row != other_row {
            return false;
        }
    }
    true
}

// ------------------------------
// ValidChunksExact
// ------------------------------

/// An iterator over strided data in which only some is "valid".
///
/// This is modeled after [std::slice::ChunksExact].
struct ValidChunksExact<'a, T: 'a> {
    padded_chunk_iter: std::slice::ChunksExact<'a, T>,
    valid_n_elements: usize,
}

impl<'a, T> ValidChunksExact<'a, T> {
    fn new(slice: &'a [T], row_stride_n_elements: usize, valid_n_elements: usize) -> Self {
        assert!(valid_n_elements <= row_stride_n_elements);
        let padded_chunk_iter = slice.chunks_exact(row_stride_n_elements);
        Self {
            padded_chunk_iter,
            valid_n_elements,
        }
    }
}

impl<'a, T> Iterator for ValidChunksExact<'a, T> {
    type Item = &'a [T];
    fn next(&mut self) -> Option<<Self as Iterator>::Item> {
        self.padded_chunk_iter
            .next()
            .map(|padded| &padded[0..self.valid_n_elements])
    }
}

#[test]
fn test_padded_chunks() {
    {
        // f32
        let avec = vec![1.0, 2.0, 3.0, 4.0, -1.0, 1.1, 2.1, 3.1, 4.1, -1.0];
        let a1: &[f32] = avec.as_slice();

        let mut myiter = ValidChunksExact::new(&a1, 5, 4);
        assert_eq!(myiter.next(), Some(&avec[0..4]));
        assert_eq!(myiter.next(), Some(&avec[5..9]));
        assert_eq!(myiter.next(), None);
    }

    {
        // u8
        let avec = vec![10, 20, 30, 40, 255, 11, 21, 31, 41, 25];
        let a1: &[u8] = avec.as_slice();

        let mut myiter = ValidChunksExact::new(&a1, 5, 4);
        assert_eq!(myiter.next(), Some(&avec[0..4]));
        assert_eq!(myiter.next(), Some(&avec[5..9]));
        assert_eq!(myiter.next(), None);
    }
}
