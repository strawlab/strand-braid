#[cfg(feature = "backtrace")]
use std::backtrace::Backtrace;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum CudaError {
    #[error("dynamic library `{lib}` could not be loaded: `{source}`")]
    DynLibLoadError {
        lib: String,
        source: libloading::Error,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("CUDA returned code `{status}`")]
    ErrCode {
        status: i32,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("Name `{name}` could not be opened: `{source}`")]
    NameFFIError {
        name: String,
        source: libloading::Error,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
}
