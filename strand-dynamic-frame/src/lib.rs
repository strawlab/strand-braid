// Copyright 2016-2025 Andrew D. Straw.
//
// Licensed under the Apache License, Version 2.0
// <http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! Images from machine vision cameras used in [Strand
//! Camera](https://strawlab.org/strand-cam).
//!
//! Building on the [`machine_vision_formats`] crate which provides compile-time
//! pixel formats, this crate provides types for images whose pixel format is
//! determined at runtime. This allows for flexibility in handling images data
//! whose pixel format is known only dynamically, such as when reading an image
//! from disk.
//!
//! There are two types here:
//! - [`DynamicFrame`]: A borrowed view of an image with a dynamic pixel format.
//! - [`DynamicFrameOwned`]: An owned version of `DynamicFrame` that contains
//!   its own buffer.
//!
//! When compiled with the `convert-image` feature, this crate also provides
//! conversion methods to convert the dynamic frame into a static pixel format
//! using the [`convert_image`](https://docs.rs/convert-image) crate.

#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

use std::borrow::Cow;

use machine_vision_formats as formats;

use formats::{image_ref::ImageRef, ImageStride, PixFmt, PixelFormat, Stride};

#[cfg(feature = "convert-image")]
use formats::{cow::CowImage, owned::OImage};

// TODO: investigate if we can implement std::borrow::Borrow<DynamicFrame> for
// DynamicFrameOwned. I think not due to the issues
// [here](https://users.rust-lang.org/t/how-to-implement-borrow-for-my-own-struct/73023).

#[macro_export]
/// Macro to match all dynamic pixel formats and execute a block of code with a typed image reference.
macro_rules! match_all_dynamic_fmts {
    ($self:expr, $x:ident, $block:expr, $err:expr) => {{
        use machine_vision_formats::{
            pixel_format::{
                BayerBG32f, BayerBG8, BayerGB32f, BayerGB8, BayerGR32f, BayerGR8, BayerRG32f,
                BayerRG8, Mono32f, Mono8, NV12, RGB8, RGBA8, YUV422, YUV444,
            },
            PixFmt,
        };
        match $self.pixel_format() {
            PixFmt::Mono8 => {
                let $x = $self.as_static::<Mono8>().unwrap();
                $block
            }
            PixFmt::Mono32f => {
                let $x = $self.as_static::<Mono32f>().unwrap();
                $block
            }
            PixFmt::RGB8 => {
                let $x = $self.as_static::<RGB8>().unwrap();
                $block
            }
            PixFmt::RGBA8 => {
                let $x = $self.as_static::<RGBA8>().unwrap();
                $block
            }
            PixFmt::BayerRG8 => {
                let $x = $self.as_static::<BayerRG8>().unwrap();
                $block
            }
            PixFmt::BayerRG32f => {
                let $x = $self.as_static::<BayerRG32f>().unwrap();
                $block
            }
            PixFmt::BayerBG8 => {
                let $x = $self.as_static::<BayerBG8>().unwrap();
                $block
            }
            PixFmt::BayerBG32f => {
                let $x = $self.as_static::<BayerBG32f>().unwrap();
                $block
            }
            PixFmt::BayerGB8 => {
                let $x = $self.as_static::<BayerGB8>().unwrap();
                $block
            }
            PixFmt::BayerGB32f => {
                let $x = $self.as_static::<BayerGB32f>().unwrap();
                $block
            }
            PixFmt::BayerGR8 => {
                let $x = $self.as_static::<BayerGR8>().unwrap();
                $block
            }
            PixFmt::BayerGR32f => {
                let $x = $self.as_static::<BayerGR32f>().unwrap();
                $block
            }
            PixFmt::YUV444 => {
                let $x = $self.as_static::<YUV444>().unwrap();
                $block
            }
            PixFmt::YUV422 => {
                let $x = $self.as_static::<YUV422>().unwrap();
                $block
            }
            PixFmt::NV12 => {
                let $x = $self.as_static::<NV12>().unwrap();
                $block
            }
            _ => {
                return Err($err);
            }
        }
    }};
}

#[inline]
const fn calc_min_stride(w: u32, pixfmt: PixFmt) -> usize {
    w as usize * pixfmt.bits_per_pixel() as usize / 8
}

#[inline]
const fn calc_min_buf_size(w: u32, h: u32, stride: usize, pixfmt: PixFmt) -> usize {
    if h == 0 {
        return 0;
    }
    let all_but_last = (h - 1) as usize * stride;
    let last = calc_min_stride(w, pixfmt);
    debug_assert!(stride >= last);
    all_but_last + last
}

/// An image whose pixel format is determined at runtime.
///
/// This type is used to represent images where the pixel format is not known at
/// compile time, allowing for flexibility in handling various image formats.
///
/// It can be created from raw image data and provides methods to access the
/// image dimensions, pixel format, and raw data. It also supports conversion to
/// static pixel formats and encoding to various formats.
///
/// # Type Parameters
/// * `'a` - Lifetime of the borrowed data. If you want to own the data, use
///   [`DynamicFrameOwned`].
/// # Notes
/// * This type is not `Sync` or `Send` because it contains a borrowed buffer.
///   If you need to share it across threads, use [`DynamicFrameOwned`] instead.
/// * The pixel format is represented by the [`PixFmt`] enum, which allows for
///   various pixel formats like `Mono8`, `RGB8`, etc.
/// * The buffer must be large enough to hold the image data for the specified
///   dimensions and pixel format.
///
/// # Examples
/// ```rust
/// # use strand_dynamic_frame::DynamicFrame;
/// # use machine_vision_formats::PixFmt;
/// // Create a frame from raw data
/// let data = vec![0u8; 1920 * 1080];
/// let frame = DynamicFrame::from_buf(1920, 1080, 1920, data, PixFmt::Mono8).unwrap();
///
/// // Check the pixel format
/// assert_eq!(frame.pixel_format(), PixFmt::Mono8);
///
/// // Get dimensions
/// println!("Size: {}x{}", frame.width(), frame.height());
/// ```
pub struct DynamicFrame<'a> {
    width: u32,
    height: u32,
    stride: usize,
    buf: Cow<'a, [u8]>,
    pixfmt: PixFmt,
}

/// An owned version of [`DynamicFrame`] that contains its own buffer.
#[derive(Clone)]
pub struct DynamicFrameOwned {
    width: u32,
    height: u32,
    stride: usize,
    pixfmt: PixFmt,
    buf: Vec<u8>,
}

impl std::fmt::Debug for DynamicFrameOwned {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        f.debug_struct("DynamicFrameOwned")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("stride", &self.stride)
            .field("pixfmt", &self.pixfmt)
            .finish_non_exhaustive()
    }
}

impl Stride for DynamicFrameOwned {
    fn stride(&self) -> usize {
        self.stride
    }
}

impl DynamicFrameOwned {
    /// Return a new [`DynamicFrameOwned`] from a statically typed frame. This
    /// moves the input data.
    pub fn from_static<FRAME, FMT>(frame: FRAME) -> Self
    where
        FRAME: ImageStride<FMT> + Into<Vec<u8>>,
        FMT: PixelFormat,
    {
        let pixfmt = formats::pixel_format::pixfmt::<FMT>().unwrap();
        let width = frame.width();
        let height = frame.height();
        let stride = frame.stride();
        let min_size = calc_min_buf_size(width, height, stride, pixfmt);
        let mut buf: Vec<u8> = frame.into();
        buf.truncate(min_size);
        Self {
            width,
            height,
            stride,
            pixfmt,
            buf,
        }
    }

    /// Return a new [`DynamicFrameOwned`] from a reference to a statically
    /// typed frame. This copies the input data.
    pub fn from_static_ref<FMT: PixelFormat>(frame: &dyn ImageStride<FMT>) -> Self {
        let pixfmt = formats::pixel_format::pixfmt::<FMT>().unwrap();
        let image_data = frame.image_data();
        let min_size = calc_min_buf_size(frame.width(), frame.height(), frame.stride(), pixfmt);
        Self {
            width: frame.width(),
            height: frame.height(),
            stride: frame.stride(),
            buf: image_data[..min_size].to_vec(),
            pixfmt,
        }
    }

    /// Creates a new [`DynamicFrameOwned`] from raw image data.
    ///
    /// This function moves the provided buffer into the new frame without
    /// copying. The buffer size must be appropriate for the given dimensions
    /// and pixel format.
    ///
    /// # Parameters
    /// * `w` - Image width in pixels
    /// * `h` - Image height in pixels
    /// * `s` - Row stride in bytes (must be >= width * `bytes_per_pixel`)
    /// * `buf` - Raw image data buffer
    /// * `pixfmt` - Pixel format of the image data
    ///
    /// # Returns
    /// * `Some(DynamicFrameOwned)` if the buffer is valid for the given parameters
    /// * `None` if the buffer is too small.
    #[must_use]
    pub fn from_buf(w: u32, h: u32, stride: usize, buf: Vec<u8>, pixfmt: PixFmt) -> Option<Self> {
        let min_size = calc_min_buf_size(w, h, stride, pixfmt);
        if buf.len() < min_size {
            return None; // Buffer too small
        }
        Some(Self {
            width: w,
            height: h,
            stride,
            buf,
            pixfmt,
        })
    }

    /// Return a borrowed view of this frame as a [`DynamicFrame`].
    #[must_use]
    pub fn borrow(&self) -> DynamicFrame<'_> {
        DynamicFrame {
            width: self.width,
            height: self.height,
            stride: self.stride,
            buf: Cow::Borrowed(&self.buf),
            pixfmt: self.pixfmt,
        }
    }

    // /// Return a mutable borrowed view of this frame as a [`DynamicFrame`].
    // pub fn borrow_mut(&mut self) -> DynamicFrame<'_> {
    //     DynamicFrame {
    //         width: self.width,
    //         height: self.height,
    //         stride: self.stride,
    //         buf: Cow::Borrowed(&self.buf),
    //         pixfmt: self.pixfmt,
    //     }
    // }

    /// Moves data into a new [`DynamicFrameOwned`] containing a region of
    /// interest (ROI) within the image without copying.
    ///
    /// The ROI is defined by the specified left, top, width, and height
    /// parameters. If the specified ROI is out of bounds or the buffer is too
    /// small, this method returns `None`.
    ///
    /// # Parameters
    /// * `left` - The left coordinate of the ROI in pixels
    /// * `top` - The top coordinate of the ROI in pixels
    /// * `width` - The width of the ROI in pixels
    /// * `height` - The height of the ROI in pixels
    ///
    /// # Returns
    /// * `Some(DynamicFrameOwned)` if the ROI is valid and the buffer is large
    ///   enough
    /// * `None` if the ROI is out of bounds or the buffer is too small
    ///
    /// To create a view with a ROI, use [`Self::borrow().roi()`].
    #[must_use]
    pub fn roi(self, left: u32, top: u32, width: u32, height: u32) -> Option<DynamicFrameOwned> {
        if left != 0 || top != 0 {
            todo!();
        }
        if left + width > self.width || top + height > self.height {
            return None; // ROI out of bounds
        }
        let stride = self.stride;
        let new_min_size = calc_min_buf_size(width, height, stride, self.pixfmt);
        if self.buf.len() < new_min_size {
            return None; // Buffer too small for ROI
        }
        Some(DynamicFrameOwned {
            width,
            height,
            stride,
            buf: self.buf,
            pixfmt: self.pixfmt,
        })
    }

    /// Moves the `DynamicFrameOwned` into a static pixel format.
    ///
    /// If the requested pixel format matches the current format, this method
    /// returns an [`formats::owned::OImage`] that owns the data without
    /// copying. Otherwise, it returns `None` since the data cannot be moved to
    /// a static format.
    ///
    /// To convert to a static format when the target format may not match the
    /// current format, use [`Self::into_pixel_format<FMT>()`] (requires the
    /// `convert-image` feature).
    ///
    /// # Type Parameters
    /// * `FMT` - The target pixel format type
    ///
    /// # Returns
    /// * `Some(OImage<FMT>)` - If the target format matches the current format,
    ///   returns an owned image in the specified format
    /// * `None` - If the target format does not match the current format
    ///
    #[must_use]
    pub fn as_static<FMT: PixelFormat>(self) -> Option<formats::owned::OImage<FMT>> {
        let pixfmt = formats::pixel_format::pixfmt::<FMT>().unwrap();
        if pixfmt == self.pixfmt {
            // Simply return the image data as a borrowed view
            Some(
                formats::owned::OImage::new(self.width, self.height, self.stride, self.buf)
                    .unwrap(),
            )
        } else {
            // Cannot convert to static format
            None
        }
    }

    #[cfg(feature = "convert-image")]
    /// Converts the image to the specified pixel format, returning an
    /// [`OImage`] that owns the data.
    ///
    /// If the requested pixel format matches the current format, this method
    /// moves the data without copying. Otherwise, the data is converted and a
    /// new owned image is returned. In both cases, the original image data is
    /// consumed.
    ///
    /// To move the data to a specified pixel format while excluding the
    /// possibility of converting the format in case the requested format does
    /// not match the current format, use [`Self::as_static<FMT>()`].
    ///
    /// # Type Parameters
    /// * `FMT` - The target pixel format type
    ///
    /// # Returns
    /// * `Ok(OImage<FMT>)` - If conversion is successful, returns an owned
    ///   image in the specified format
    /// * `Err(convert_image::Error)` - If conversion fails
    ///
    /// # Examples
    /// ```rust
    /// # use strand_dynamic_frame::DynamicFrameOwned;
    /// # use machine_vision_formats::{PixFmt, pixel_format::Mono8};
    /// let data = vec![64u8; 2000];
    /// let frame = DynamicFrameOwned::from_buf(40, 50, 40, data, PixFmt::Mono8).unwrap();
    ///
    /// // No conversion or copying needed - returns original data
    /// let owned_image = frame.into_pixel_format::<Mono8>().unwrap();
    /// ```
    pub fn into_pixel_format<FMT>(self) -> Result<OImage<FMT>, convert_image::Error>
    where
        FMT: PixelFormat,
    {
        let dest_fmt = formats::pixel_format::pixfmt::<FMT>().unwrap();
        let self_ = self.borrow();
        if dest_fmt == self_.pixel_format() {
            // Fast path. Simply return the data.
            Ok(OImage::new(self_.width(), self_.height(), self_.stride(), self.buf).unwrap())
        } else {
            // Conversion path. Allocate a new buffer and convert the data.
            let width = self_.width();
            let dest_stride = calc_min_stride(width, dest_fmt);
            let mut dest = OImage::zeros(width, self_.height(), dest_stride).unwrap();
            self_.into_pixel_format_dest(&mut dest)?;
            Ok(dest)
        }
    }
}

impl<'a> DynamicFrame<'a> {
    /// Return a new [`DynamicFrameOwned`] by copying data.
    #[must_use]
    pub fn copy_to_owned(&self) -> DynamicFrameOwned {
        let pixfmt = self.pixfmt;
        let width = self.width;
        let height = self.height;
        let stride = self.stride;
        let buf = self.buf.to_vec();
        DynamicFrameOwned {
            width,
            height,
            stride,
            pixfmt,
            buf,
        }
    }

    /// Return a new [`DynamicFrame`] from a reference to a statically
    /// typed frame. This does not copy the input data.
    pub fn from_static_ref<FMT: PixelFormat>(frame: &'a dyn ImageStride<FMT>) -> Self {
        let pixfmt = formats::pixel_format::pixfmt::<FMT>().unwrap();
        let image_data = frame.image_data();
        let min_size = calc_min_buf_size(frame.width(), frame.height(), frame.stride(), pixfmt);
        let image_data = &image_data[..min_size];
        Self {
            width: frame.width(),
            height: frame.height(),
            stride: frame.stride(),
            buf: std::borrow::Cow::Borrowed(image_data),
            pixfmt,
        }
    }

    /// Creates a new [`DynamicFrame`] from raw image data.
    ///
    /// This function moves the provided buffer into the new frame without
    /// copying. The buffer size must be appropriate for the given dimensions
    /// and pixel format.
    ///
    /// # Parameters
    /// * `w` - Image width in pixels
    /// * `h` - Image height in pixels
    /// * `s` - Row stride in bytes (must be >= width * `bytes_per_pixel`)
    /// * `buf` - Raw image data buffer
    /// * `pixfmt` - Pixel format of the image data
    ///
    /// # Returns
    /// * `Some(DynamicFrame)` if the buffer is valid for the given parameters
    /// * `None` if the buffer is too small.
    ///
    /// # Examples
    /// ```rust
    /// # use strand_dynamic_frame::DynamicFrame;
    /// # use machine_vision_formats::PixFmt;
    /// let data = vec![128u8; 640 * 480]; // Gray image data
    /// let frame = DynamicFrame::from_buf(640, 480, 640, data, PixFmt::Mono8);
    /// assert!(frame.is_some());
    /// ```
    #[must_use]
    pub fn from_buf(w: u32, h: u32, stride: usize, buf: Vec<u8>, pixfmt: PixFmt) -> Option<Self> {
        let min_size = calc_min_buf_size(w, h, stride, pixfmt);
        if buf.len() < min_size {
            return None; // Buffer too small
        }
        Some(Self {
            width: w,
            height: h,
            stride,
            buf: Cow::Owned(buf),
            pixfmt,
        })
    }

    /// Returns the width of the image in pixels.
    ///
    /// # Examples
    /// ```rust
    /// # use strand_dynamic_frame::DynamicFrame;
    /// # use machine_vision_formats::PixFmt;
    /// let data = vec![0u8; 1500];
    /// let frame = DynamicFrame::from_buf(50, 10, 150, data, PixFmt::RGB8).unwrap();
    /// assert_eq!(frame.width(), 50);
    /// ```
    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Returns the height of the image in pixels.
    ///
    /// # Examples
    /// ```rust
    /// # use strand_dynamic_frame::DynamicFrame;
    /// # use machine_vision_formats::PixFmt;
    /// let data = vec![0u8; 2000];
    /// let frame = DynamicFrame::from_buf(40, 50, 40, data, PixFmt::Mono8).unwrap();
    /// assert_eq!(frame.height(), 50);
    /// ```
    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Returns a view of the raw image data as bytes.
    ///
    /// This method provides access to the underlying pixel data without any
    /// type information about the pixel format. The returned slice contains
    /// the raw bytes that make up the image.
    ///
    /// The data layout depends on the pixel format and stride. Use [`pixel_format()`](Self::pixel_format)
    /// to determine how to interpret the bytes.
    fn minimum_image_data_without_format(&self) -> &[u8] {
        let min_size = calc_min_buf_size(self.width, self.height, self.stride, self.pixfmt);
        &self.buf[..min_size]
    }

    /// Creates a new [`DynamicFrame`] from an existing frame using borrowed data.
    ///
    /// This function copies the image data from the source frame and creates a
    /// new [`DynamicFrame`]. The original frame remains unchanged.
    ///
    /// # Type Parameters
    /// * `FMT` - The pixel format type of the source frame
    ///
    /// # Parameters
    /// * `frame` - Reference to the source frame implementing [`ImageStride`]
    ///
    /// # Examples
    /// ```rust
    /// # use strand_dynamic_frame::DynamicFrame;
    /// # use machine_vision_formats::owned::OImage;
    /// # use machine_vision_formats::pixel_format::Mono8;
    /// let source = OImage::<Mono8>::new(100, 100, 100, vec![0u8; 10000]).unwrap();
    /// let dynamic_frame = DynamicFrame::copy_from(&source);
    /// assert_eq!(dynamic_frame.width(), 100);
    /// ```
    pub fn copy_from<FMT: PixelFormat>(frame: &'a dyn ImageStride<FMT>) -> Self {
        let width = frame.width();
        let height = frame.height();
        let stride = frame.stride();
        let pixfmt = formats::pixel_format::pixfmt::<FMT>().unwrap();
        let min_size = calc_min_buf_size(width, height, stride, pixfmt);
        let data = frame.image_data();
        debug_assert!(
            data.len() >= min_size,
            "Buffer too small for image dimensions and pixel format"
        );
        let min_data = &data[..min_size];
        Self {
            width,
            height,
            stride,
            buf: Cow::Borrowed(min_data),
            pixfmt,
        }
    }

    #[cfg(feature = "convert-image")]
    /// Converts the image to the specified pixel format, returning a [`CowImage`] that may borrow or own the data.
    ///
    /// If the requested pixel format matches the current format, this method returns
    /// a borrowed view of the data without copying. Otherwise, the data is converted
    /// and a new owned image is returned.
    ///
    /// # Type Parameters
    /// * `FMT` - The target pixel format type
    ///
    /// # Returns
    /// * `Ok(CowImage<FMT>)` - Either a borrowed view or owned converted image
    /// * `Err(convert_image::Error)` - If conversion fails
    ///
    /// # Examples
    /// ```rust
    /// # use strand_dynamic_frame::DynamicFrame;
    /// # use machine_vision_formats::{PixFmt, pixel_format::Mono8};
    /// let data = vec![64u8; 2000];
    /// let frame = DynamicFrame::from_buf(20, 10, 200, data, PixFmt::Mono8).unwrap();
    ///
    /// // No conversion needed - returns borrowed view
    /// let cow_image = frame.into_pixel_format::<Mono8>().unwrap();
    /// ```
    pub fn into_pixel_format<FMT>(&self) -> Result<CowImage<'_, FMT>, convert_image::Error>
    where
        FMT: PixelFormat,
    {
        let dest_fmt = formats::pixel_format::pixfmt::<FMT>().unwrap();
        if dest_fmt == self.pixel_format() {
            // Fast path. Simply return the data.
            Ok(CowImage::Borrowed(
                ImageRef::new(
                    self.width(),
                    self.height(),
                    self.stride(),
                    self.minimum_image_data_without_format(),
                )
                .unwrap(),
            ))
        } else {
            // Conversion path. Allocate a new buffer and convert the data.
            let width = self.width();
            let dest_stride = calc_min_stride(width, dest_fmt);
            let mut dest = OImage::zeros(width, self.height(), dest_stride).unwrap();
            self.into_pixel_format_dest(&mut dest)?;
            Ok(CowImage::Owned(dest))
        }
    }

    /// Return a borrowed view of the image data as a static pixel format.
    ///
    /// This method allows you to treat the dynamic frame as a specific pixel format
    /// without copying the data, as long as the pixel format matches.
    ///
    /// # Type Parameters
    /// * `FMT` - The target pixel format type
    ///
    /// # Returns
    /// * `Some(ImageRef<FMT>)` if the pixel format matches
    /// * `None` if the pixel format does not match
    ///
    /// # Examples
    /// ```rust
    /// # use strand_dynamic_frame::DynamicFrame;
    /// # use machine_vision_formats::{PixFmt, pixel_format::Mono8, image_ref::ImageRef, ImageData};
    /// // Create a dynamic frame with Mono8 pixel format.
    /// let data = vec![128u8; 1000];
    /// let frame = DynamicFrame::from_buf(100, 10, 100, data, PixFmt::Mono8).unwrap();
    ///
    /// // Convert to a static Mono8 view
    /// let static_view: Option<ImageRef<Mono8>> = frame.as_static();
    /// assert!(static_view.is_some());
    /// assert_eq!(static_view.unwrap().width(), 100);
    /// ```
    #[must_use]
    pub fn as_static<FMT: PixelFormat>(&'a self) -> Option<ImageRef<'a, FMT>> {
        let pixfmt = formats::pixel_format::pixfmt::<FMT>().unwrap();
        if pixfmt == self.pixel_format() {
            // Simply return the image data as a borrowed view
            Some(
                ImageRef::new(
                    self.width(),
                    self.height(),
                    self.stride(),
                    self.minimum_image_data_without_format(),
                )
                .unwrap(),
            )
        } else {
            // Cannot convert to static format
            None
        }
    }

    /// Converts the image data into a mutable destination buffer of the
    /// specified pixel format.
    ///
    /// This method will convert the data in-place, modifying the destination
    /// buffer to match the pixel format of the source image.
    ///
    /// # Parameters
    /// * `dest` - A mutable reference to the destination buffer implementing
    ///   [`machine_vision_formats::iter::HasRowChunksExactMut`] for the target
    ///   pixel format.
    ///
    /// # Returns
    /// * `Ok(())` if the conversion was successful
    /// * `Err(convert_image::Error)` if the conversion fails
    ///
    /// # Examples
    /// ```rust
    /// # use strand_dynamic_frame::DynamicFrame;
    /// # use machine_vision_formats::{PixFmt, pixel_format::Mono8, iter::HasRowChunksExactMut,owned::OImage, ImageData, Stride};
    /// // Create a dynamic frame with RGB8 pixel format.
    /// let data = vec![255u8; 3000]; // RGB8 data for 100x10 image
    /// let frame = DynamicFrame::from_buf(100, 10, 300, data, PixFmt::RGB8).unwrap();
    ///
    /// // Create a destination buffer for Mono8 format
    /// let mut dest = OImage::<Mono8>::zeros(100, 10, 100).unwrap();
    ///
    /// // Convert the frame into the destination buffer
    /// frame.into_pixel_format_dest(&mut dest).unwrap();
    /// assert_eq!(dest.width(), 100);
    /// assert_eq!(dest.height(), 10);
    /// assert_eq!(dest.stride(), 100);
    /// ```
    #[cfg(feature = "convert-image")]
    pub fn into_pixel_format_dest<FMT>(
        &self,
        dest: &mut dyn machine_vision_formats::iter::HasRowChunksExactMut<FMT>,
    ) -> Result<(), convert_image::Error>
    where
        FMT: PixelFormat,
    {
        let pixfmt = self.pixel_format();
        match_all_dynamic_fmts!(
            self,
            x,
            convert_image::convert_into(&x, dest),
            convert_image::Error::UnimplementedPixelFormat(pixfmt)
        )
    }

    /// Converts the image to a byte buffer encoded in the specified format.
    ///
    /// This method encodes the image data into a format suitable for storage or transmission.
    /// The encoding options can be specified using [`convert_image::EncoderOptions`].
    ///
    /// # Parameters
    /// * `opts` - Encoding options for the output format
    ///
    /// # Returns
    /// * `Ok(Vec<u8>)` - The encoded image data as a byte vector
    /// * `Err(convert_image::Error)` - If the encoding fails
    ///
    /// # Examples
    /// ```rust
    /// # use strand_dynamic_frame::DynamicFrame;
    /// # use machine_vision_formats::PixFmt;
    /// let data = vec![255u8; 3000]; // RGB8 data for 100x10 image
    /// let frame = DynamicFrame::from_buf(100, 10, 300, data, PixFmt::RGB8).unwrap();
    ///
    /// // Encode the frame to PNG bytes
    /// let encoded_buffer = frame.to_encoded_buffer(convert_image::EncoderOptions::Png).unwrap();
    /// assert!(!encoded_buffer.is_empty());
    /// ```
    #[cfg(feature = "convert-image")]
    pub fn to_encoded_buffer(
        &self,
        opts: convert_image::EncoderOptions,
    ) -> Result<Vec<u8>, convert_image::Error> {
        let pixfmt = self.pixel_format();
        match_all_dynamic_fmts!(
            self,
            x,
            convert_image::frame_to_encoded_buffer(&x, opts),
            convert_image::Error::UnimplementedPixelFormat(pixfmt)
        )
    }

    /// Returns the pixel format of this image.
    ///
    /// # Examples
    /// ```rust
    /// # use strand_dynamic_frame::DynamicFrame;
    /// # use machine_vision_formats::PixFmt;
    /// let data = vec![0u8; 300];
    /// let frame = DynamicFrame::from_buf(10, 10, 30, data, PixFmt::RGB8).unwrap();
    /// assert_eq!(frame.pixel_format(), PixFmt::RGB8);
    /// ```
    #[must_use]
    pub fn pixel_format(&self) -> PixFmt {
        self.pixfmt
    }

    /// Forces the image data to be interpreted as a different pixel format without converting the data.
    ///
    /// Use this method with caution - the resulting image may not be valid if the buffer
    /// size is incompatible with the new pixel format requirements.
    ///
    /// # Parameters
    /// * `pixfmt` - The new pixel format to interpret the data as
    ///
    /// # Returns
    /// * `Some(DynamicFrame)` if the buffer size is compatible with the new format
    /// * `None` if the buffer is too small for the new format
    ///
    /// # Examples
    /// ```rust
    /// # use strand_dynamic_frame::DynamicFrame;
    /// # use machine_vision_formats::PixFmt;
    /// // Create a Mono8 image
    /// let data = vec![128u8; 1000];
    /// let frame = DynamicFrame::from_buf(100, 10, 100, data, PixFmt::Mono8).unwrap();
    ///
    /// // Force it to be interpreted as a different format (if buffer size allows)
    /// let forced_frame = frame.force_pixel_format(PixFmt::Mono8);
    /// assert!(forced_frame.is_some());
    /// ```
    #[must_use]
    pub fn force_pixel_format(self, pixfmt: PixFmt) -> Option<DynamicFrame<'a>> {
        let new_min_size = calc_min_buf_size(self.width, self.height, self.stride, pixfmt);
        if self.buf.len() < new_min_size {
            None // Buffer too small for new pixel format
        } else {
            Some(DynamicFrame {
                width: self.width,
                height: self.height,
                stride: self.stride,
                buf: self.buf,
                pixfmt,
            })
        }
    }

    /// Returns a new [`DynamicFrame`] representing a region of interest (ROI) within the image.
    ///
    /// The ROI is defined by the specified left, top, width, and height parameters.
    /// If the specified ROI is out of bounds or the buffer is too small, this method returns `None`.
    ///
    /// # Parameters
    /// * `left` - The left coordinate of the ROI in pixels
    /// * `top` - The top coordinate of the ROI in pixels
    /// * `width` - The width of the ROI in pixels
    /// * `height` - The height of the ROI in pixels
    ///
    /// # Returns
    /// * `Some(DynamicFrame)` if the ROI is valid and the buffer is large enough
    /// * `None` if the ROI is out of bounds or the buffer is too small
    #[must_use]
    pub fn roi(&'a self, left: u32, top: u32, width: u32, height: u32) -> Option<DynamicFrame<'a>> {
        if left + width > self.width || top + height > self.height {
            return None; // ROI out of bounds
        }
        if left != 0 || top != 0 {
            todo!();
        }
        let stride = self.stride;
        let new_min_size = calc_min_buf_size(width, height, stride, self.pixfmt);
        if self.buf.len() < new_min_size {
            return None; // Buffer too small for ROI
        }
        Some(DynamicFrame {
            width,
            height,
            stride,
            buf: Cow::Borrowed(&self.buf[..new_min_size]),
            pixfmt: self.pixfmt,
        })
    }
}

/// Compile-time test to ensure [`DynamicFrame`] implements the [`Send`] trait.
fn _test_dynamic_frame_is_send() {
    fn implements<T: Send>() {}
    implements::<DynamicFrame>();
}

impl std::fmt::Debug for DynamicFrame<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        f.debug_struct("DynamicFrame")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("stride", &self.stride)
            .field("pixfmt", &self.pixfmt)
            .finish_non_exhaustive()
    }
}

impl Stride for DynamicFrame<'_> {
    /// Returns the stride (bytes per row) of the image.
    ///
    /// The stride represents the number of bytes from the start of one row
    /// to the start of the next row. This may be larger than the minimum
    /// required by the pixel format due to alignment requirements.
    ///
    /// # Examples
    /// ```rust
    /// # use strand_dynamic_frame::DynamicFrame;
    /// # use machine_vision_formats::{PixFmt, Stride};
    /// let data = vec![0u8; 1000];
    /// let frame = DynamicFrame::from_buf(10, 10, 100, data, PixFmt::Mono8).unwrap();
    /// assert_eq!(frame.stride(), 100);
    /// ```
    fn stride(&self) -> usize {
        self.stride
    }
}
