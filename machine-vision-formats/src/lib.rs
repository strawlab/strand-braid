//! Type definitions for working with machine vision cameras.
//!
//! This crate aims to be a lowest common denominator for working with images
//! from machine vision cameras from companies such as Basler, FLIR, and AVT.
#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate core as std;

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
use alloc::vec::Vec;

// TODO: Should we move module `pixel_format` to own crate?
#[allow(non_camel_case_types)]
pub mod pixel_format;

// re-export
pub use pixel_format::{PixFmt, PixelFormat};

// ------------------------------- ImageBufferRef ----------------------

/// A concrete type with view of image data with pixel format `F`.
///
/// This is a zero-size wrapper around a slice of bytes parameterized by the
/// type `F`. It should cause no additional overhead above passing the raw byte
/// slice but maintains a compile-time guarantee of the image format.
#[derive(Clone)]
pub struct ImageBufferRef<'a, F> {
    /// The pixel format
    pub pixel_format: std::marker::PhantomData<F>,
    /// The raw bytes of the image buffer.
    pub data: &'a [u8],
}

impl<'a, F> ImageBufferRef<'a, F> {
    #[inline]
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            pixel_format: std::marker::PhantomData,
            data,
        }
    }
    /// Copy the data to make a new buffer.
    #[cfg(any(feature = "std", feature = "alloc"))]
    #[inline]
    pub fn to_buffer(&self) -> ImageBuffer<F> {
        ImageBuffer::new(self.data.to_vec())
    }
}

// ------------------------------- ImageBufferMutRef ----------------------

/// A concrete type with view of mutable image data with pixel format `F`.
///
/// This is a zero-size wrapper around a slice of bytes parameterized by the
/// type `F`. It should cause no additional overhead above passing the raw byte
/// slice but maintains a compile-time guarantee of the image format.
pub struct ImageBufferMutRef<'a, F> {
    /// The pixel format
    pub pixel_format: std::marker::PhantomData<F>,
    /// The raw bytes of the image buffer.
    pub data: &'a mut [u8],
}

impl<'a, F> ImageBufferMutRef<'a, F> {
    #[inline]
    pub fn new(data: &'a mut [u8]) -> Self {
        Self {
            pixel_format: std::marker::PhantomData,
            data,
        }
    }
    /// Copy the data to make a new buffer.
    #[cfg(any(feature = "std", feature = "alloc"))]
    #[inline]
    pub fn to_buffer(&self) -> ImageBuffer<F> {
        ImageBuffer::new(self.data.to_vec())
    }
}

// ------------------------------- ImageBuffer ----------------------

/// A concrete type which containing image data with pixel format `F`.
///
/// This is a zero-size wrapper around bytes parameterized by the type `F`. It
/// should cause no additional overhead above passing the raw byte vector but
/// maintains a compile-time guarantee of the image format.
#[cfg(any(feature = "std", feature = "alloc"))]
#[derive(Clone)]
pub struct ImageBuffer<F> {
    /// The pixel format
    pub pixel_format: std::marker::PhantomData<F>,
    /// The raw bytes of the image buffer.
    pub data: Vec<u8>,
}

#[cfg(any(feature = "std", feature = "alloc"))]
impl<F> ImageBuffer<F> {
    #[inline]
    pub fn new(data: Vec<u8>) -> Self {
        Self {
            pixel_format: std::marker::PhantomData,
            data,
        }
    }
}

// ------------------------------- simple traits ----------------------

/// An image.
///
/// The pixel format is specified as the type `F`.
pub trait ImageData<F> {
    /// Number of pixel columns in the image. Note: this is not the stride.
    fn width(&self) -> u32;
    /// Number of pixel rows in the image.
    fn height(&self) -> u32;
    /// Returns the raw image data as specified by pixel format `F`.
    ///
    /// This does not copy the data but returns a view of it.
    ///
    /// This method may be deprecated in factor of `buffer_ref`.
    #[inline]
    fn image_data(&self) -> &[u8] {
        &self.buffer_ref().data
    }
    /// Returns the image buffer specified by pixel format `F`.
    ///
    /// Ideally, prefer using this over `image_data()`.
    ///
    /// This does not copy the data but returns a view of it.
    fn buffer_ref(&self) -> ImageBufferRef<'_, F>;
    /// Returns the image buffer specified by pixel format `F`.
    ///
    /// Implementations should move the data without copying it if possible. The
    /// implementation may copy the data if needed. To guarantee a move with no
    /// copy, use the `Into<Vec<u8>>` trait required by the OwnedImage trait.
    #[cfg(any(feature = "std", feature = "alloc"))]
    fn buffer(self) -> ImageBuffer<F>;
}

/// An image whose data is stored such that successive rows are a stride apart.
///
/// This is sometimes also called "pitch".
pub trait Stride {
    /// the width (in bytes) of each row of image data
    ///
    /// This is sometimes also called "pitch".
    fn stride(&self) -> usize;
}

// ------------------------------- compound traits ----------------------

/// Can be converted into `ImageData`.
pub trait AsImageData<F>: ImageData<F> {
    fn as_image_data(&self) -> &dyn ImageData<F>;
}
impl<S: ImageData<F>, F> AsImageData<F> for S {
    fn as_image_data(&self) -> &dyn ImageData<F> {
        self
    }
}

#[cfg(any(feature = "std", feature = "alloc"))]
/// An image which can be moved into `Vec<u8>`.
pub trait OwnedImage<F>: AsImageData<F> + ImageData<F> + Into<Vec<u8>> {}

#[cfg(any(feature = "std", feature = "alloc"))]
impl<S, F> OwnedImage<F> for S
where
    S: AsImageData<F> + ImageData<F>,
    Vec<u8>: From<S>,
{
}

/// An image with a stride.
pub trait ImageStride<F>: ImageData<F> + Stride {}
impl<S: ImageData<F> + Stride, F> ImageStride<F> for S {}

/// Can be converted into `ImageStride`.
pub trait AsImageStride<F>: ImageStride<F> {
    fn as_image_stride(&self) -> &dyn ImageStride<F>;
}
impl<S: ImageStride<F>, F> AsImageStride<F> for S {
    fn as_image_stride(&self) -> &dyn ImageStride<F> {
        self
    }
}

#[cfg(any(feature = "std", feature = "alloc"))]
/// An image with a stride which can be moved into `Vec<u8>`.
pub trait OwnedImageStride<F>: AsImageStride<F> + ImageStride<F> + Into<Vec<u8>> {}
#[cfg(any(feature = "std", feature = "alloc"))]
impl<S, F> OwnedImageStride<F> for S
where
    S: AsImageStride<F> + ImageStride<F>,
    Vec<u8>: From<S>,
{
}
