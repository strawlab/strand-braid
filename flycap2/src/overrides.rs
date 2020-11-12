//! The **overrides** module provides custom FFI definitions.

use libflycapture2_sys::{fc2PixelFormat, fc2Image, fc2Error, fc2ImageFileFormat,
    fc2TimeStamp};

extern "C" {
    pub fn fc2ConvertImageTo(format: fc2PixelFormat, pImageIn: *const fc2Image,
                                pImageOut: *mut fc2Image) -> fc2Error;

    pub fn fc2SaveImage(pImage: *const fc2Image,
                        pFilename: *const ::std::os::raw::c_char,
                        format: fc2ImageFileFormat) -> fc2Error;
    pub fn fc2GetImageTimeStamp(pImage: *const fc2Image) -> fc2TimeStamp;
}
