#[cfg(feature = "backtrace")]
use std::backtrace::Backtrace;

use crate::NvInt;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum NvencError {
    #[error("dynamic library `{dynlib}` could not be loaded: `{source}`")]
    DynLibLoadError {
        dynlib: String,
        source: libloading::Error,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("NvEnc returned code `{status}`: {message}")]
    ErrCode {
        status: NvInt,
        message: &'static str,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("Name `{name}` could not be opened")]
    NameFFIError {
        name: String,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("Name `{name}` could not be opened: `{source}`")]
    NameFFIError2 {
        name: String,
        source: libloading::Error,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("Unable to compute image size")]
    UnableToComputeSize {
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("Encode configuration required")]
    EncodeConfigRequired {
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
}

pub fn code_to_string(code: crate::ffi::_NVENCSTATUS::Type) -> &'static str {
    use crate::ffi::_NVENCSTATUS::*;
    match code {
        NV_ENC_SUCCESS => "NV_ENC_SUCCESS",
        NV_ENC_ERR_NO_ENCODE_DEVICE => "NV_ENC_ERR_NO_ENCODE_DEVICE",
        NV_ENC_ERR_UNSUPPORTED_DEVICE => "NV_ENC_ERR_UNSUPPORTED_DEVICE",
        NV_ENC_ERR_INVALID_ENCODERDEVICE => "NV_ENC_ERR_INVALID_ENCODERDEVICE",
        NV_ENC_ERR_INVALID_DEVICE => "NV_ENC_ERR_INVALID_DEVICE",
        NV_ENC_ERR_DEVICE_NOT_EXIST => "NV_ENC_ERR_DEVICE_NOT_EXIST",
        NV_ENC_ERR_INVALID_PTR => "NV_ENC_ERR_INVALID_PTR",
        NV_ENC_ERR_INVALID_EVENT => "NV_ENC_ERR_INVALID_EVENT",
        NV_ENC_ERR_INVALID_PARAM => "NV_ENC_ERR_INVALID_PARAM",
        NV_ENC_ERR_INVALID_CALL => "NV_ENC_ERR_INVALID_CALL",
        NV_ENC_ERR_OUT_OF_MEMORY => "NV_ENC_ERR_OUT_OF_MEMORY",
        NV_ENC_ERR_ENCODER_NOT_INITIALIZED => "NV_ENC_ERR_ENCODER_NOT_INITIALIZED",
        NV_ENC_ERR_UNSUPPORTED_PARAM => "NV_ENC_ERR_UNSUPPORTED_PARAM",
        NV_ENC_ERR_LOCK_BUSY => "NV_ENC_ERR_LOCK_BUSY",
        NV_ENC_ERR_NOT_ENOUGH_BUFFER => "NV_ENC_ERR_NOT_ENOUGH_BUFFER",
        NV_ENC_ERR_INVALID_VERSION => "NV_ENC_ERR_INVALID_VERSION",
        NV_ENC_ERR_MAP_FAILED => "NV_ENC_ERR_MAP_FAILED",
        NV_ENC_ERR_NEED_MORE_INPUT => "NV_ENC_ERR_NEED_MORE_INPUT",
        NV_ENC_ERR_ENCODER_BUSY => "NV_ENC_ERR_ENCODER_BUSY",
        NV_ENC_ERR_EVENT_NOT_REGISTERD => "NV_ENC_ERR_EVENT_NOT_REGISTERD",
        NV_ENC_ERR_GENERIC => "NV_ENC_ERR_GENERIC",
        NV_ENC_ERR_INCOMPATIBLE_CLIENT_KEY => "NV_ENC_ERR_INCOMPATIBLE_CLIENT_KEY",
        NV_ENC_ERR_UNIMPLEMENTED => "NV_ENC_ERR_UNIMPLEMENTED",
        NV_ENC_ERR_RESOURCE_REGISTER_FAILED => "NV_ENC_ERR_RESOURCE_REGISTER_FAILED",
        NV_ENC_ERR_RESOURCE_NOT_REGISTERED => "NV_ENC_ERR_RESOURCE_NOT_REGISTERED",
        NV_ENC_ERR_RESOURCE_NOT_MAPPED => "NV_ENC_ERR_RESOURCE_NOT_MAPPED",
        _ => "unknown error",
    }
}
