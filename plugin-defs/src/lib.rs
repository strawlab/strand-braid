use std::os::raw::{c_char, c_float, c_void};

include!(concat!(env!("OUT_DIR"), "/codegen.rs"));

/// Describes the format of raw image data
///
/// Uses [PFNC (Pixel Format Naming
/// Convention)](https://www.emva.org/wp-content/uploads/GenICam_PixelFormatValues.pdf)
/// names.
///
// TODO: Check if names from ffmpeg (e.g. `AV_PIX_FMT_YUVA444P`) would be
// better.
//
// Mostly copied from machine-vision-formats
#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum EisvogelPixelFormat {
    /// Red, Green, Blue, 1 byte each, total 3 bytes per pixel.
    ///
    /// Also sometimes called `RGB8packed`.
    RGB8 = 0,
    /// Luminance, 1 byte per pixel.
    MONO8,
    /// Luminance, 10 bits per pixel.
    MONO10,
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

#[repr(C)]
pub struct FrameData {
    pub data: *const c_char,
    pub stride: u64,
    pub rows: u32,
    pub cols: u32,
    pub pixel_format: EisvogelPixelFormat,
}

pub type DataHandle = *mut c_void;

/// Any `ProcessFrameFunc` allocates new memory that needs to be freed with `strandcam_frame_annotation_free`.
pub type ProcessFrameFunc =
    extern "C" fn(*const FrameData, DataHandle, f64) -> StrandCamFrameAnnotation;

/// CABI wrapper around point.
#[repr(C)]
pub struct EisvogelImagePoint {
    pub x: c_float,
    pub y: c_float,
}

/// CABI wrapper around frame annotation points.
#[repr(C)]
pub struct StrandCamFrameAnnotation {
    pub points: *mut EisvogelImagePoint,
    pub len: usize,
    pub owned: bool,
}

impl std::fmt::Display for StrandCamFrameAnnotation {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "StrandCamFrameAnnotation {{ len: {}, owned: {} }}",
            self.len, self.owned
        )
    }
}

impl StrandCamFrameAnnotation {
    /// Creates a new `StrandCamFrameAnnotation` from a slice.
    pub fn new(s: &[EisvogelImagePoint]) -> StrandCamFrameAnnotation {
        StrandCamFrameAnnotation {
            points: s.as_ptr() as *mut EisvogelImagePoint,
            len: s.len(),
            owned: false,
        }
    }

    /// Creates a new `StrandCamFrameAnnotation` from an owned Rust string.
    pub fn from_vec(mut s: Vec<EisvogelImagePoint>) -> StrandCamFrameAnnotation {
        s.shrink_to_fit();
        let rv = StrandCamFrameAnnotation {
            points: s.as_ptr() as *mut EisvogelImagePoint,
            len: s.len(),
            owned: true,
        };
        std::mem::forget(s);
        rv
    }

    /// Releases memory held by an unmanaged `StrandCamFrameAnnotation`.
    pub unsafe fn free(&mut self) {
        if self.owned {
            Vec::from_raw_parts(self.points as *mut _, self.len, self.len); // this gets dropped
            self.points = std::ptr::null_mut(); // clear pointer
            self.len = 0;
            self.owned = false;
        }
    }

    /// Returns the slice managed by a `StrandCamFrameAnnotation`.
    pub fn as_slice(&self) -> &[EisvogelImagePoint] {
        unsafe { std::slice::from_raw_parts(self.points as *const _, self.len) }
    }
}

impl Drop for StrandCamFrameAnnotation {
    fn drop(&mut self) {
        unsafe { self.free() }
    }
}

/// Create new frame annotation data filled with zeros.
#[no_mangle]
pub extern "C" fn strandcam_new_frame_annotation_zeros(
    n_points: usize,
) -> StrandCamFrameAnnotation {
    let points = (0..n_points)
        .map(|_| EisvogelImagePoint { x: 0.0, y: 0.0 })
        .collect();
    StrandCamFrameAnnotation::from_vec(points)
}

#[no_mangle]
pub unsafe extern "C" fn strandcam_set_frame_annotation(
    fa: *mut StrandCamFrameAnnotation,
    i: isize,
    x: f32,
    y: f32,
) {
    let ptr = (*fa).points.offset(i);
    (*ptr).x = x;
    (*ptr).y = y;
}

/// Free frame annotation data.
///
/// If the data is marked as not owned then this function does not
/// do anything.
#[no_mangle]
pub unsafe extern "C" fn strandcam_frame_annotation_free(data: *mut StrandCamFrameAnnotation) {
    (*data).free()
}
