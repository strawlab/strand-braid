//! Type definitions for working with machine vision cameras.
//!
//! This crate aims to be a lowest common denominator for working with images
//! from machine vision cameras from companies such as Basler, FLIR, and AVT.
#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(not(feature = "std"))]
use core::num::NonZeroU8;
#[cfg(feature = "std")]
use std::num::NonZeroU8;

#[cfg(not(feature = "std"))]
use core::str::FromStr;
#[cfg(feature = "std")]
use std::str::FromStr;

#[cfg(not(feature = "std"))]
use core::fmt;
#[cfg(feature = "std")]
use std::fmt;

#[cfg(feature = "alloc")]
use alloc::vec::Vec;

/// Describes the format of raw image data
///
/// Uses [PFNC (Pixel Format Naming
/// Convention)](https://www.emva.org/wp-content/uploads/GenICam_PixelFormatValues.pdf)
/// names.
///
/// TODO: Check if names from ffmpeg (e.g. `AV_PIX_FMT_YUVA444P`) would be
/// better.
#[non_exhaustive]
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum PixelFormat {
    /// Red, Green, Blue, 1 byte each, total 3 bytes per pixel.
    ///
    /// Also sometimes called `RGB8packed`.
    RGB8,
    /// Luminance, 1 byte per pixel.
    MONO8,
    /// Luminance, 10 bits per pixel.
    MONO10,
    /// Luminance, 10 bits per pixel. p.
    MONO10p,
    /// Luminance, 12 bits per pixel.
    MONO12,
    /// Luminance, 12 bits per pixel. p. MONO12p and MONO12packed are different formats.
    MONO12p,
    /// Luminance, 12 bits per pixel. packed. MONO12p and MONO12packed are different formats.
    MONO12packed,
    /// Luminance, 16 bits per pixel.
    MONO16,
    /// Luminance, 32 bits floating point per pixel.
    MONO32f,
    /// Bayer Red Green pattern, 1 byte per pixel.
    BayerRG8,
    /// Bayer Blue Green pattern, 1 byte per pixel.
    BayerBG8,
    /// Bayer Green Blue pattern, 1 byte per pixel.
    BayerGB8,
    /// Bayer Green Red pattern, 1 byte per pixel.
    BayerGR8,
    /// Bayer Red Green pattern, 32 bits floating point per pixel.
    BayerRG32f,
    /// Bayer Blue Green pattern, 32 bits floating point per pixel.
    BayerBG32f,
    /// Bayer Green Blue pattern, 32 bits floating point per pixel.
    BayerGB32f,
    /// Bayer Green Red pattern, 32 bits floating point per pixel.
    BayerGR32f,
    /// 3 bytes per pixel (12 bytes per 4 pixels)
    YUV444,
    /// 4 bytes per 2 pixels ( 8 bytes per 4 pixels)
    ///
    /// Also sometimes called `YUV422Packed`.
    YUV422,
    /// 6 bytes per 4 pixels
    YUV411,
    // More here (e.g. even JPEG?)
}

impl PixelFormat {
    /// The number of bits per pixel, if possible.
    pub fn bits_per_pixel(&self) -> Option<NonZeroU8> {
        use crate::PixelFormat::*;
        match self {
            MONO8 | BayerRG8 | BayerGB8 | BayerGR8 | BayerBG8 => NonZeroU8::new(8),
            RGB8 => NonZeroU8::new(24),
            YUV422 => NonZeroU8::new(16),
            MONO32f | BayerRG32f | BayerBG32f | BayerGB32f | BayerGR32f => NonZeroU8::new(32),
            _ => None,
        }
    }
}

// ------------------------------- simple traits ----------------------

/// An image.
pub trait ImageData {
    /// Number of pixel columns in the image. Note: this is not the stride.
    fn width(&self) -> u32;
    /// Number of pixel rows in the image.
    fn height(&self) -> u32;
    /// returns a slice to the raw image data, does not copy the data
    fn image_data(&self) -> &[u8];
    /// the image format
    fn pixel_format(&self) -> PixelFormat;
}

/// An image whose data is stored such that successive rows are a stride apart.
pub trait Stride {
    /// the width (in bytes) of each row of image data
    fn stride(&self) -> usize;
}

// ------------------------------- compound traits ----------------------

/// Can be converted into `ImageData`.
pub trait AsImageData: ImageData {
    fn as_image_data(&self) -> &dyn ImageData;
}
impl<S: ImageData> AsImageData for S {
    fn as_image_data(&self) -> &dyn ImageData {
        self
    }
}

#[cfg(any(feature = "std", feature = "alloc"))]
/// An image which can be moved into `Vec<u8>`.
pub trait OwnedImage: AsImageData + ImageData + Into<Vec<u8>> {}

#[cfg(any(feature = "std", feature = "alloc"))]
impl<S> OwnedImage for S
where
    S: AsImageData + ImageData,
    Vec<u8>: From<S>,
{
}

/// An image with a stride.
pub trait ImageStride: ImageData + Stride {}
impl<S: ImageData + Stride> ImageStride for S {}

/// Can be converted into `ImageStride`.
pub trait AsImageStride: ImageStride {
    fn as_image_stride(&self) -> &dyn ImageStride;
}
impl<S: ImageStride> AsImageStride for S {
    fn as_image_stride(&self) -> &dyn ImageStride {
        self
    }
}

#[cfg(any(feature = "std", feature = "alloc"))]
/// An image with a stride which can be moved into `Vec<u8>`.
pub trait OwnedImageStride: AsImageStride + ImageStride + Into<Vec<u8>> {}
#[cfg(any(feature = "std", feature = "alloc"))]
impl<S> OwnedImageStride for S
where
    S: AsImageStride + ImageStride,
    Vec<u8>: From<S>,
{
}

// -----------------------------------------------------------------------

impl FromStr for PixelFormat {
    type Err = &'static str;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "RGB8" => Ok(PixelFormat::RGB8),
            "MONO8" => Ok(PixelFormat::MONO8),
            "MONO10" => Ok(PixelFormat::MONO10),
            "MONO32f" => Ok(PixelFormat::MONO32f),
            "BayerRG8" => Ok(PixelFormat::BayerRG8),
            "BayerBG8" => Ok(PixelFormat::BayerBG8),
            "BayerGB8" => Ok(PixelFormat::BayerGB8),
            "BayerGR8" => Ok(PixelFormat::BayerGR8),
            "BayerRG32f" => Ok(PixelFormat::BayerRG32f),
            "BayerBG32f" => Ok(PixelFormat::BayerBG32f),
            "BayerGB32f" => Ok(PixelFormat::BayerGB32f),
            "BayerGR32f" => Ok(PixelFormat::BayerGR32f),
            "YUV444" => Ok(PixelFormat::YUV444),
            "YUV422" => Ok(PixelFormat::YUV422),
            "YUV411" => Ok(PixelFormat::YUV411),
            _ => Err("unknown pixel format"),
        }
    }
}

impl fmt::Display for PixelFormat {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "{:?}", self)
    }
}

#[cfg(test)]
mod tests {

    #[cfg(feature = "std")]
    #[test]
    fn pixel_format_str_roundtrip() {
        use crate::PixelFormat::*;
        use std::str::FromStr;

        // A list of all PixelFormat variants.
        let fmts = [
            RGB8, MONO8, MONO10, MONO32f, BayerRG8, BayerBG8, BayerGB8, BayerGR8, BayerRG32f,
            BayerBG32f, BayerGB32f, BayerGR32f, YUV444, YUV422, YUV411,
        ];

        for fmt in fmts.iter() {
            let my_str = format!("{}", fmt);
            println!("testing {}", my_str);
            let result = crate::PixelFormat::from_str(&my_str).unwrap();
            assert_eq!(fmt, &result);
        }
    }
}
