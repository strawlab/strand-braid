#[macro_use]
extern crate log;
extern crate libc;
extern crate camiface_sys as ffi;
extern crate machine_vision_formats as formats;

use libc::{intptr_t, c_long, c_int};
use std::fmt::{self, Display};
use std::ffi::CStr;
use std::str;
use std::error::Error;
use std::sync::{Once, ONCE_INIT};
use std::ptr;
use std::sync::{Arc, Mutex};
use std::os::raw::c_char;

use std::string::FromUtf8Error;
use formats::{PixelFormat, Endian};

pub struct CamIface {
}

pub struct CamInfo {
    pub vendor: String,
    pub model: String,
    pub chip: String,
}

pub struct CamContext {
    pub c_ptr: *mut ffi::CamContext,
    pub module: Arc<Mutex<CamIface>>, // hold reference to module to prevent dropping it
    buffer_size: u32,
    coding: PixelFormat,
    roi: FrameROI,
}

#[derive(Copy,Clone)]
pub struct FrameROI {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
}

impl Display for FrameROI {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f,
               "FrameROI {{ left: {}, top: {}, width: {}, height: {} }}",
               self.left,
               self.top,
               self.width,
               self.height)
    }
}

pub struct CameraPropertyInfo {
    pub name: String,
    prop_info: ffi::CameraPropertyInfo,
}

impl CameraPropertyInfo {
    pub fn get_units(&self) -> CamIfaceResult<String> {
        let x = &self.prop_info;
        let units = if x.scaled_unit_name.is_null() {
            "".to_string()
        } else {
            let slice = unsafe { CStr::from_ptr(x.scaled_unit_name) };
            try!(std::str::from_utf8(slice.to_bytes()))
                // .expect("from UTF8")
                .to_string()
        };
        Ok(units)
    }
    pub fn is_present(&self) -> bool {
        self.prop_info.is_present != 0
    }
    pub fn has_auto_mode(&self) -> bool {
        self.prop_info.has_auto_mode != 0
    }
    pub fn has_manual_mode(&self) -> bool {
        self.prop_info.has_manual_mode != 0
    }
    pub fn get_min_value(&self) -> u32 {
        self.prop_info.min_value as u32
    }
    pub fn get_max_value(&self) -> u32 {
        self.prop_info.max_value as u32
    }
}


impl Display for CameraPropertyInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let slice = unsafe { CStr::from_ptr(self.prop_info.name) };
        write!(f, "camera property {{ name: {}, present: {}, /* other props not implemented */ }}",
           str::from_utf8(slice.to_bytes()).unwrap(),
           (self.prop_info.is_present!=0),
           )
    }
}

pub struct CameraProperty {
    value: c_long,
    auto: c_int,
}

impl CameraProperty {
    pub fn new(value: u32, auto: bool) -> CameraProperty {
        CameraProperty {
            value: value as c_long,
            auto: auto as c_int,
        }
    }
    pub fn get_value(&self) -> u32 {
        self.value as u32
    }
    pub fn is_auto(&self) -> bool {
        self.auto != 0
    }
}

#[derive(Debug)]
pub struct Timestamp {
    pub secs: u64,
    pub nsecs: u64,
}

impl Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f,
               "Timestamp {{ secs: {}, nsecs: {} }}",
               self.secs,
               self.nsecs)
    }
}

impl Timestamp {
    pub fn from_f64(float_timestamp: f64) -> Timestamp {
        fn to_u64(val: f64) -> u64 {
            val as u64
        }
        Timestamp {
            secs: to_u64(float_timestamp.trunc()),
            nsecs: to_u64(float_timestamp.fract() * 1e9),
        }
    }
}

pub struct CaptureData {
    pub roi: FrameROI, // the ROI of the image data
    pub stride: i32, // the stride of the image data
    pub image_data: Vec<u8>, // the raw image data
    pub timestamp: Option<Timestamp>, // the timestamp, if available
    pub framenumber: Option<u64>, // the framenumber, if available
    pub coding: PixelFormat,
}

impl Display for CaptureData {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fn op_fmt<T: Display>(ooval: &Option<T>) -> String {
            match *ooval {
                Some(ref val) => format!("{}", val),
                None => "(not available)".to_string(),
            }
        }
        write!(f,
               "CaptureData {{ framenumber: {}, timestamp: {}, image_data: {} byte buffer }}",
               op_fmt(&self.framenumber),
               op_fmt(&self.timestamp),
               self.image_data.len())
    }
}

#[derive(Debug)]
pub enum CamIfaceError {
    StringConversionError,
    MyFromUtf8Error(FromUtf8Error),
    MyUtf8Error(str::Utf8Error),
    FromC(i32),
    InitializedAlready,
    FailedToGetConstructor,
    FrameDataMissing,
    FrameTimeout,
    FrameDataLost,
    HardwareFeatureNotAvailable,
    OtherError,
    FrameInterruptedSyscall,
    SelectReturnedButNoFrameAvailable,
    FrameDataCorrupt,
    BufferOverflow,
    CameraNotAvailable,
    GenericError,
}

impl Error for CamIfaceError {
    fn description(&self) -> &str {
        match *self {
            CamIfaceError::StringConversionError => "StringConversionError",
            CamIfaceError::MyFromUtf8Error(_) => "FromUtf8Error",
            CamIfaceError::MyUtf8Error(_) => "str::Utf8Error",
            CamIfaceError::FromC(_) => "FromC",
            CamIfaceError::InitializedAlready => "already started",
            CamIfaceError::FailedToGetConstructor => "failed to get constructor",
            CamIfaceError::FrameDataMissing => "FrameDataMissing",
            CamIfaceError::FrameTimeout => "FrameTimeout",
            CamIfaceError::FrameDataLost => "FrameDataLost",
            CamIfaceError::HardwareFeatureNotAvailable => "HardwareFeatureNotAvailable",
            CamIfaceError::OtherError => "OtherError",
            CamIfaceError::FrameInterruptedSyscall => "FrameInterruptedSyscall",
            CamIfaceError::SelectReturnedButNoFrameAvailable => "SelectReturnedButNoFrameAvail",
            CamIfaceError::FrameDataCorrupt => "FrameDataCorrupt",
            CamIfaceError::BufferOverflow => "BufferOverflow",
            CamIfaceError::CameraNotAvailable => "CameraNotAvailable",
            CamIfaceError::GenericError => "GenericError",
        }
    }
}

impl Display for CamIfaceError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt(&self.description(), f)
    }
}

impl From<FromUtf8Error> for CamIfaceError {
    fn from(err: FromUtf8Error) -> CamIfaceError {
        CamIfaceError::MyFromUtf8Error(err)
    }
}

impl From<str::Utf8Error> for CamIfaceError {
    fn from(err: str::Utf8Error) -> CamIfaceError {
        CamIfaceError::MyUtf8Error(err)
    }
}

pub type CamIfaceResult<T> = Result<T, CamIfaceError>;

macro_rules! check_string {
  ($x:expr) => {{
    let xstr = unsafe { CStr::from_ptr( $x ) };
    let result = try!( str::from_utf8(xstr.to_bytes()) );
    result.to_string()
  }}
}

pub fn get_current_error() -> CamIfaceResult<()> {
    let err_val: i32;
    unsafe {
        err_val = ffi::cam_iface_have_error();
        ffi::cam_iface_clear_error();
    }
    match err_val {
        0 => Ok(()),
        ffi::CAM_IFACE_FRAME_DATA_MISSING_ERROR => Err(CamIfaceError::FrameDataMissing),
        ffi::CAM_IFACE_FRAME_TIMEOUT => Err(CamIfaceError::FrameTimeout),
        ffi::CAM_IFACE_FRAME_DATA_LOST_ERROR => Err(CamIfaceError::FrameDataLost),
        ffi::CAM_IFACE_HARDWARE_FEATURE_NOT_AVAILABLE => {
            Err(CamIfaceError::HardwareFeatureNotAvailable)
        }
        ffi::CAM_IFACE_OTHER_ERROR => Err(CamIfaceError::OtherError),
        ffi::CAM_IFACE_FRAME_INTERRUPTED_SYSCALL => Err(CamIfaceError::FrameInterruptedSyscall),
        ffi::CAM_IFACE_SELECT_RETURNED_BUT_NO_FRAME_AVAILABLE => {
            Err(CamIfaceError::SelectReturnedButNoFrameAvailable)
        }
        ffi::CAM_IFACE_FRAME_DATA_CORRUPT_ERROR => Err(CamIfaceError::FrameDataCorrupt),
        ffi::CAM_IFACE_BUFFER_OVERFLOW_ERROR => Err(CamIfaceError::BufferOverflow),
        ffi::CAM_IFACE_CAMERA_NOT_AVAILABLE_ERROR => Err(CamIfaceError::CameraNotAvailable),
        ffi::CAM_IFACE_GENERIC_ERROR => Err(CamIfaceError::GenericError),
        _ => Err(CamIfaceError::FromC(err_val)),
    }
}

// Return from function with Err(CamIfaceError) if
// cam_iface_have_error() returns true.
macro_rules! err_on_camiface_error {
    () => {{
        match get_current_error() {
            Ok(_) => {},
            Err(e) => return Err(e),
        }
    }}
}

macro_rules! check_camiface_error {
  ($x:expr) => {{
    let result;
    unsafe {
      result = $x; // original expression
    }
    err_on_camiface_error!();
    result
  }}
}

fn c_chars_to_str(in_arr: &[c_char]) -> CamIfaceResult<String> {
    let s1 = try!(String::from_utf8(in_arr.iter().map(|&x| x as u8).collect()));
    let s2 = s1.split('\0').take(1).next();
    s2.map(|s| s.to_string()).ok_or(CamIfaceError::StringConversionError)
}

pub fn get_api_version() -> CamIfaceResult<String> {
    Ok(check_string!(ffi::cam_iface_get_api_version()))
}

impl CamIface {
    pub fn new() -> CamIfaceResult<CamIface> {
        // Check if we have already called cam_iface_startup()
        static START: Once = ONCE_INIT;
        let mut started_this_time = false;
        START.call_once(|| {
            started_this_time = true;
        });
        if !started_this_time {
            return Err(CamIfaceError::InitializedAlready);
        }

        // No, we didn't call it yet, OK to call it now.
        debug!("cam_iface_startup()");
        check_camiface_error!{ffi::cam_iface_startup()};
        Ok(CamIface {}) // create and return struct with no member
    }

    pub fn get_driver_name(&self) -> CamIfaceResult<String> {
        Ok(check_string!(ffi::cam_iface_get_driver_name()))
    }

    pub fn get_num_cameras(&self) -> CamIfaceResult<i32> {
        let ncams = check_camiface_error!{ffi::cam_iface_get_num_cameras()};
        Ok(ncams)
    }

    pub fn get_num_modes(&self, device_number: i32) -> CamIfaceResult<i32> {
        let mut nmodes: i32 = 0;
        check_camiface_error!{ffi::cam_iface_get_num_modes(device_number, &mut nmodes)};
        Ok(nmodes)
    }

    pub fn get_mode_string(&self, device_number: i32, node_number: i32) -> CamIfaceResult<String> {
        const MAXLEN: i32 = 255;
        let mut mode_string_arr = [0_i8; MAXLEN as usize + 1]; // ensure trailing null
        check_camiface_error!{ ffi::cam_iface_get_mode_string(device_number,
                                                          node_number,
                                                          &mut mode_string_arr[0],
                                                          MAXLEN)};
        let result: String = try!(c_chars_to_str(&mode_string_arr));
        Ok(result)
    }


    pub fn get_camera_info(&self, cam_num: i32) -> CamIfaceResult<CamInfo> {

        let mut data: ffi::Camwire_id = Default::default();
        check_camiface_error!{ffi::cam_iface_get_camera_info(cam_num, &mut data)};

        Ok(CamInfo {
            vendor: try!(c_chars_to_str(&data.vendor)),
            model: try!(c_chars_to_str(&data.model)),
            chip: try!(c_chars_to_str(&data.chip)),
        })
    }
}

impl Drop for CamIface {
    fn drop(&mut self) {
        debug!("cam_iface_shutdown()");
        unsafe {
            ffi::cam_iface_shutdown();
        }
    }
}

impl CamContext {
    pub fn new(module: Arc<Mutex<CamIface>>,
               device_number: i32,
               num_buffers: i32,
               mode_number: i32)
               -> CamIfaceResult<CamContext> {

        let new_cam_context_r: Option<ffi::cam_iface_constructor_func_t>;

        new_cam_context_r = unsafe { ffi::cam_iface_get_constructor_func(device_number) };
        match new_cam_context_r {
            None => Err(CamIfaceError::FailedToGetConstructor),
            Some(new_cam_context) => {
                let ccc = (new_cam_context)(device_number, num_buffers, mode_number, ptr::null());
                err_on_camiface_error!();

                let roi = FrameROI {
                    left: 0,
                    top: 0,
                    width: 0,
                    height: 0,
                };
                let mut result = CamContext {
                    c_ptr: ccc,
                    module: module,

                    // below here are cached values filled by _update_cached_values()
                    roi: roi,
                    coding: PixelFormat::MONO8,
                    buffer_size: 0,
                };
                try!(result._update_cached_values());
                Ok(result)
            }
        }
    }

    fn _update_cached_values(&mut self) -> CamIfaceResult<()> {
        self.roi = try!(self._get_frame_roi());
        unsafe {
            self.coding = match (*self.c_ptr).coding {
                ffi::CameraPixelCoding::CAM_IFACE_MONO8 => PixelFormat::MONO8,
                ffi::CameraPixelCoding::CAM_IFACE_YUV411 => PixelFormat::YUV411,
                ffi::CameraPixelCoding::CAM_IFACE_YUV422 => PixelFormat::YUV422,
                ffi::CameraPixelCoding::CAM_IFACE_YUV444 => PixelFormat::YUV444,
                ffi::CameraPixelCoding::CAM_IFACE_RGB8 => PixelFormat::RGB8,
                ffi::CameraPixelCoding::CAM_IFACE_MONO16 |
                ffi::CameraPixelCoding::CAM_IFACE_MONO16S |
                ffi::CameraPixelCoding::CAM_IFACE_RGB16 |
                ffi::CameraPixelCoding::CAM_IFACE_RGB16S |
                ffi::CameraPixelCoding::CAM_IFACE_UNKNOWN |
                ffi::CameraPixelCoding::CAM_IFACE_RAW8 |
                ffi::CameraPixelCoding::CAM_IFACE_RAW16 |
                ffi::CameraPixelCoding::CAM_IFACE_ARGB8 => return Err(CamIfaceError::OtherError),
                ffi::CameraPixelCoding::CAM_IFACE_MONO8_BAYER_BGGR |
                ffi::CameraPixelCoding::CAM_IFACE_MONO8_BAYER_RGGB |
                ffi::CameraPixelCoding::CAM_IFACE_MONO8_BAYER_GRBG |
                ffi::CameraPixelCoding::CAM_IFACE_MONO8_BAYER_GBRG => PixelFormat::BayerBG8,
            };
        }
        self.buffer_size = try!(self._get_buffer_size()) as u32;
        Ok(())
    }

    pub fn get_num_camera_properties(&mut self) -> CamIfaceResult<i32> {
        let mut nprops = 0;
        check_camiface_error!{
      ( (*(*self.c_ptr).vmt).get_num_camera_properties)( &mut (*self.c_ptr), &mut nprops ) };
        Ok(nprops)
    }

    fn _get_frame_roi(&mut self) -> CamIfaceResult<FrameROI> {
        let mut left = 0;
        let mut top = 0;
        let mut width = 0;
        let mut height = 0;
        check_camiface_error!{
      ( (*(*self.c_ptr).vmt).get_frame_roi)( &mut (*self.c_ptr),
                                             &mut left, &mut top,
                                             &mut width, &mut height ) };
        Ok(FrameROI {
            left: left,
            top: top,
            width: width,
            height: height,
        })
    }

    pub fn get_num_framebuffers(&mut self) -> CamIfaceResult<i32> {
        let mut val = 0;
        check_camiface_error!{
      ( (*(*self.c_ptr).vmt).get_num_framebuffers)( &mut (*self.c_ptr), &mut val ) };
        Ok(val)
    }

    pub fn get_camera_property_info(&mut self,
                                    property_number: i32)
                                    -> CamIfaceResult<CameraPropertyInfo> {
        let mut cp = ffi::CameraPropertyInfo {
            name: ptr::null(),
            is_present: 0,
            min_value: 0,
            max_value: 0,
            has_auto_mode: 0,
            has_manual_mode: 0,
            is_scaled_quantity: 0,

            scaled_unit_name: ptr::null(),
            scale_offset: 0.0,
            scale_gain: 0.0,
            original_value: 0,
            available: 0,
            readout_capable: 0,
            on_off_capable: 0,
            absolute_capable: 0,
            absolute_control_mode: 0,
            absolute_min_value: 0.0,
            absolute_max_value: 0.0,
        };
        check_camiface_error!{
      ( (*(*self.c_ptr).vmt).get_camera_property_info)( &mut (*self.c_ptr),
                                                        property_number,
                                                        &mut cp ) };

        let slice = unsafe { CStr::from_ptr(cp.name) };
        let name = str::from_utf8(slice.to_bytes()).expect("from UTF8").to_string();
        Ok(CameraPropertyInfo {
            name: name,
            prop_info: cp,
        })
    }

    pub fn get_camera_property(&mut self, property_number: i32) -> CamIfaceResult<CameraProperty> {
        let mut cp = CameraProperty {
            value: 0,
            auto: 0,
        };
        check_camiface_error!{
      ( (*(*self.c_ptr).vmt).get_camera_property)( &mut (*self.c_ptr),
                                                   property_number,
                                                   &mut cp.value,
                                                   &mut cp.auto ) };
        Ok(cp)
    }


    pub fn set_camera_property(&mut self,
                               property_number: i32,
                               cp: CameraProperty)
                               -> CamIfaceResult<()> {
        check_camiface_error!{
      ( (*(*self.c_ptr).vmt).set_camera_property)( &mut (*self.c_ptr),
                                                   property_number,
                                                   cp.value,
                                                   cp.auto ) };
        Ok(())
    }

    pub fn start_camera(&mut self) -> CamIfaceResult<i8> {
        check_camiface_error!{
      ( (*(*self.c_ptr).vmt).start_camera)( &mut (*self.c_ptr) ) };
        Ok(1) //dummy return value
    }

    pub fn stop_camera(&mut self) -> CamIfaceResult<i8> {
        check_camiface_error!{
      ( (*(*self.c_ptr).vmt).stop_camera)( &mut (*self.c_ptr) ) };
        Ok(1) //dummy return value
    }

    pub fn get_max_frame_size(&mut self) -> CamIfaceResult<(i32, i32)> {
        let mut val = (0, 0);
        check_camiface_error!{
      ( (*(*self.c_ptr).vmt).get_max_frame_size)( &mut (*self.c_ptr), &mut val.0, &mut val.1 ) };
        Ok(val)
    }

    fn _get_buffer_size(&mut self) -> CamIfaceResult<i32> {
        let mut val = 0;
        check_camiface_error!{
      ( (*(*self.c_ptr).vmt).get_buffer_size)( &mut (*self.c_ptr), &mut val ) };
        Ok(val)
    }

    pub fn get_last_timestamp(&mut self) -> CamIfaceResult<Timestamp> {
        let mut val = 0.0;
        check_camiface_error!{
      ( (*(*self.c_ptr).vmt).get_last_timestamp)( &mut (*self.c_ptr), &mut val ) };
        Ok(Timestamp::from_f64(val))
    }

    pub fn get_last_framenumber(&mut self) -> CamIfaceResult<u64> {
        let mut val = 0;
        check_camiface_error!{
      ( (*(*self.c_ptr).vmt).get_last_framenumber)( &mut (*self.c_ptr), &mut val ) };
        Ok(val as u64)
    }

    pub fn get_capture_blocking_into(&mut self,
                                     mut cd: CaptureData,
                                     timeout: f32)
                                     -> CamIfaceResult<CaptureData> {
        // FIXME: check size of cd.image_data
        let stride = self.roi.width;

        check_camiface_error!{
          ( (*(*self.c_ptr).vmt).grab_next_frame_blocking_with_stride)(
              &mut (*self.c_ptr), cd.image_data.as_mut_ptr(), stride as intptr_t, timeout ) };

        unsafe { cd.image_data.set_len(self.buffer_size as usize) };

        cd.stride = stride;
        cd.roi = self.roi;
        cd.image_data = cd.image_data;
        cd.timestamp = Some(try!(self.get_last_timestamp()));
        cd.framenumber = Some(try!(self.get_last_framenumber()));
        cd.coding = self.coding;
        Ok(cd)
    }

    pub fn get_capture_blocking(&mut self, timeout: f32) -> CamIfaceResult<CaptureData> {
        let dst: Vec<u8> = Vec::with_capacity(self.buffer_size as usize);
        let cd = CaptureData {
            stride: self.roi.width,
            roi: self.roi,
            image_data: dst,
            timestamp: None,
            framenumber: None,
            coding: self.coding,
        };
        self.get_capture_blocking_into(cd, timeout)
    }
}

// ---------------

#[cfg(test)]
mod tests {
    use super::{CamIface, Timestamp};

    #[test]
    fn multiple_inits() {
        let mut cam_iface1 = CamIface::new().unwrap(); // Initialization 1
        cam_iface1.get_driver_name().unwrap(); // prevent unused variable warning
        let result2 = CamIface::new(); // Initialization 2
        assert!(result2.is_err());
    }

    #[test]
    fn timestamp_conversion() {
        let ts1 = Timestamp::from_f64(1.1);
        let eps = 10;
        assert!(ts1.secs == 1);
        assert!(((ts1.nsecs as i64) - 100000000) < eps);

        let ts1 = Timestamp::from_f64(1.2);
        assert!(ts1.secs == 1);
        assert!(((ts1.nsecs as i64) - 200000000) < eps);

        let ts1 = Timestamp::from_f64(3.0);
        assert!(ts1.secs == 3);
        assert!(((ts1.nsecs as i64) - 0) < eps);

    }
}
