#[macro_use]
extern crate log;
extern crate failure;
extern crate libc;
#[macro_use]
extern crate failure_derive;

use std::ffi::CStr;
use std::os::raw::{c_char, c_uint, c_void};

mod ffi;

// ---------------------------
// errors

pub type Result<M> = std::result::Result<M,Error>;

const ERRBUFLEN: usize = 255;

#[derive(Fail, Debug)]
pub enum Error {
    #[fail(display = "EnumNotMatched")]
    EnumNotMatched,
    #[fail(display = "CallbackFail")]
    CallbackFail,
    #[fail(display = "NameNotFound")]
    NameNotFound,
    #[fail(display = "NullPointer")]
    NullPointer,
    #[fail(display = "PylonException")]
    PylonException,
    #[fail(display = "InvalidResult")]
    InvalidResult,
    #[fail(display = "PylonExceptionDescr {}", _0)]
    PylonExceptionDescr(String),
    #[fail(display = "WrongNodeType {}", _0)]
    WrongNodeType(String),
    #[fail(display = "{}", _0)]
    Utf8Error(#[cause] std::str::Utf8Error),
}

fn str_from_u8_nul_utf8(utf8_src: &[u8]) -> std::result::Result<&str, std::str::Utf8Error> {
    let nul_range_end = utf8_src.iter()
        .position(|&c| c == b'\0')
        .unwrap_or(utf8_src.len()); // default to length if no `\0` present
    ::std::str::from_utf8(&utf8_src[0..nul_range_end])
}

macro_rules! pylon_try {
    ($x:expr) => {
        trace!("calling pylon_try in {}:{}", file!(), line!());
        match unsafe { $x } {
            ffi::PylonCppError_t::PYLONCPPWRAP_NO_ERROR => {
                trace!("  pylon_try OK");
            },
            ffi::PylonCppError_t::PYLONCPPWRAP_ERROR_ENUM_NOT_MATCHED => {
                return Err(Error::EnumNotMatched)
            },
            ffi::PylonCppError_t::PYLONCPPWRAP_ERROR_CALLBACK_FAIL => {
                return Err(Error::CallbackFail)
            },
            ffi::PylonCppError_t::PYLONCPPWRAP_ERROR_NAME_NOT_FOUND => {
                return Err(Error::NameNotFound)
            },
            ffi::PylonCppError_t::PYLONCPPWRAP_ERROR_NULL_POINTER => {
                return Err(Error::NullPointer)
            },
            ffi::PylonCppError_t::PYLONCPPWRAP_ERROR_PYLON_EXCEPTION => {
                return Err(Error::PylonException)
            },
            ffi::PylonCppError_t::PYLONCPPWRAP_ERROR_INVALID_RESULT => {
                return Err(Error::InvalidResult)
            },
        }
    }
}

macro_rules! pylon_try_panic {
    ($x:expr, $chain:expr) => {
        trace!("calling pylon_try_panic on {} in {}:{}", $chain, file!(), line!());
        match unsafe { $x } {
            ffi::PylonCppError_t::PYLONCPPWRAP_NO_ERROR => {
                trace!("  pylon_try_panic OK");
            },
            e => {
                // We could do something particular with the specific
                // error (see pylon_try), but what?
                trace!("  pylon_try_panic err");
                panic!("got error {:?}", e);
            },
        }
    }
}

// ---------------------------
// c++ std

struct CppStringWrap {
    inner: *mut ffi::CppStdString,
}

impl CppStringWrap {
    fn new() -> Self {
        Self {
            inner: unsafe{ ffi::CppStdString_new() },
        }
    }
}

impl Drop for CppStringWrap {
    fn drop(&mut self) {
        unsafe{ ffi::CppStdString_delete(self.inner) };
    }
}


// ---------------------------
// Pylon

pub struct Pylon {}

impl Pylon {
    pub fn new() -> Result<Pylon> {
        pylon_try!(ffi::Pylon_initialize());
        Ok(Pylon {})
    }
    pub fn tl_factory(&self) -> Result<TLFactory> {
        let mut inner: *mut ffi::CTlFactory = std::ptr::null_mut();
        pylon_try!(ffi::CPylon_new_tl_factory(&mut inner));
        Ok(TLFactory { inner: inner })
    }
}
impl Drop for Pylon {
    fn drop(&mut self) {
        pylon_try_panic!(ffi::Pylon_terminate(), "Pylon_terminate");
    }
}

// ---------------------------

pub fn version_string() -> Result<&'static std::ffi::CStr> {
    let mut sptr: *const c_char = std::ptr::null_mut();
    // Pylon_getVersionString returns a pointer to static memory
    pylon_try_panic!(ffi::Pylon_getVersionString(&mut sptr), "Pylon_getVersionString");
    let slice = unsafe { std::ffi::CStr::from_ptr(sptr) } ;
    Ok(slice)
}

// ---------------------------

pub struct DeviceInfoList {
    pub a: Vec<DeviceInfo>,
}

pub struct NodeList {
    pub a: Vec<Node>,
}

// ---------------------------
// DeviceInfo

pub struct DeviceInfo {
    inner: *mut ffi::CDeviceInfo,
}

// ---------------------------
// HasProperties trait

pub trait HasProperties {
    fn property_names(&self) -> Result<Vec<String>>;
    fn property_value(&self, &str) -> Result<String>;
}

impl HasProperties for DeviceInfo {
    fn property_names(&self) -> Result<Vec<String>> {

        extern "C" fn convert_and_append_name(target_c: *mut c_void, value: *const c_char) -> u8 {
            let result = std::panic::catch_unwind(|| {
                let mut target = unsafe { Box::from_raw(target_c as *mut Vec<String>) };

                let val_cstr = unsafe { CStr::from_ptr(value) };
                let v = val_cstr.to_str().expect("failed to convert to str");
                (*target).push(v.to_string());
                std::mem::forget(target); // don't drop target
            });
            match result {
                Ok(_) => 0, // OK
                Err(_) => 1,
            }
        }

        let target = Box::new(Vec::new());
        let raw_target = Box::into_raw(target);
        let inner_const = self.inner as *const ffi::CDeviceInfo;

        pylon_try!(ffi::IProperties_get_property_names(inner_const,
                                                             convert_and_append_name,
                                                             raw_target as *mut c_void));
        let target = unsafe { Box::from_raw(raw_target) };
        Ok(*target) // unbox
    }

    fn property_value(&self, name: &str) -> Result<String> {
        let inner_const = self.inner as *const ffi::CDeviceInfo;
        let name_cstr = std::ffi::CString::new(name).unwrap();
        const MAXLEN: usize = 300;
        let mut value: [c_char; MAXLEN] = [0; MAXLEN];
        pylon_try!(ffi::IProperties_get_property_value(inner_const,
                                                             name_cstr.as_ptr(),
                                                             value.as_mut_ptr(),
                                                             MAXLEN));
        let val_cstr = unsafe { CStr::from_ptr(value.as_ptr()) };
        let v = val_cstr.to_str().map_err(|e| Error::Utf8Error(e))?;
        Ok(v.to_string())
    }
}
impl DeviceInfo {
    fn new(inner: *mut ffi::CDeviceInfo) -> DeviceInfo {
        DeviceInfo { inner: inner }
    }
}

impl Drop for DeviceInfo {
    fn drop(&mut self) {
        pylon_try_panic!(ffi::CDeviceInfo_delete(self.inner), "CDeviceInfo_delete");
        self.inner = std::ptr::null_mut();
    }
}

impl std::fmt::Debug for DeviceInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        fn fixerr(_orig_err: Error) -> std::fmt::Error {
            // TODO implement something more useful here
            std::fmt::Error::default()
        }

        let vendor = self.property_value("VendorName").map_err(fixerr)?;
        let model = self.property_value("ModelName").map_err(fixerr)?;
        let serial = self.property_value("SerialNumber").map_err(fixerr)?;
        write!(f,
               "DeviceInfo {{ VendorName: \"{}\", ModelName: \"{}\", SerialNumber: \"{}\" }}",
               vendor,
               model,
               serial)
    }
}

// ---------------------------
// GigETransportLayer

pub struct GigETransportLayer {
    inner: *mut ffi::IGigETransportLayer,
}

impl HasNodeMap for GigETransportLayer {
    fn node_map(&self) -> Result<NodeMap> {
        let mut result: *mut ffi::INodeMap = std::ptr::null_mut();
        pylon_try!(ffi::IGigETransportLayer_node_map(self.inner, &mut result));
        Ok(NodeMap { inner: result })
    }
}

// ---------------------------
// TLFactory

pub struct TLFactory {
    inner: *mut ffi::CTlFactory,
}

unsafe impl Send for TLFactory {}

impl TLFactory {
    pub fn create_gige_transport_layer(&self) -> Result<GigETransportLayer> {
        let mut result: *mut ffi::IGigETransportLayer = std::ptr::null_mut();

        pylon_try!(ffi::CTlFactory_create_gige_transport_layer(self.inner,
                                                  &mut result));
        Ok(GigETransportLayer {
               inner: result,
        })
    }

    pub fn enumerate_devices(&self) -> Result<Vec<DeviceInfo>> {

        extern "C" fn convert_and_append_device_info(target_c: *mut c_void,
                                                     val: *mut ffi::CDeviceInfo)
                                                     -> u8 {
            let result = std::panic::catch_unwind(|| {
                let mut target = unsafe { Box::from_raw(target_c as *mut DeviceInfoList) };
                let rval = DeviceInfo::new(val);
                (*target).a.push(rval);
                std::mem::forget(target); // don't drop target
            });
            match result {
                Ok(_) => 0, // OK
                Err(_) => 1,
            }
        }

        let target = Box::new(DeviceInfoList { a: Vec::new() });
        let raw_target = Box::into_raw(target);
        pylon_try!(ffi::CTlFactory_enumerate_devices(self.inner,
                                                           convert_and_append_device_info,
                                                           raw_target as *mut c_void));
        let target = unsafe { Box::from_raw(raw_target) };
        Ok((*target).a)
    }

    pub fn create_device(&self, info: &DeviceInfo) -> Result<Device> {
        let mut result: *mut ffi::IPylonDevice = std::ptr::null_mut();

        let mut err_msg: Vec<u8> = vec![0; ERRBUFLEN];
        let max_len = ERRBUFLEN;

        trace!("pylon_try calling CTlFactory_create_device in {}:{}", file!(), line!());
        let c_result = unsafe {
            ffi::CTlFactory_create_device(self.inner, info.inner, &mut result, err_msg.as_mut_ptr() as *mut c_char, max_len as i32)
        };
        trace!("pylon_try c_result {:?} CTlFactory_create_device in {}:{}", c_result, file!(), line!());

        match c_result {
            ffi::PylonCppError_t::PYLONCPPWRAP_NO_ERROR => {
                trace!("  pylon_try OK");
            },
            ffi::PylonCppError_t::PYLONCPPWRAP_ERROR_ENUM_NOT_MATCHED => {
                return Err(Error::EnumNotMatched)
            },
            ffi::PylonCppError_t::PYLONCPPWRAP_ERROR_CALLBACK_FAIL => {
                return Err(Error::CallbackFail)
            },
            ffi::PylonCppError_t::PYLONCPPWRAP_ERROR_NAME_NOT_FOUND => {
                return Err(Error::NameNotFound)
            },
            ffi::PylonCppError_t::PYLONCPPWRAP_ERROR_NULL_POINTER => {
                return Err(Error::NullPointer)
            },
            ffi::PylonCppError_t::PYLONCPPWRAP_ERROR_PYLON_EXCEPTION => {
                let cs = str_from_u8_nul_utf8(&err_msg).map_err(|e| Error::Utf8Error(e))?.to_string();
                return Err(Error::PylonExceptionDescr(cs))
            },
            ffi::PylonCppError_t::PYLONCPPWRAP_ERROR_INVALID_RESULT => {
                return Err(Error::InvalidResult)
            },
        }

        let serial = info.property_value("SerialNumber")?;
        Ok(Device {
               inner: result,
               _guid: serial,
           })
    }
}

// ---------------------------
// trait HasNodeMap

pub trait HasNodeMap {
    fn node_map(&self) -> Result<NodeMap>;

    /// shortcut helper to get integer GenApi node value
    fn integer_value(&self, param: &str) -> Result<i64> {
        let node_map = self.node_map()?;
        let node = node_map.node(param)?;
        let integer_node = node.to_integer_node()?;
        integer_node.value()
    }

    /// shortcut helper to get integer GenApi node range
    fn integer_range(&self, param: &str) -> Result<(i64, i64)> {
        let node_map = self.node_map()?;
        let node = node_map.node(param)?;
        let integer_node = node.to_integer_node()?;
        integer_node.range()
    }

    /// shortcut helper to get integer GenApi node value
    fn set_integer_value(&mut self, param: &str, value: i64) -> Result<()> {
        debug!("setting integer value {} to {}", param, value);
        let node_map = self.node_map()?;
        let node = node_map.node(param)?;
        let mut integer_node = node.to_integer_node()?;
        integer_node.set_value(value)
    }

    /// shortcut helper to get boolean GenApi node value
    fn boolean_value(&self, param: &str) -> Result<bool> {
        let node_map = self.node_map()?;
        let node = node_map.node(param)?;
        let boolean_node = node.to_boolean_node()?;
        boolean_node.value()
    }

    /// shortcut helper to get boolean GenApi node value
    fn set_boolean_value(&mut self, param: &str, value: bool) -> Result<()> {
        debug!("setting boolean value {} to {}", param, value);
        let node_map = self.node_map()?;
        let node = node_map.node(param)?;
        let mut boolean_node = node.to_boolean_node()?;
        boolean_node.set_value(value)
    }

    /// shortcut helper to get float GenApi node value
    fn float_value(&self, param: &str) -> Result<f64> {
        let node_map = self.node_map()?;
        let node = node_map.node(param)?;
        let float_node = node.to_float_node()?;
        float_node.value()
    }

    /// shortcut helper to get float GenApi node range
    fn float_range(&self, param: &str) -> Result<(f64, f64)> {
        let node_map = self.node_map()?;
        let node = node_map.node(param)?;
        let float_node = node.to_float_node()?;
        float_node.range()
    }

    /// shortcut helper to get float GenApi node value
    fn set_float_value(&mut self, param: &str, value: f64) -> Result<()> {
        debug!("setting float value {} to {}", param, value);
        let node_map = self.node_map()?;
        let node = node_map.node(param)?;
        let mut float_node = node.to_float_node()?;
        float_node.set_value(value)
    }

    fn string_value(&self, param: &str) -> Result<String> {
        let node_map = self.node_map()?;
        let node = node_map.node(param)?;
        let string_node = node.to_string_node()?;
        string_node.value()
    }

    fn set_string_value(&mut self, param: &str, value: &str) -> Result<()> {
        debug!("setting string value {} to {}", param, value);
        let node_map = self.node_map()?;
        let node = node_map.node(param)?;
        let mut string_node = node.to_string_node()?;
        string_node.set_value(value)
    }

    fn enumeration_value(&self, param: &str) -> Result<String> {
        let node_map = self.node_map()?;
        let node = node_map.node(param)?;
        let enumeration_node = node.to_enumeration_node()?;
        enumeration_node.value()
    }

    fn print_enumeration_entries(&self, param: &str) -> Result<()> {
        let node_map = self.node_map()?;
        let node = node_map.node(param)?;
        let enumeration_node = node.to_enumeration_node()?;
        let entries = enumeration_node.get_entries()?;
        println!("entries: {:?}", entries);
        Ok(())
    }

    fn get_enumeration_entries(&self, param: &str) -> Result<Vec<String>> {
        let node_map = self.node_map()?;
        let node = node_map.node(param)?;
        let enumeration_node = node.to_enumeration_node()?;
        let entries = enumeration_node.get_entries()?;
        let mut result = Vec::with_capacity(entries.len());
        for entry in entries.iter() {
            result.push(entry.name(false));
        }
        Ok(result)
    }

    fn set_enumeration_value(&mut self, param: &str, value: &str) -> Result<()> {
        debug!("setting enumeration value {} to {}", param, value);
        let node_map = self.node_map()?;
        let node = node_map.node(param)?;
        let mut enumeration_node = node.to_enumeration_node()?;
        enumeration_node.set_value(value)
    }

    fn execute_command(&mut self, param: &str) -> Result<()> {
        let node_map = self.node_map()?;
        let node = node_map.node(param)?;
        let command_node = node.to_command_node()?;
        command_node.execute()
    }
}
// ---------------------------
// AccessMode

pub enum AccessMode {
    Control,
    Stream,
    Event,
    Exclusive,
}

// ---------------------------
// Device

pub struct Device {
    inner: *mut ffi::IPylonDevice,
    _guid: String,
}

unsafe impl Send for Device {}

impl Device {
    pub fn open(&mut self, access_mode: Vec<AccessMode>) -> Result<()> {
        // Definition of these values taken from EDeviceAccessMode.
        let int_vals = access_mode
            .iter()
            .map(|m| match m {
                     &AccessMode::Control => 0x1,
                     &AccessMode::Stream => 0x3,
                     &AccessMode::Event => 0x4,
                     &AccessMode::Exclusive => 0x5,
                 })
            .collect::<Vec<u64>>();
        let access_mode_set: u64 = int_vals.iter().fold(0, |acc, &x| acc | x);
        pylon_try!(ffi::IPylonDevice_open(self.inner, access_mode_set));
        Ok(())
    }

    pub fn close(&mut self) -> Result<()> {
        pylon_try!(ffi::IPylonDevice_close(self.inner));
        Ok(())
    }

    pub fn num_stream_grabber_channels(&self) -> Result<usize> {
        let mut result = 0;
        pylon_try!(ffi::IPylonDevice_num_stream_grabber_channels(self.inner, &mut result));
        Ok(result)
    }

    pub fn stream_grabber(&self, index: usize) -> Result<StreamGrabber> {
        let mut result: *mut ffi::IStreamGrabber = std::ptr::null_mut();
        pylon_try!(ffi::IPylonDevice_stream_grabber(self.inner, index, &mut result));
        Ok(StreamGrabber {
               inner: result,
               buffers: Vec::new(),
           })
    }
}

impl HasNodeMap for Device {
    fn node_map(&self) -> Result<NodeMap> {
        let mut result: *mut ffi::INodeMap = std::ptr::null_mut();
        pylon_try!(ffi::IPylonDevice_node_map(self.inner, &mut result));
        Ok(NodeMap { inner: result })
    }
}

// ---------------------------
// StreamGrabber

pub struct StreamGrabber {
    inner: *mut ffi::IStreamGrabber,
    buffers: Vec<Buffer>,
}

unsafe impl Send for StreamGrabber {}

impl StreamGrabber {
    pub fn open(&mut self) -> Result<()> {
        pylon_try!(ffi::IStreamGrabber_open(self.inner));
        Ok(())
    }
    pub fn close(&mut self) -> Result<()> {
        pylon_try!(ffi::IStreamGrabber_close(self.inner));
        Ok(())
    }
    pub fn prepare_grab(&mut self) -> Result<()> {
        pylon_try!(ffi::IStreamGrabber_prepare_grab(self.inner));
        Ok(())
    }
    pub fn cancel_grab(&mut self) -> Result<()> {
        pylon_try!(ffi::IStreamGrabber_cancel_grab(self.inner));
        Ok(())
    }
    pub fn finish_grab(&mut self) -> Result<()> {
        pylon_try!(ffi::IStreamGrabber_finish_grab(self.inner));
        Ok(())
    }
    pub fn register_buffer(&mut self, mut buf: Buffer) -> Result<Handle> {
        // TODO FIXME unsafe: Should I std::mem::forget() about buf.data here?
        let mut result: ffi::StreamBufferHandle = std::ptr::null_mut();
        pylon_try!(ffi::IStreamGrabber_register_buffer(self.inner,
                                                             buf.data.as_mut_ptr(),
                                                             buf.data.len(),
                                                             &mut result));
        self.buffers.push(buf);
        Ok(Handle { inner: result })
    }

    pub fn pop_buffer(&mut self) -> Option<Buffer> {
        self.buffers.pop()
    }

    pub fn queue_buffer(&mut self, handle: Handle) -> Result<()> {
        let mut err_msg: Vec<u8> = vec![0; ERRBUFLEN];
        let max_len = ERRBUFLEN;

        trace!("pylon_try calling IStreamGrabber_queue_buffer in {}:{}", file!(), line!());
        let c_result = unsafe {
            ffi::IStreamGrabber_queue_buffer(self.inner, handle.inner, err_msg.as_mut_ptr() as *mut c_char, max_len as i32)
        };
        trace!("pylon_try c_result {:?} IStreamGrabber_queue_buffer in {}:{}", c_result, file!(), line!());

        match c_result {
            ffi::PylonCppError_t::PYLONCPPWRAP_NO_ERROR => {
                trace!("  pylon_try OK");
            },
            ffi::PylonCppError_t::PYLONCPPWRAP_ERROR_ENUM_NOT_MATCHED => {
                return Err(Error::EnumNotMatched)
            },
            ffi::PylonCppError_t::PYLONCPPWRAP_ERROR_CALLBACK_FAIL => {
                return Err(Error::CallbackFail)
            },
            ffi::PylonCppError_t::PYLONCPPWRAP_ERROR_NAME_NOT_FOUND => {
                return Err(Error::NameNotFound)
            },
            ffi::PylonCppError_t::PYLONCPPWRAP_ERROR_NULL_POINTER => {
                return Err(Error::NullPointer)
            },
            ffi::PylonCppError_t::PYLONCPPWRAP_ERROR_PYLON_EXCEPTION => {
                let cs = str_from_u8_nul_utf8(&err_msg).map_err(|e| Error::Utf8Error(e))?.to_string();
                return Err(Error::PylonExceptionDescr(cs))
            },
            ffi::PylonCppError_t::PYLONCPPWRAP_ERROR_INVALID_RESULT => {
                return Err(Error::InvalidResult)
            },
        }

        Ok(())
    }

    pub fn get_wait_object(&mut self) -> Result<WaitObject> {
        let mut result: *mut ffi::WaitObject = std::ptr::null_mut();
        pylon_try!(ffi::IStreamGrabber_get_wait_object(self.inner, &mut result));
        Ok(WaitObject { inner: result })
    }

    pub fn retrieve_result(&mut self) -> Result<Option<GrabResult>> {
        let mut result: *mut ffi::GrabResult = std::ptr::null_mut();
        let mut is_ready = false;
        pylon_try!(ffi::IStreamGrabber_retrieve_result(self.inner,
                                                             &mut result,
                                                             &mut is_ready));
        match is_ready {
            true => Ok(Some(GrabResult { inner: result })),
            false => Ok(None),
        }
    }
}

impl HasNodeMap for StreamGrabber {
    fn node_map(&self) -> Result<NodeMap> {
        let mut result: *mut ffi::INodeMap = std::ptr::null_mut();
        pylon_try!(ffi::IStreamGrabber_node_map(self.inner, &mut result));
        Ok(NodeMap { inner: result })
    }
}

// ---------------------------
// WaitObject

pub struct WaitObject {
    inner: *mut ffi::WaitObject,
}

unsafe impl Send for WaitObject {}

impl WaitObject {
    pub fn wait(&self, timeout_msec: u64) -> Result<bool> {
        let val = timeout_msec as c_uint;
        let mut result = false;
        pylon_try!(ffi::WaitObject_wait(self.inner, val, &mut result));
        Ok(result)
    }
}

// ---------------------------
// PayloadType

pub enum PayloadType {
    Undefined,
    Image,
    RawData,
    File,
    ChunkData,
    DeviceSpecific,
}

// ---------------------------
// GrabResult

pub struct GrabResult {
    inner: *mut ffi::GrabResult,
}

impl GrabResult {
    pub fn status(&self) -> GrabStatus {
        let mut result = ffi::EGRAB_STATUS_IDLE;
        pylon_try_panic!(ffi::GrabResult_status(self.inner, &mut result),
                         "GrabResult_status");
        match result {
            ffi::EGRAB_STATUS_UNDEFINED_GRAB_STATUS => GrabStatus::_UndefinedGrabStatus,
            ffi::EGRAB_STATUS_IDLE => GrabStatus::Idle,
            ffi::EGRAB_STATUS_QUEUED => GrabStatus::Queued,
            ffi::EGRAB_STATUS_GRABBED => GrabStatus::Grabbed,
            ffi::EGRAB_STATUS_CANCELED => GrabStatus::Canceled,
            ffi::EGRAB_STATUS_FAILED => GrabStatus::Failed,
            _ => unreachable!(),
        }
    }
    pub fn payload_type(&self) -> PayloadType {
        let mut result = ffi::PayloadType_Undefined;
        pylon_try_panic!(ffi::GrabResult_get_payload_type(self.inner, &mut result),
                         "GrabResult_status");
        match result {
            ffi::PayloadType_Undefined => PayloadType::Undefined,
            ffi::PayloadType_Image => PayloadType::Image,
            ffi::PayloadType_RawData => PayloadType::RawData,
            ffi::PayloadType_File => PayloadType::File,
            ffi::PayloadType_ChunkData => PayloadType::ChunkData,
            ffi::PayloadType_DeviceSpecific => PayloadType::DeviceSpecific,
            _ => unreachable!(),
        }
    }
    pub fn data_view(&self) -> &[u8] {
        let mut buffer: *const u8 = std::ptr::null_mut();
        let mut size: i64 = 0;
        pylon_try_panic!(ffi::GrabResult_get_buffer(self.inner, &mut buffer, &mut size),
                         "GrabResult_get_buffer");
        unsafe { std::slice::from_raw_parts(buffer as *const u8, size as usize) }
    }
    pub fn error_code(&self) -> u32 {
        let mut result = 0;
        pylon_try_panic!(ffi::GrabResult_error_code(self.inner, &mut result),
                         "GrabResult_error_code");
        result
    }
    pub fn error_description(&self) -> String {
        let swrap = CppStringWrap::new();
        pylon_try_panic!(ffi::GrabResult_error_description(self.inner, swrap.inner),
                         "GrabResult_error_description");
        let val_cstr = unsafe { CStr::from_ptr(ffi::CppStdString_bytes(swrap.inner)) }; // view data as CStr
        let bytes = val_cstr.to_bytes(); // view data as &[u8]
        let byte_vec: Vec<u8> = bytes.to_vec(); // copy data
        String::from_utf8(byte_vec).expect("non utf-8 error")
    }
    pub fn payload_size(&self) -> usize {
        let mut result = 0;
        pylon_try_panic!(ffi::GrabResult_payload_size(self.inner, &mut result),
                         "GrabResult_payload_size");
        result
    }
    pub fn size_x(&self) -> i32 {
        let mut result = 0;
        pylon_try_panic!(ffi::GrabResult_size_x(self.inner, &mut result),
                         "GrabResult_size_x");
        result
    }
    pub fn size_y(&self) -> i32 {
        let mut result = 0;
        pylon_try_panic!(ffi::GrabResult_size_y(self.inner, &mut result),
                         "GrabResult_size_y");
        result
    }
    pub fn handle(self) -> Handle {
        // consume the GrabResult and return a BufferHandle
        // let bh_ctx = self.ctx.hBuffer;
        // BufferHandle{ctx: bh_ctx}
        let mut result: ffi::StreamBufferHandle = std::ptr::null_mut();
        pylon_try_panic!(ffi::GrabResult_handle(self.inner, &mut result),
                         "GrabResult_handle");
        Handle { inner: result }
    }
    pub fn time_stamp(&self) -> u64 {
        let mut result = 0;
        pylon_try_panic!(ffi::GrabResult_time_stamp(self.inner, &mut result),
                         "GrabResult_time_stamp");
        result
    }

    pub fn block_id(&self) -> Result<u64> {
        let mut result = 0;
        pylon_try!(ffi::GrabResult_block_id(self.inner, &mut result));
        Ok(result)
    }

    pub fn image(&self) -> Result<ImageRef> {
        let mut result: *mut ffi::RefHolder = std::ptr::null_mut();
        pylon_try!(ffi::GrabResult_image(self.inner, &mut result));
        Ok(ImageRef{ inner: result, _parent: &self})
    }
}

impl Drop for GrabResult {
    fn drop(&mut self) {
        pylon_try_panic!(ffi::GrabResult_delete(self.inner), "GrabResult_delete");
        self.inner = std::ptr::null_mut();
    }
}

pub struct ImageRef<'a> {
    inner: *mut ffi::RefHolder,
    _parent: &'a GrabResult, // TODO: make this PhantomData?
}

impl<'a> Drop for ImageRef<'a> {
    fn drop(&mut self) {
        unsafe { ffi::RefHolder_delete(self.inner); }
        self.inner = std::ptr::null_mut();
    }
}

#[allow(non_camel_case_types)]
#[derive(Debug)]
pub enum PixelType {
    Mono1packed,
    Mono2packed,
    Mono4packed,
    Mono8,
    Mono8signed,
    Mono10,
    Mono10packed,
    Mono10p,
    Mono12,
    Mono12packed,
    Mono12p,
    Mono16,
    BayerGR8,
    BayerRG8,
    BayerGB8,
    BayerBG8,
    BayerGR10,
    BayerRG10,
    BayerGB10,
    BayerBG10,
    BayerGR12,
    BayerRG12,
    BayerGB12,
    BayerBG12,
    RGB8packed,
    BGR8packed,
    RGBA8packed,
    BGRA8packed,
    RGB10packed,
    BGR10packed,
    RGB12packed,
    BGR12packed,
    RGB16packed,
    BGR10V1packed,
    BGR10V2packed,
    YUV411packed,
    YUV422packed,
    YUV444packed,
    RGB8planar,
    RGB10planar,
    RGB12planar,
    RGB16planar,
    YUV422_YUYV_Packed,
    BayerGR12Packed,
    BayerRG12Packed,
    BayerGB12Packed,
    BayerBG12Packed,
    BayerGR10p,
    BayerRG10p,
    BayerGB10p,
    BayerBG10p,
    BayerGR12p,
    BayerRG12p,
    BayerGB12p,
    BayerBG12p,
    BayerGR16,
    BayerRG16,
    BayerGB16,
    BayerBG16,
    RGB12V1packed,
    Double,
    Undefined,
}

impl From<ffi::PixelType> for PixelType {
    fn from(orig: ffi::PixelType) -> PixelType {
        use PixelType::*;
        match orig {
            ffi::PIXELTYPE_MONO1PACKED => Mono1packed,
            ffi::PIXELTYPE_MONO2PACKED => Mono2packed,
            ffi::PIXELTYPE_MONO4PACKED => Mono4packed,
            ffi::PIXELTYPE_MONO8 => Mono8,
            ffi::PIXELTYPE_MONO8SIGNED => Mono8signed,
            ffi::PIXELTYPE_MONO10 => Mono10,
            ffi::PIXELTYPE_MONO10PACKED => Mono10packed,
            ffi::PIXELTYPE_MONO10P => Mono10p,
            ffi::PIXELTYPE_MONO12 => Mono12,
            ffi::PIXELTYPE_MONO12PACKED => Mono12packed,
            ffi::PIXELTYPE_MONO12P => Mono12p,
            ffi::PIXELTYPE_MONO16 => Mono16,
            ffi::PIXELTYPE_BAYERGR8 => BayerGR8,
            ffi::PIXELTYPE_BAYERRG8 => BayerRG8,
            ffi::PIXELTYPE_BAYERGB8 => BayerGB8,
            ffi::PIXELTYPE_BAYERBG8 => BayerBG8,
            ffi::PIXELTYPE_BAYERGR10 => BayerGR10,
            ffi::PIXELTYPE_BAYERRG10 => BayerRG10,
            ffi::PIXELTYPE_BAYERGB10 => BayerGB10,
            ffi::PIXELTYPE_BAYERBG10 => BayerBG10,
            ffi::PIXELTYPE_BAYERGR12 => BayerGR12,
            ffi::PIXELTYPE_BAYERRG12 => BayerRG12,
            ffi::PIXELTYPE_BAYERGB12 => BayerGB12,
            ffi::PIXELTYPE_BAYERBG12 => BayerBG12,
            ffi::PIXELTYPE_RGB8PACKED => RGB8packed,
            ffi::PIXELTYPE_BGR8PACKED => BGR8packed,
            ffi::PIXELTYPE_RGBA8PACKED => RGBA8packed,
            ffi::PIXELTYPE_BGRA8PACKED => BGRA8packed,
            ffi::PIXELTYPE_RGB10PACKED => RGB10packed,
            ffi::PIXELTYPE_BGR10PACKED => BGR10packed,
            ffi::PIXELTYPE_RGB12PACKED => RGB12packed,
            ffi::PIXELTYPE_BGR12PACKED => BGR12packed,
            ffi::PIXELTYPE_RGB16PACKED => RGB16packed,
            ffi::PIXELTYPE_BGR10V1PACKED => BGR10V1packed,
            ffi::PIXELTYPE_BGR10V2PACKED => BGR10V2packed,
            ffi::PIXELTYPE_YUV411PACKED => YUV411packed,
            ffi::PIXELTYPE_YUV422PACKED => YUV422packed,
            ffi::PIXELTYPE_YUV444PACKED => YUV444packed,
            ffi::PIXELTYPE_RGB8PLANAR => RGB8planar,
            ffi::PIXELTYPE_RGB10PLANAR => RGB10planar,
            ffi::PIXELTYPE_RGB12PLANAR => RGB12planar,
            ffi::PIXELTYPE_RGB16PLANAR => RGB16planar,
            ffi::PIXELTYPE_YUV422_YUYV_PACKED => YUV422_YUYV_Packed,
            ffi::PIXELTYPE_BAYERGR12PACKED => BayerGR12Packed,
            ffi::PIXELTYPE_BAYERRG12PACKED => BayerRG12Packed,
            ffi::PIXELTYPE_BAYERGB12PACKED => BayerGB12Packed,
            ffi::PIXELTYPE_BAYERBG12PACKED => BayerBG12Packed,
            ffi::PIXELTYPE_BAYERGR10P => BayerGR10p,
            ffi::PIXELTYPE_BAYERRG10P => BayerRG10p,
            ffi::PIXELTYPE_BAYERGB10P => BayerGB10p,
            ffi::PIXELTYPE_BAYERBG10P => BayerBG10p,
            ffi::PIXELTYPE_BAYERGR12P => BayerGR12p,
            ffi::PIXELTYPE_BAYERRG12P => BayerRG12p,
            ffi::PIXELTYPE_BAYERGB12P => BayerGB12p,
            ffi::PIXELTYPE_BAYERBG12P => BayerBG12p,
            ffi::PIXELTYPE_BAYERGR16 => BayerGR16,
            ffi::PIXELTYPE_BAYERRG16 => BayerRG16,
            ffi::PIXELTYPE_BAYERGB16 => BayerGB16,
            ffi::PIXELTYPE_BAYERBG16 => BayerBG16,
            ffi::PIXELTYPE_RGB12V1PACKED => RGB12V1packed,
            ffi::PIXELTYPE_DOUBLE => Double,
            _ => Undefined, // e.g. PIXELTYPE_UNDEFINED
        }
    }
}

impl<'a> ImageRef<'a> {
    pub fn valid(&self) -> Result<bool> {
        let mut result = false;
        pylon_try!(ffi::CGrabResultImageRef_is_valid(self.inner, &mut result));
        Ok(result)
    }
    pub fn pixel_type(&self) -> Result<PixelType> {
        let mut result: ffi::PixelType = -1;
        pylon_try!(ffi::CGrabResultImageRef_get_pixel_type(self.inner, &mut result));
        Ok(result.into())
    }
    pub fn width(&self) -> Result<u32> {
        let mut result = 0;
        pylon_try!(ffi::CGrabResultImageRef_get_width(self.inner, &mut result));
        Ok(result)
    }
    pub fn height(&self) -> Result<u32> {
        let mut result = 0;
        pylon_try!(ffi::CGrabResultImageRef_get_height(self.inner, &mut result));
        Ok(result)
    }
    pub fn data_view(&self) -> &[u8] {
        let mut buffer: *const c_void = std::ptr::null_mut();
        let mut size: usize = 0;
        pylon_try_panic!(ffi::CGrabResultImageRef_get_buffer(self.inner, &mut buffer),
            "CGrabResultImageRef_get_buffer");
        pylon_try_panic!(ffi::CGrabResultImageRef_get_image_size(self.inner, &mut size),
            "CGrabResultImageRef_get_image_size");
        unsafe { std::slice::from_raw_parts(buffer as *const u8, size) }
    }
    pub fn image_size(&self) -> Result<usize> {
        let mut result: usize = 0;
        pylon_try!(ffi::CGrabResultImageRef_get_image_size(self.inner, &mut result));
        Ok(result)
    }
    pub fn stride(&self) -> Result<usize> {
        let mut result = 0;
        pylon_try!(ffi::CGrabResultImageRef_get_stride(self.inner, &mut result));
        Ok(result)
    }
}

// ---------------------------
// GrabStatus

#[derive(Debug)]
pub enum GrabStatus {
    _UndefinedGrabStatus,
    Idle,
    Queued,
    Grabbed,
    Canceled,
    Failed,
}

// ---------------------------
// Handle

pub struct Handle {
    inner: ffi::StreamBufferHandle,
}

// ---------------------------
// NodeMap

pub struct NodeMap {
    inner: *mut ffi::INodeMap,
}

impl NodeMap {
    pub fn node(&self, name: &str) -> Result<Node> {
        let name_cstr = std::ffi::CString::new(name).unwrap();
        let mut result: *mut ffi::INode = std::ptr::null_mut();
        pylon_try!(ffi::INodeMap_node(self.inner, name_cstr.as_ptr(), &mut result));
        // println!("NodeMap::node return address of {:?}", result);
        Ok(Node::from_raw(result))
    }

    pub fn nodes(&self) -> Result<Vec<Node>> {
        let target = Box::new(NodeList { a: Vec::new() });
        let raw_target = Box::into_raw(target);
        pylon_try!(ffi::INodeMap_get_nodes(self.inner,
                                                 convert_and_append_node,
                                                 raw_target as *mut c_void));
        let target = unsafe { Box::from_raw(raw_target) };
        Ok((*target).a)
    }
}

impl HasNodeMap for NodeMap {
    fn node_map(&self) -> Result<NodeMap> {
        Ok(NodeMap { inner: self.inner })
    }
}

extern "C" fn convert_and_append_node(target_c: *mut c_void, val: *mut ffi::INode) -> u8 {
    let result = std::panic::catch_unwind(|| {
        let mut target = unsafe { Box::from_raw(target_c as *mut NodeList) };
        let node = Node::from_raw(val);
        (*target).a.push(node);
        std::mem::forget(target); // don't drop target
    });
    match result {
        Ok(_) => 0, // OK
        Err(_) => 1,
    }
}

// ---------------------------
// Visibility

#[derive(Debug,PartialEq)]
pub enum Visibility {
    Beginner,
    Expert,
    Guru,
    Invisible,
    Undefined,
}

// ---------------------------
// Node

pub struct Node {
    inner: *mut ffi::INode,
}

impl Node {
    fn from_raw(inner: *mut ffi::INode) -> Node {
        Node { inner: inner }
    }
    pub fn name(&self, fully_qualified: bool) -> String {
        const MAXLEN: usize = 300;
        let mut value: [c_char; MAXLEN] = [0; MAXLEN];
        pylon_try_panic!(ffi::INode_get_name(self.inner,
                                             fully_qualified,
                                             value.as_mut_ptr(),
                                             MAXLEN),
                         "INode_get_name");
        let val_cstr = unsafe { CStr::from_ptr(value.as_ptr()) };
        let v = val_cstr.to_str().expect("failed to convert to str");
        v.to_string()
    }
    pub fn visibility(&self) -> Visibility {
        let mut vis = -1;
        pylon_try_panic!(ffi::INode_get_visibility(self.inner, &mut vis),
                         "INode_get_visibility");
        match vis {
            ffi::EVISIBILITY_BEGINNER => Visibility::Beginner,
            ffi::EVISIBILITY_EXPERT => Visibility::Expert,
            ffi::EVISIBILITY_GURU => Visibility::Guru,
            ffi::EVISIBILITY_INVISIBLE => Visibility::Invisible,
            ffi::EVISIBILITY_UNDEFINED => Visibility::Undefined,
            _ => panic!("unexpected visibility result"),
        }
    }
    pub fn principal_interface_type(&self) -> Result<InterfaceType> {
        let mut c_result = 0;
        pylon_try!(ffi::INode_principal_interface_type(self.inner, &mut c_result));
        let result = match c_result {
            0 => InterfaceType::IValue,
            1 => InterfaceType::IBase,
            2 => InterfaceType::IInteger,
            3 => InterfaceType::IBoolean,
            4 => InterfaceType::ICommand,
            5 => InterfaceType::IFloat,
            6 => InterfaceType::IString,
            7 => InterfaceType::IRegister,
            8 => InterfaceType::ICategory,
            9 => InterfaceType::IEnumeration,
            10 => InterfaceType::IEnumEntry,
            11 => InterfaceType::IPort,
            _ => unimplemented!(),
        };
        Ok(result)
    }
    pub fn to_integer_node(mut self) -> Result<IntegerNode> {
        match self.principal_interface_type()? {
            InterfaceType::IInteger => {
                let mut result: *mut ffi::IInteger = std::ptr::null_mut();
                pylon_try!(ffi::INode_to_integer_node(&mut self.inner, &mut result));
                Ok(IntegerNode { inner: result })
            }
            t => Err(Error::WrongNodeType(format!("cannot cast type {:?} to integer node", t)))
        }
    }
    pub fn to_boolean_node(mut self) -> Result<BooleanNode> {
        match self.principal_interface_type()? {
            InterfaceType::IBoolean => {
                let mut result: *mut ffi::IBoolean = std::ptr::null_mut();
                pylon_try!(ffi::INode_to_boolean_node(&mut self.inner, &mut result));
                Ok(BooleanNode { inner: result })
            }
            t => Err(Error::WrongNodeType(format!("cannot cast type {:?} to boolean node", t)))
        }
    }
    pub fn to_float_node(mut self) -> Result<FloatNode> {
        match self.principal_interface_type()? {
            InterfaceType::IFloat => {
                let mut result: *mut ffi::IFloat = std::ptr::null_mut();
                pylon_try!(ffi::INode_to_float_node(&mut self.inner, &mut result));
                Ok(FloatNode { inner: result })
            }
            t => Err(Error::WrongNodeType(format!("cannot cast type {:?} to float node", t)))
        }
    }
    pub fn to_string_node(mut self) -> Result<StringNode> {
        match self.principal_interface_type()? {
            InterfaceType::IString => {
                let mut result: *mut ffi::IString = std::ptr::null_mut();
                pylon_try!(ffi::INode_to_string_node(&mut self.inner, &mut result));
                Ok(StringNode { inner: result })
            }
            t => Err(Error::WrongNodeType(format!("cannot cast type {:?} to string node", t)))
        }
    }
    pub fn to_enumeration_node(mut self) -> Result<EnumerationNode> {
        match self.principal_interface_type()? {
            InterfaceType::IEnumeration => {
                let mut result: *mut ffi::IEnumeration = std::ptr::null_mut();
                pylon_try!(ffi::INode_to_enumeration_node(&mut self.inner, &mut result));
                Ok(EnumerationNode { inner: result })
            }
            t => Err(Error::WrongNodeType(format!("cannot cast type {:?} to enumeration node", t)))
        }
    }
    pub fn to_command_node(mut self) -> Result<CommandNode> {
        match self.principal_interface_type()? {
            InterfaceType::ICommand => {
                let mut result: *mut ffi::ICommand = std::ptr::null_mut();
                pylon_try!(ffi::INode_to_command_node(&mut self.inner, &mut result));
                Ok(CommandNode { inner: result })
            }
            t => Err(Error::WrongNodeType(format!("cannot cast type {:?} to command node", t)))
        }
    }
}

impl std::fmt::Debug for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f,
               "INode {{ Name: \"{}\", Type: {:?} }}",
               self.name(true),
               self.principal_interface_type())
    }
}


// ---------------------------
// IntegerNode

pub struct IntegerNode {
    inner: *mut ffi::IInteger,
}

impl IntegerNode {
    pub fn value(&self) -> Result<i64> {
        let mut result = 0;
        pylon_try!(ffi::IInteger_get_value(self.inner, &mut result));
        Ok(result)
    }
    pub fn range(&self) -> Result<(i64, i64)> {
        let mut min = 0;
        let mut max = 0;
        pylon_try!(ffi::IInteger_get_range(self.inner, &mut min, &mut max));
        Ok((min, max))
    }
    pub fn set_value(&mut self, value: i64) -> Result<()> {
        pylon_try!(ffi::IInteger_set_value(self.inner, value));
        Ok(())
    }
}

// ---------------------------
// BooleanNode

pub struct BooleanNode {
    inner: *mut ffi::IBoolean,
}

impl BooleanNode {
    pub fn value(&self) -> Result<bool> {
        let mut result = false;
        pylon_try!(ffi::IBoolean_get_value(self.inner, &mut result));
        Ok(result)
    }
    pub fn set_value(&mut self, value: bool) -> Result<()> {
        pylon_try!(ffi::IBoolean_set_value(self.inner, value));
        Ok(())
    }
}

// ---------------------------
// FloatNode

pub struct FloatNode {
    inner: *mut ffi::IFloat,
}

impl FloatNode {
    pub fn value(&self) -> Result<f64> {
        let mut result = 0.0;
        pylon_try!(ffi::IFloat_get_value(self.inner, &mut result));
        Ok(result)
    }
    pub fn range(&self) -> Result<(f64, f64)> {
        let mut min = 0.0;
        let mut max = 0.0;
        pylon_try!(ffi::IFloat_get_range(self.inner, &mut min, &mut max));
        Ok((min, max))
    }
    pub fn set_value(&mut self, value: f64) -> Result<()> {
        pylon_try!(ffi::IFloat_set_value(self.inner, value));
        Ok(())
    }
}

// ---------------------------
// StringNode

pub struct StringNode {
    inner: *mut ffi::IString,
}

impl StringNode {
    pub fn value(&self) -> Result<String> {
        const MAXLEN: usize = 300;
        let mut value: [c_char; MAXLEN] = [0; MAXLEN];
        pylon_try!(ffi::IString_get_value(self.inner, value.as_mut_ptr(), MAXLEN));
        let val_cstr = unsafe { CStr::from_ptr(value.as_ptr()) };
        let v = val_cstr.to_str().map_err(|e| Error::Utf8Error(e))?;
        Ok(v.to_string())
    }
    pub fn set_value(&mut self, value: &str) -> Result<()> {
        let value_cstr = std::ffi::CString::new(value).unwrap();
        pylon_try!(ffi::IString_set_value(self.inner, value_cstr.as_ptr()));
        Ok(())
    }
}

// ---------------------------
// EnumerationNode

pub struct EnumerationNode {
    inner: *mut ffi::IEnumeration,
}

impl EnumerationNode {
    pub fn value(&self) -> Result<String> {
        const MAXLEN: usize = 300;
        let mut value: [c_char; MAXLEN] = [0; MAXLEN];
        pylon_try!(ffi::IEnumeration_get_value(self.inner, value.as_mut_ptr(), MAXLEN));
        let val_cstr = unsafe { CStr::from_ptr(value.as_ptr()) };
        let v = val_cstr.to_str().map_err(|e| Error::Utf8Error(e))?;
        Ok(v.to_string())
    }
    pub fn set_value(&mut self, value: &str) -> Result<()> {
        let value_cstr = std::ffi::CString::new(value).unwrap();
        pylon_try!(ffi::IEnumeration_set_value(self.inner, value_cstr.as_ptr()));
        Ok(())
    }
    pub fn get_entries(&self) -> Result<Vec<Node>> {
        let target = Box::new(NodeList { a: Vec::new() });
        let raw_target = Box::into_raw(target);
        // like INodeMap_get_nodes
        pylon_try!(ffi::IEnumeration_get_entries(self.inner,
                                                 convert_and_append_node,
                                                 raw_target as *mut c_void));
        let target = unsafe { Box::from_raw(raw_target) };
        Ok((*target).a)
    }
}

// ---------------------------
// CommandNode

pub struct CommandNode {
    inner: *mut ffi::ICommand,
}

impl CommandNode {
    pub fn execute(&self) -> Result<()> {
        pylon_try!(ffi::ICommand_execute(self.inner));
        Ok(())
    }
}

// ---------------------------
// InterfaceType

#[derive(Debug,PartialEq)]
pub enum InterfaceType {
    IValue,
    IBase,
    IInteger,
    IBoolean,
    ICommand,
    IFloat,
    IString,
    IRegister,
    ICategory,
    IEnumeration,
    IEnumEntry,
    IPort,
}

// ---------------------------
// Buffer

pub struct Buffer {
    data: Vec<u8>,
}

impl Buffer {
    pub fn new(data: Vec<u8>) -> Buffer {
        Buffer { data: data }
    }
}
