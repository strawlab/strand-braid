//! Implementations of specific pixel formats.
//

// TODO: Check if we should use [PFNC (Pixel Format Naming
// Convention)](https://www.emva.org/wp-content/uploads/GenICamPixelFormatValues.pdf)
// names.

// TODO: Check if names from ffmpeg (e.g. `AV_PIX_FMT_YUVA444P`) would be
// better.

#[cfg(not(feature = "std"))]
extern crate core as std;

use std::convert::TryFrom;

/// This type allows runtime inspection of pixel format.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum PixFmt {
    Mono8,
    Mono32f,
    RGB8,
    BayerRG8,
    BayerRG32f,
    BayerBG8,
    BayerBG32f,
    BayerGB8,
    BayerGB32f,
    BayerGR8,
    BayerGR32f,
    YUV422,
    NV12,
}

impl PixFmt {
    /// Convert a runtime variant into a static type.
    pub fn to_static<FMT: PixelFormat>(&self) -> Option<std::marker::PhantomData<FMT>> {
        let other = pixfmt::<FMT>();
        if Ok(self) == other.as_ref() {
            Some(std::marker::PhantomData)
        } else {
            None
        }
    }
    /// The average number of bits per pixel.
    pub const fn bits_per_pixel(&self) -> u8 {
        use PixFmt::*;
        match self {
            Mono8 => 8,
            Mono32f => 32,
            RGB8 => 24,
            BayerRG8 => 8,
            BayerRG32f => 32,
            BayerBG8 => 8,
            BayerBG32f => 32,
            BayerGB8 => 8,
            BayerGB32f => 32,
            BayerGR8 => 8,
            BayerGR32f => 32,
            YUV422 => 16,
            NV12 => 12,
        }
    }
    /// The name of the pixel format.
    pub const fn as_str(&self) -> &'static str {
        use PixFmt::*;
        match self {
            Mono8 => "Mono8",
            Mono32f => "Mono32f",
            RGB8 => "RGB8",
            BayerRG8 => "BayerRG8",
            BayerRG32f => "BayerRG32f",
            BayerBG8 => "BayerBG8",
            BayerBG32f => "BayerBG32f",
            BayerGB8 => "BayerGB8",
            BayerGB32f => "BayerGB32f",
            BayerGR8 => "BayerGR8",
            BayerGR32f => "BayerGR32f",
            YUV422 => "YUV422",
            NV12 => "NV12",
        }
    }
}

impl std::fmt::Display for PixFmt {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for PixFmt {
    type Err = &'static str;
    fn from_str(instr: &str) -> Result<Self, <Self as std::str::FromStr>::Err> {
        use PixFmt::*;
        if instr == "Mono8" {
            Ok(Mono8)
        } else if instr == "Mono32f" {
            Ok(Mono32f)
        } else if instr == "RGB8" {
            Ok(RGB8)
        } else if instr == "BayerRG8" {
            Ok(BayerRG8)
        } else if instr == "BayerRG32f" {
            Ok(BayerRG32f)
        } else if instr == "BayerBG8" {
            Ok(BayerBG8)
        } else if instr == "BayerBG32f" {
            Ok(BayerBG32f)
        } else if instr == "BayerGB8" {
            Ok(BayerGB8)
        } else if instr == "BayerGB32f" {
            Ok(BayerGB32f)
        } else if instr == "BayerGR8" {
            Ok(BayerGR8)
        } else if instr == "BayerGR32f" {
            Ok(BayerGR32f)
        } else if instr == "YUV422" {
            Ok(YUV422)
        } else if instr == "NV12" {
            Ok(NV12)
        } else {
            Err("Cannot parse string")
        }
    }
}

#[test]
fn test_pixfmt_roundtrip() {
    use PixFmt::*;
    let fmts = [
        Mono8, Mono32f, RGB8, BayerRG8, BayerRG32f, BayerBG8, BayerBG32f, BayerGB8, BayerGB32f,
        BayerGR8, BayerGR32f, YUV422, NV12,
    ];
    for fmt in &fmts {
        let fmt_str = fmt.as_str();
        dbg!(fmt_str);
        let fmt2 = std::str::FromStr::from_str(fmt_str).unwrap();
        assert_eq!(fmt, &fmt2);
    }
}

macro_rules! try_downcast {
    ($name:ident, $orig:expr) => {{
        if let Some(_) = <dyn std::any::Any>::downcast_ref::<std::marker::PhantomData<$name>>($orig)
        {
            return Ok(PixFmt::$name);
        }
    }};
}

impl<FMT> TryFrom<std::marker::PhantomData<FMT>> for PixFmt
where
    FMT: PixelFormat,
{
    type Error = &'static str;

    fn try_from(orig: std::marker::PhantomData<FMT>) -> Result<PixFmt, Self::Error> {
        try_downcast!(Mono8, &orig);
        try_downcast!(Mono32f, &orig);
        try_downcast!(RGB8, &orig);
        try_downcast!(BayerRG8, &orig);
        try_downcast!(BayerRG32f, &orig);
        try_downcast!(BayerBG8, &orig);
        try_downcast!(BayerBG32f, &orig);
        try_downcast!(BayerGB8, &orig);
        try_downcast!(BayerGB32f, &orig);
        try_downcast!(BayerGR8, &orig);
        try_downcast!(BayerGR32f, &orig);
        try_downcast!(YUV422, &orig);
        try_downcast!(NV12, &orig);
        Err("unknown PixelFormat implementation could not be converted to PixFmt")
    }
}

/// Convert a compile-time type FMT into a runtime type.
#[inline]
pub fn pixfmt<FMT: PixelFormat>() -> Result<PixFmt, &'static str> {
    use std::convert::TryInto;
    let concrete: std::marker::PhantomData<FMT> = std::marker::PhantomData;
    concrete.try_into()
}

#[test]
fn test_compile_runtime_roundtrip() {
    macro_rules! gen_test {
        ($name:ident) => {{
            let x = PixFmt::$name;
            let y = x.to_static::<$name>().unwrap();
            let z = PixFmt::try_from(y).unwrap();
            assert_eq!(x, z);
        }};
    }
    gen_test!(Mono8);
    gen_test!(Mono32f);
    gen_test!(RGB8);
    gen_test!(BayerRG8);
    gen_test!(BayerRG32f);
    gen_test!(BayerBG8);
    gen_test!(BayerBG32f);
    gen_test!(BayerGB8);
    gen_test!(BayerGB32f);
    gen_test!(BayerGR8);
    gen_test!(BayerGR32f);
    gen_test!(YUV422);
    gen_test!(NV12);
}

/// Implementations of this trait describe the format of raw image data.
///
/// Note that when [const generics for custom
/// types](https://blog.rust-lang.org/2021/02/26/const-generics-mvp-beta.html#const-generics-for-custom-types)
/// are introduced to the rust compiler, we intend to switch PixelFormat to use
/// that feature.
pub trait PixelFormat: std::any::Any + Clone {}

macro_rules! define_pixel_format {
    ($name:ident, $comment:literal) => {
        #[doc = $comment]
        #[derive(Clone)]
        pub struct $name {}
        impl PixelFormat for $name {}
    };
}

define_pixel_format!(Mono8, "Luminance, 1 byte per pixel.");
define_pixel_format!(
    Mono32f,
    "Luminance, 32 bytes per pixel, Little-Endian, IEEE-754"
);

define_pixel_format!(
    RGB8,
    "Red, Green, Blue, 1 byte each, total 3 bytes per pixel.

Also sometimes called `RGB8packed`."
);
define_pixel_format!(BayerRG8, "Bayer Red Green pattern, 1 byte per pixel.");
define_pixel_format!(BayerRG32f, "Bayer Red Green pattern, 4 bytes per pixel.");
define_pixel_format!(BayerBG8, "Bayer Blue Green pattern, 1 byte per pixel.");
define_pixel_format!(BayerBG32f, "Bayer Blue Green pattern, 4 bytes per pixel.");
define_pixel_format!(BayerGB8, "Bayer Green Blue pattern, 1 byte per pixel.");
define_pixel_format!(BayerGB32f, "Bayer Green Blue pattern, 4 bytes per pixel.");
define_pixel_format!(BayerGR8, "Bayer Green Red pattern, 1 byte per pixel.");
define_pixel_format!(BayerGR32f, "Bayer Green Red pattern, 4 bytes per pixel.");
define_pixel_format!(YUV422, "YUV 4:2:2 8-bit, total 2 bytes per pixel.");
define_pixel_format!(NV12, "NV12 format, average 12 bits per pixel");
