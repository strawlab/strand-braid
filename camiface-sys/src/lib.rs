#![allow(non_snake_case,non_camel_case_types)]

extern crate libc;

use libc::{c_int, c_uchar, c_char, c_ulong, c_long, c_double, c_float, intptr_t};

pub const CAMWIRE_ID_MAX_CHARS: usize = 100;
pub const CAM_IFACE_FRAME_DATA_MISSING_ERROR: i32 = -392073;
pub const CAM_IFACE_FRAME_TIMEOUT: i32 = -392074;
pub const CAM_IFACE_FRAME_DATA_LOST_ERROR: i32 = -392075;
pub const CAM_IFACE_HARDWARE_FEATURE_NOT_AVAILABLE: i32 = -392076;
pub const CAM_IFACE_OTHER_ERROR: i32 = -392077;
pub const CAM_IFACE_FRAME_INTERRUPTED_SYSCALL: i32 = -392078;
pub const CAM_IFACE_SELECT_RETURNED_BUT_NO_FRAME_AVAILABLE: i32 = -392079;
pub const CAM_IFACE_FRAME_DATA_CORRUPT_ERROR: i32 = -392080;
pub const CAM_IFACE_BUFFER_OVERFLOW_ERROR: i32 = -392081;
pub const CAM_IFACE_CAMERA_NOT_AVAILABLE_ERROR: i32 = -392082;
pub const CAM_IFACE_GENERIC_ERROR: i32 = -1;

#[repr(C)]
pub struct Camwire_id {
    pub vendor: [c_char; CAMWIRE_ID_MAX_CHARS + 1],
    pub model: [c_char; CAMWIRE_ID_MAX_CHARS + 1],
    pub chip: [c_char; CAMWIRE_ID_MAX_CHARS + 1],
}

pub type voidptr_t = intptr_t;

pub type cam_iface_constructor_func_t = extern "C" fn(c_int, c_int, c_int, *const c_char)
                                                      -> *mut CamContext;

#[repr(C)]
pub struct CameraPropertyInfo {
    pub name: *const c_char,
    pub is_present: c_int,

    pub min_value: c_long,
    pub max_value: c_long,
    pub has_auto_mode: c_int,
    pub has_manual_mode: c_int,
    pub is_scaled_quantity: c_int,

    pub scaled_unit_name: *const c_char,
    pub scale_offset: c_double,
    pub scale_gain: c_double,
    pub original_value: c_long,
    pub available: c_int,
    pub readout_capable: c_int,
    pub on_off_capable: c_int,
    pub absolute_capable: c_int,
    pub absolute_control_mode: c_int,
    pub absolute_min_value: c_double,
    pub absolute_max_value: c_double,
}

#[repr(C)]
pub struct CamContext_functable {
    pub construct: cam_iface_constructor_func_t,
    pub destroy: extern "C" fn(*mut CamContext),

    pub CamContext: extern "C" fn(*mut CamContext, c_int, c_int, *const c_char),
    pub close: extern "C" fn(*mut CamContext),

    pub start_camera: extern "C" fn(*mut CamContext),
    pub stop_camera: extern "C" fn(*mut CamContext),

    pub get_num_camera_properties: extern "C" fn(*mut CamContext, *mut c_int),
    pub get_camera_property_info: extern "C" fn(*mut CamContext,
                                                property_number: c_int,
                                                value: *mut CameraPropertyInfo),
    pub get_camera_property: extern "C" fn(*mut CamContext,
                                           property_number: c_int,
                                           value: *mut c_long,
                                           auto: *mut c_int),
    pub set_camera_property: extern "C" fn(*mut CamContext,
                                           property_number: c_int,
                                           value: c_long,
                                           auto: c_int),
    pub grab_next_frame_blocking: extern "C" fn(*mut CamContext, *mut c_uchar, c_float),
    pub grab_next_frame_blocking_with_stride: extern "C" fn(*mut CamContext,
                                                            *mut c_uchar,
                                                            intptr_t,
                                                            c_float),
    pub point_next_frame_blocking: extern "C" fn(*mut CamContext, *mut c_uchar, c_float),
    pub unpoint_frame: extern "C" fn(*mut CamContext),
    pub get_last_timestamp: extern "C" fn(*mut CamContext, *mut c_double),
    pub get_last_framenumber: extern "C" fn(*mut CamContext, *mut c_ulong),
    pub get_num_trigger_modes: extern "C" fn(*mut CamContext, *mut c_int),
    pub get_trigger_mode_string: extern "C" fn(*mut CamContext, c_int, *mut c_char, c_int),
    pub get_trigger_mode_number: extern "C" fn(*mut CamContext, c_int, *mut c_int),
    pub set_trigger_mode_number: extern "C" fn(*mut CamContext, c_int, c_int),
    pub get_frame_roi: extern "C" fn(*mut CamContext,
                                     *mut c_int,
                                     *mut c_int,
                                     *mut c_int,
                                     *mut c_int),
    pub set_frame_roi: extern "C" fn(*mut CamContext, c_int, c_int, c_int, c_int),
    pub get_max_frame_size: extern "C" fn(*mut CamContext, *mut c_int, *mut c_int),
    pub get_buffer_size: extern "C" fn(*mut CamContext, *mut c_int),
    pub get_framerate: extern "C" fn(*mut CamContext, *mut c_float),
    pub set_framerate: extern "C" fn(*mut CamContext, c_float),
    pub get_num_framebuffers: extern "C" fn(*mut CamContext, *mut c_int),
    pub set_num_framebuffers: extern "C" fn(*mut CamContext, c_int),
}

#[repr(C)]
pub struct CamContext {
    pub vmt: *mut CamContext_functable, // ...
    pub cam: voidptr_t,
    pub backend_extras: voidptr_t,
    pub coding: CameraPixelCoding,
}

#[repr(C)]
pub enum CameraPixelCoding {
    CAM_IFACE_UNKNOWN = 0,
    CAM_IFACE_MONO8, // pure monochrome (no Bayer)
    CAM_IFACE_YUV411,
    CAM_IFACE_YUV422,
    CAM_IFACE_YUV444,
    CAM_IFACE_RGB8,
    CAM_IFACE_MONO16,
    CAM_IFACE_RGB16,
    CAM_IFACE_MONO16S,
    CAM_IFACE_RGB16S,
    CAM_IFACE_RAW8,
    CAM_IFACE_RAW16,
    CAM_IFACE_ARGB8,
    CAM_IFACE_MONO8_BAYER_BGGR, // BGGR Bayer coding
    CAM_IFACE_MONO8_BAYER_RGGB, // RGGB Bayer coding
    CAM_IFACE_MONO8_BAYER_GRBG, // GRBG Bayer coding
    CAM_IFACE_MONO8_BAYER_GBRG, // GBRG Bayer coding
}

impl Default for Camwire_id {
    fn default() -> Camwire_id {
        Camwire_id {
            vendor: [0; CAMWIRE_ID_MAX_CHARS + 1], // ensure trailing null
            model: [0; CAMWIRE_ID_MAX_CHARS + 1], // ensure trailing null
            chip: [0; CAMWIRE_ID_MAX_CHARS + 1], // ensure trailing null
        }
    }
}

extern "C" {
    pub fn cam_iface_get_api_version() -> *const c_char;
    pub fn cam_iface_get_driver_name() -> *const c_char;
    pub fn cam_iface_get_error_string() -> *const c_char;
    pub fn cam_iface_get_num_cameras() -> c_int;
    pub fn cam_iface_get_num_modes(device_number: c_int, num_modes: *mut c_int);
    pub fn cam_iface_get_mode_string(device_number: c_int,
                                     node_number: c_int,
                                     mode_string: *mut c_char,
                                     mode_string_maxlen: c_int);

    pub fn cam_iface_get_camera_info(cam_num: c_int, data: *mut Camwire_id) -> c_int;
    pub fn cam_iface_have_error() -> c_int;
    pub fn cam_iface_clear_error();
    pub fn cam_iface_startup();
    pub fn cam_iface_shutdown();

    pub fn cam_iface_get_constructor_func(device_number: c_int)
                                          -> Option<cam_iface_constructor_func_t>;
}
