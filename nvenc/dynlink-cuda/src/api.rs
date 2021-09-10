#[cfg(feature = "backtrace")]
use std::backtrace::Backtrace;

use std::mem::zeroed;

use crate::error::CudaError;
use crate::ffi::*;
use crate::load::SharedLibrary;

macro_rules! api_call {
    ($expr:expr) => {{
        let status = $expr;
        if status != cudaError_enum::CUDA_SUCCESS {
            return Err(CudaError::ErrCode {
                status,
                #[cfg(feature = "backtrace")]
                backtrace: Backtrace::capture(),
            });
        }
    }};
}

#[allow(non_snake_case, dead_code)]
pub struct LibCuda<'lib> {
    cuInit: libloading::Symbol<'lib, extern "C" fn(::std::os::raw::c_uint) -> CUresult>,
    cuDriverGetVersion:
        libloading::Symbol<'lib, extern "C" fn(*mut ::std::os::raw::c_int) -> CUresult>,
    cuDeviceGetCount:
        libloading::Symbol<'lib, extern "C" fn(*mut ::std::os::raw::c_int) -> CUresult>,
    cuDeviceGet:
        libloading::Symbol<'lib, extern "C" fn(*mut CUdevice, ::std::os::raw::c_int) -> CUresult>,
    cuDeviceGetName: libloading::Symbol<
        'lib,
        extern "C" fn(
            name: *mut ::std::os::raw::c_char,
            ::std::os::raw::c_int,
            CUdevice,
        ) -> CUresult,
    >,
    cuCtxCreate_v2: libloading::Symbol<
        'lib,
        extern "C" fn(*mut CUcontext, ::std::os::raw::c_uint, CUdevice) -> CUresult,
    >,
}

impl<'lib> LibCuda<'lib> {
    pub fn init(&self, flags: u32) -> Result<(), CudaError> {
        api_call!((*self.cuInit)(flags));
        Ok(())
    }
    pub fn driver_get_version(&self) -> Result<i32, CudaError> {
        let mut value = 0;
        api_call!((*self.cuDriverGetVersion)(&mut value));
        Ok(value)
    }
    pub fn device_get_count(&self) -> Result<i32, CudaError> {
        let mut value = 0;
        api_call!((*self.cuDeviceGetCount)(&mut value));
        Ok(value)
    }
    pub fn new_device(&self, i: i32) -> Result<CudaDevice, CudaError> {
        let mut inner: CUdevice = unsafe { zeroed() };
        api_call!((*self.cuDeviceGet)(&mut inner, i));
        Ok(CudaDevice {
            parent: self,
            inner,
        })
    }
}

pub struct CudaDevice<'a> {
    parent: &'a LibCuda<'a>,
    inner: CUdevice,
}

pub struct CudaContext<'a> {
    _parent: &'a LibCuda<'a>,
    inner: CUcontext,
}

impl<'a> CudaContext<'a> {
    pub fn as_mut_void_ptr(&mut self) -> *mut std::ffi::c_void {
        self.inner as *mut std::ffi::c_void
    }
}

impl<'a> CudaDevice<'a> {
    pub fn name(&self) -> Result<String, CudaError> {
        use std::convert::TryInto;
        const MAX_LEN: i32 = 255;
        let value = std::ffi::CString::new(vec![b' '; MAX_LEN.try_into().unwrap()]).unwrap();
        let raw = value.into_raw();
        api_call!((*self.parent.cuDeviceGetName)(raw, MAX_LEN, self.inner));
        // Note: on error we will leak the memory allocated in CString::new().
        let cs = unsafe { std::ffi::CString::from_raw(raw) };
        let r = cs.into_string().unwrap();
        Ok(r)
    }
    pub fn into_context(self) -> Result<CudaContext<'a>, CudaError> {
        let mut context: CUcontext = unsafe { zeroed() };
        api_call!((*self.parent.cuCtxCreate_v2)(&mut context, 0, self.inner));
        Ok(CudaContext {
            _parent: self.parent,
            inner: context,
        })
    }
}

macro_rules! get_func {
    ($lib:expr, $name:expr) => {{
        unsafe { $lib.library.get($name) }.map_err(|source| CudaError::NameFFIError {
            name: String::from_utf8_lossy($name).to_string(),
            source,
            #[cfg(feature = "backtrace")]
            backtrace: Backtrace::capture(),
        })?
    }};
}

pub fn init(library: &SharedLibrary) -> Result<LibCuda<'_>, CudaError> {
    let lib_cuda = LibCuda {
        cuInit: get_func!(library, b"cuInit\0"),
        cuDriverGetVersion: get_func!(library, b"cuDriverGetVersion\0"),
        cuDeviceGetCount: get_func!(library, b"cuDeviceGetCount\0"),
        cuDeviceGet: get_func!(library, b"cuDeviceGet\0"),
        cuDeviceGetName: get_func!(library, b"cuDeviceGetName\0"),
        cuCtxCreate_v2: get_func!(library, b"cuCtxCreate_v2\0"),
    };

    Ok(lib_cuda)
}
