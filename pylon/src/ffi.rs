#![allow(dead_code,non_camel_case_types)]

use std::os::raw::{c_char, c_void, c_uint, c_int};
use ::libc::size_t;

pub type StreamBufferHandle = *mut c_void;
pub type EGrabStatus = i8;
pub type PixelType = i8;

pub const EGRAB_STATUS_UNDEFINED_GRAB_STATUS: i8 = -1;
pub const EGRAB_STATUS_IDLE: i8 = 0;
pub const EGRAB_STATUS_QUEUED: i8 = 1;
pub const EGRAB_STATUS_GRABBED: i8 = 2;
pub const EGRAB_STATUS_CANCELED: i8 = 3;
pub const EGRAB_STATUS_FAILED: i8 = 4;

pub const EVISIBILITY_BEGINNER: i8 = 0;
pub const EVISIBILITY_EXPERT: i8 = 1;
pub const EVISIBILITY_GURU: i8 = 2;
pub const EVISIBILITY_INVISIBLE: i8 = 3;
pub const EVISIBILITY_UNDEFINED: i8 = 99;

pub const PIXELTYPE_UNDEFINED: i8 = -1;
pub const PIXELTYPE_MONO1PACKED: i8 = 0;
pub const PIXELTYPE_MONO2PACKED: i8 = 1;
pub const PIXELTYPE_MONO4PACKED: i8 = 2;
pub const PIXELTYPE_MONO8: i8 = 3;
pub const PIXELTYPE_MONO8SIGNED: i8 = 4;
pub const PIXELTYPE_MONO10: i8 = 5;
pub const PIXELTYPE_MONO10PACKED: i8 = 6;
pub const PIXELTYPE_MONO10P: i8 = 7;
pub const PIXELTYPE_MONO12: i8 = 8;
pub const PIXELTYPE_MONO12PACKED: i8 = 9;
pub const PIXELTYPE_MONO12P: i8 = 10;
pub const PIXELTYPE_MONO16: i8 = 11;
pub const PIXELTYPE_BAYERGR8: i8 = 12;
pub const PIXELTYPE_BAYERRG8: i8 = 13;
pub const PIXELTYPE_BAYERGB8: i8 = 14;
pub const PIXELTYPE_BAYERBG8: i8 = 15;
pub const PIXELTYPE_BAYERGR10: i8 = 16;
pub const PIXELTYPE_BAYERRG10: i8 = 17;
pub const PIXELTYPE_BAYERGB10: i8 = 18;
pub const PIXELTYPE_BAYERBG10: i8 = 19;
pub const PIXELTYPE_BAYERGR12: i8 = 20;
pub const PIXELTYPE_BAYERRG12: i8 = 21;
pub const PIXELTYPE_BAYERGB12: i8 = 22;
pub const PIXELTYPE_BAYERBG12: i8 = 23;
pub const PIXELTYPE_RGB8PACKED: i8 = 24;
pub const PIXELTYPE_BGR8PACKED: i8 = 25;
pub const PIXELTYPE_RGBA8PACKED: i8 = 26;
pub const PIXELTYPE_BGRA8PACKED: i8 = 27;
pub const PIXELTYPE_RGB10PACKED: i8 = 28;
pub const PIXELTYPE_BGR10PACKED: i8 = 29;
pub const PIXELTYPE_RGB12PACKED: i8 = 30;
pub const PIXELTYPE_BGR12PACKED: i8 = 31;
pub const PIXELTYPE_RGB16PACKED: i8 = 32;
pub const PIXELTYPE_BGR10V1PACKED: i8 = 33;
pub const PIXELTYPE_BGR10V2PACKED: i8 = 34;
pub const PIXELTYPE_YUV411PACKED: i8 = 35;
pub const PIXELTYPE_YUV422PACKED: i8 = 36;
pub const PIXELTYPE_YUV444PACKED: i8 = 37;
pub const PIXELTYPE_RGB8PLANAR: i8 = 38;
pub const PIXELTYPE_RGB10PLANAR: i8 = 39;
pub const PIXELTYPE_RGB12PLANAR: i8 = 40;
pub const PIXELTYPE_RGB16PLANAR: i8 = 41;
pub const PIXELTYPE_YUV422_YUYV_PACKED: i8 = 42;
pub const PIXELTYPE_BAYERGR12PACKED: i8 = 43;
pub const PIXELTYPE_BAYERRG12PACKED: i8 = 44;
pub const PIXELTYPE_BAYERGB12PACKED: i8 = 45;
pub const PIXELTYPE_BAYERBG12PACKED: i8 = 46;
pub const PIXELTYPE_BAYERGR10P: i8 = 47;
pub const PIXELTYPE_BAYERRG10P: i8 = 48;
pub const PIXELTYPE_BAYERGB10P: i8 = 49;
pub const PIXELTYPE_BAYERBG10P: i8 = 50;
pub const PIXELTYPE_BAYERGR12P: i8 = 51;
pub const PIXELTYPE_BAYERRG12P: i8 = 52;
pub const PIXELTYPE_BAYERGB12P: i8 = 53;
pub const PIXELTYPE_BAYERBG12P: i8 = 54;
pub const PIXELTYPE_BAYERGR16: i8 = 55;
pub const PIXELTYPE_BAYERRG16: i8 = 56;
pub const PIXELTYPE_BAYERGB16: i8 = 57;
pub const PIXELTYPE_BAYERBG16: i8 = 58;
pub const PIXELTYPE_RGB12V1PACKED: i8 = 59;
pub const PIXELTYPE_DOUBLE: i8 = 60;

#[allow(non_upper_case_globals)]
pub const PayloadType_Undefined: EPayloadType = -1;
#[allow(non_upper_case_globals)]
pub const PayloadType_Image: EPayloadType = 0;
#[allow(non_upper_case_globals)]
pub const PayloadType_RawData: EPayloadType = 1;
#[allow(non_upper_case_globals)]
pub const PayloadType_File: EPayloadType = 2;
#[allow(non_upper_case_globals)]
pub const PayloadType_ChunkData: EPayloadType = 3;
#[allow(non_upper_case_globals)]
pub const PayloadType_DeviceSpecific: EPayloadType = 0x8000;

#[derive(Copy, Clone)]
#[repr(u32)]
#[derive(Debug)]
pub enum PylonCppError_t {
    PYLONCPPWRAP_NO_ERROR = 0,
    PYLONCPPWRAP_ERROR_ENUM_NOT_MATCHED = 1,
    PYLONCPPWRAP_ERROR_CALLBACK_FAIL = 2,
    PYLONCPPWRAP_ERROR_NAME_NOT_FOUND = 3,
    PYLONCPPWRAP_ERROR_NULL_POINTER = 4,
    PYLONCPPWRAP_ERROR_PYLON_EXCEPTION = 5,
    PYLONCPPWRAP_ERROR_INVALID_RESULT = 6,
}

// opaque types
//   std namespace
pub(crate) enum CppStdString {}

//   Pylon namespace
pub enum CTlFactory {}
pub enum CDeviceInfo {}
pub enum IPylonDevice {}
pub enum IStreamGrabber {}
pub enum WaitObject {}
pub enum GrabResult {}
pub type EPayloadType = i32;
pub enum IGigETransportLayer {}
pub enum RefHolder {}

// pub enum StreamBufferHandle {}
//   GenApi namespace
pub enum INodeMap {}
pub enum INode {}
pub enum IInteger {}
pub enum IBoolean {}
pub enum IFloat {}
pub enum IString {}
pub enum IEnumeration {}
pub enum ICommand {}

// TODO: switch everything here from pub to pub(crate).

extern "C" {
    pub(crate) fn CppStdString_new() -> *mut CppStdString;
    pub(crate) fn CppStdString_delete(s: *mut CppStdString);
    pub(crate) fn CppStdString_bytes(s: *mut CppStdString) -> *const c_char;

    pub fn Pylon_initialize() -> PylonCppError_t;
    pub fn Pylon_terminate() -> PylonCppError_t;
    pub fn Pylon_getVersionString(sptr: *mut *const c_char) -> PylonCppError_t;
    pub fn CPylon_new_tl_factory(handle: *mut *mut CTlFactory) -> PylonCppError_t;
    pub fn CTlFactory_create_gige_transport_layer(tl_factory: *mut CTlFactory,
                                                  handle: *mut *mut IGigETransportLayer)
                                                  -> PylonCppError_t;
    pub fn IGigETransportLayer_node_map(tl: *mut IGigETransportLayer,
                                        handle: *mut *mut INodeMap)
                                        -> PylonCppError_t;
    pub fn CTlFactory_enumerate_devices(tl_factory: *mut CTlFactory,
                                        cb: extern "C" fn(*mut c_void, *mut CDeviceInfo) -> u8,
                                        target: *mut c_void)
                                        -> PylonCppError_t;
    pub fn CTlFactory_create_device(tl_factory: *mut CTlFactory,
                                    info: *mut CDeviceInfo,
                                    handle: *mut *mut IPylonDevice,
                                    err_msg: *mut c_char,
                                    err_msg_maxlen: c_int,
                                    )
                                    -> PylonCppError_t;

    pub fn IPylonDevice_open(device: *mut IPylonDevice, modeset: u64) -> PylonCppError_t;
    pub fn IPylonDevice_close(device: *mut IPylonDevice) -> PylonCppError_t;
    pub fn IPylonDevice_num_stream_grabber_channels(device: *mut IPylonDevice,
                                                    result: *mut usize)
                                                    -> PylonCppError_t;
    pub fn IPylonDevice_stream_grabber(device: *mut IPylonDevice,
                                       index: usize,
                                       handle: *mut *mut IStreamGrabber)
                                       -> PylonCppError_t;
    pub fn IPylonDevice_node_map(device: *mut IPylonDevice,
                                 handle: *mut *mut INodeMap)
                                 -> PylonCppError_t;
    pub fn INodeMap_get_nodes(node_map: *mut INodeMap,
                              cb: extern "C" fn(*mut c_void, *mut INode) -> u8,
                              target: *mut c_void)
                              -> PylonCppError_t;
    pub fn INodeMap_node(node_map: *mut INodeMap,
                         name: *const c_char,
                         handle: *mut *mut INode)
                         -> PylonCppError_t;
    pub fn INode_get_name(node: *mut INode,
                          fully_qualified: bool,
                          value: *mut c_char,
                          maxlen: usize)
                          -> PylonCppError_t;
    pub fn INode_get_visibility(node: *mut INode, visibility: *mut i8) -> PylonCppError_t;
    pub fn INode_principal_interface_type(node: *mut INode, value: *mut u8) -> PylonCppError_t;
    pub fn INode_to_integer_node(handle: *mut *mut INode,
                                 handle2: *mut *mut IInteger)
                                 -> PylonCppError_t;
    pub fn INode_to_boolean_node(handle: *mut *mut INode,
                                 handle2: *mut *mut IBoolean)
                                 -> PylonCppError_t;
    pub fn INode_to_float_node(handle: *mut *mut INode,
                               handle2: *mut *mut IFloat)
                               -> PylonCppError_t;
    pub fn INode_to_string_node(handle: *mut *mut INode,
                                handle2: *mut *mut IString)
                                -> PylonCppError_t;
    pub fn INode_to_enumeration_node(handle: *mut *mut INode,
                                     handle2: *mut *mut IEnumeration)
                                     -> PylonCppError_t;
    pub fn INode_to_command_node(handle: *mut *mut INode,
                                 handle2: *mut *mut ICommand)
                                 -> PylonCppError_t;

    pub fn IInteger_get_value(node: *mut IInteger, value: *mut i64) -> PylonCppError_t;
    pub fn IInteger_get_range(node: *mut IInteger, min: *mut i64, max: *mut i64) -> PylonCppError_t;
    pub fn IInteger_set_value(node: *mut IInteger, value: i64) -> PylonCppError_t;
    pub fn IBoolean_get_value(node: *mut IBoolean, value: *mut bool) -> PylonCppError_t;
    pub fn IBoolean_set_value(node: *mut IBoolean, value: bool) -> PylonCppError_t;
    pub fn IFloat_get_value(node: *mut IFloat, value: *mut f64) -> PylonCppError_t;
    pub fn IFloat_get_range(node: *mut IFloat, min: *mut f64, max: *mut f64) -> PylonCppError_t;
    pub fn IFloat_set_value(node: *mut IFloat, value: f64) -> PylonCppError_t;
    pub fn IString_get_value(node: *mut IString,
                             value: *mut c_char,
                             maxlen: usize)
                             -> PylonCppError_t;
    pub fn IString_set_value(node: *mut IString, value: *const c_char) -> PylonCppError_t;
    pub fn IEnumeration_get_value(node: *mut IEnumeration,
                                  value: *mut c_char,
                                  maxlen: usize)
                                  -> PylonCppError_t;
    pub fn IEnumeration_set_value(node: *mut IEnumeration,
                                  value: *const c_char)
                                  -> PylonCppError_t;
    pub fn IEnumeration_get_entries(node_map: *mut IEnumeration,
                              cb: extern "C" fn(*mut c_void, *mut INode) -> u8,
                              target: *mut c_void)
                              -> PylonCppError_t;

    pub fn ICommand_execute(node: *mut ICommand) -> PylonCppError_t;

    pub fn CDeviceInfo_delete(device_info: *mut CDeviceInfo) -> PylonCppError_t;

    pub fn IProperties_get_property_names(prop: *const CDeviceInfo,
                                          cb: extern "C" fn(*mut c_void, *const c_char) -> u8,
                                          target: *mut c_void)
                                          -> PylonCppError_t;
    pub fn IProperties_get_property_value(prop: *const CDeviceInfo,
                                          name: *const c_char,
                                          value: *mut c_char,
                                          maxlen: usize)
                                          -> PylonCppError_t;
    pub fn IStreamGrabber_open(grabber: *mut IStreamGrabber) -> PylonCppError_t;
    pub fn IStreamGrabber_close(grabber: *mut IStreamGrabber) -> PylonCppError_t;
    pub fn IStreamGrabber_node_map(grabber: *mut IStreamGrabber,
                                   handle: *mut *mut INodeMap)
                                   -> PylonCppError_t;
    pub fn IStreamGrabber_prepare_grab(grabber: *mut IStreamGrabber) -> PylonCppError_t;
    pub fn IStreamGrabber_cancel_grab(grabber: *mut IStreamGrabber) -> PylonCppError_t;
    pub fn IStreamGrabber_finish_grab(grabber: *mut IStreamGrabber) -> PylonCppError_t;
    pub fn IStreamGrabber_register_buffer(grabber: *mut IStreamGrabber,
                                          buffer: *mut u8,
                                          buffer_size: usize,
                                          result: *mut StreamBufferHandle)
                                          -> PylonCppError_t;
    pub fn IStreamGrabber_queue_buffer(grabber: *mut IStreamGrabber,
                                       handle: StreamBufferHandle,
                                       err_msg: *mut c_char,
                                       err_msg_maxlen: c_int)
                                       -> PylonCppError_t;
    pub fn IStreamGrabber_get_wait_object(grabber: *mut IStreamGrabber,
                                          handle: *mut *mut WaitObject)
                                          -> PylonCppError_t;
    pub fn IStreamGrabber_retrieve_result(grabber: *mut IStreamGrabber,
                                          handle: *mut *mut GrabResult,
                                          is_ready: *mut bool)
                                          -> PylonCppError_t;
    pub fn GrabResult_get_buffer(gr: *mut GrabResult,
                                 buffer: *mut *const u8,
                                 size: *mut i64)
                                 -> PylonCppError_t;
    pub fn GrabResult_get_payload_type(gr: *mut GrabResult, payload_type: *mut EPayloadType) -> PylonCppError_t;
    pub fn GrabResult_delete(gr: *mut GrabResult) -> PylonCppError_t;
    pub fn GrabResult_status(gr: *mut GrabResult, status: *mut EGrabStatus) -> PylonCppError_t;
    pub fn GrabResult_error_code(gr: *mut GrabResult, result: *mut u32) -> PylonCppError_t;
    pub(crate) fn GrabResult_error_description(gr: *mut GrabResult, result: *mut CppStdString)
                                         -> PylonCppError_t;

    pub fn GrabResult_payload_size(gr: *mut GrabResult, result: *mut size_t) -> PylonCppError_t;
    pub fn GrabResult_size_x(gr: *mut GrabResult, result: *mut i32) -> PylonCppError_t;
    pub fn GrabResult_size_y(gr: *mut GrabResult, result: *mut i32) -> PylonCppError_t;
    pub fn GrabResult_time_stamp(gr: *mut GrabResult, result: *mut u64) -> PylonCppError_t;
    pub fn GrabResult_block_id(gr: *mut GrabResult, result: *mut u64) -> PylonCppError_t;
    pub fn GrabResult_image(gr: *mut GrabResult, handle: *mut *mut RefHolder) -> PylonCppError_t;
    pub fn GrabResult_handle(gr: *mut GrabResult,
                             result: *mut StreamBufferHandle)
                             -> PylonCppError_t;
    pub fn RefHolder_delete(s: *mut RefHolder);

    pub fn CGrabResultImageRef_is_valid(im: *mut RefHolder, result: *mut bool) -> PylonCppError_t;
    pub fn CGrabResultImageRef_get_pixel_type(im: *mut RefHolder, result: *mut PixelType) -> PylonCppError_t;
    pub fn CGrabResultImageRef_get_width(im: *mut RefHolder, result: *mut u32) -> PylonCppError_t;
    pub fn CGrabResultImageRef_get_height(im: *mut RefHolder, result: *mut u32) -> PylonCppError_t;
    pub fn CGrabResultImageRef_get_buffer(im: *mut RefHolder, result: *mut *const c_void) -> PylonCppError_t;
    pub fn CGrabResultImageRef_get_image_size(im: *mut RefHolder, result: *mut size_t) -> PylonCppError_t;
    pub fn CGrabResultImageRef_get_stride(im: *mut RefHolder, result: *mut size_t) -> PylonCppError_t;
    pub fn WaitObject_wait(wait_object: *mut WaitObject,
                           timeout_msec: c_uint,
                           result: *mut bool)
                           -> PylonCppError_t;
}
