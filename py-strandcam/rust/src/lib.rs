use std::cell::RefCell;
use std::mem;
use std::ptr;
// use std::str;
use anyhow::Error;
use std::borrow::Cow;
use std::os::raw::c_char;

use plugin_defs::{DataHandle, ProcessFrameFunc};

thread_local! {
    pub static LAST_ERROR: RefCell<Option<Error>> = RefCell::new(None);
}

fn set_last_error(err: Error) {
    LAST_ERROR.with(|e| {
        *e.borrow_mut() = Some(err);
    });
}

/// Represents all possible error codes.
#[repr(u32)]
pub enum StrandCamErrorCode {
    NoError = 0,
    Panic = 1,
    Unknown = 2,
}

impl StrandCamErrorCode {
    /// This maps all errors that can possibly happen.
    pub fn from_error(error: &Error) -> StrandCamErrorCode {
        for cause in error.chain() {
            if cause.downcast_ref::<Panic>().is_some() {
                return StrandCamErrorCode::Panic;
            }
        }
        StrandCamErrorCode::Unknown
    }
}

/// Returns the last error code.
///
/// If there is no error, 0 is returned.
#[no_mangle]
pub unsafe extern "C" fn strandcam_err_get_last_code() -> StrandCamErrorCode {
    LAST_ERROR.with(|e| {
        if let Some(ref err) = *e.borrow() {
            StrandCamErrorCode::from_error(err)
        } else {
            StrandCamErrorCode::NoError
        }
    })
}

/// Returns the last error message.
///
/// If there is no error an empty string is returned.  This allocates new memory
/// that needs to be freed with `strandcam_str_free`.
#[no_mangle]
pub unsafe extern "C" fn strandcam_err_get_last_message() -> StrandCamStr {
    use std::fmt::Write;
    LAST_ERROR.with(|e| {
        if let Some(ref err) = *e.borrow() {
            let mut msg = err.to_string();
            for cause in err.chain() {
                write!(&mut msg, "\n  caused by: {}", cause).ok();
            }
            StrandCamStr::from_string(msg)
        } else {
            Default::default()
        }
    })
}

/// Frees a strandcam str.
///
/// If the string is marked as not owned then this function does not
/// do anything.
#[no_mangle]
pub unsafe extern "C" fn strandcam_str_free(string: *mut StrandCamStr) {
    (*string).free()
}

/// CABI wrapper around string.
#[repr(C)]
pub struct StrandCamStr {
    pub data: *mut c_char,
    pub len: usize,
    pub owned: bool,
}

impl Default for StrandCamStr {
    fn default() -> StrandCamStr {
        StrandCamStr {
            data: ptr::null_mut(),
            len: 0,
            owned: false,
        }
    }
}

impl StrandCamStr {
    /// Creates a new `StrandCamStr` from a Rust string.
    pub fn new(s: &str) -> StrandCamStr {
        StrandCamStr {
            data: s.as_ptr() as *mut c_char,
            len: s.len(),
            owned: false,
        }
    }

    /// Creates a new `StrandCamStr` from an owned Rust string.
    pub fn from_string(mut s: String) -> StrandCamStr {
        s.shrink_to_fit();
        let rv = StrandCamStr {
            data: s.as_ptr() as *mut c_char,
            len: s.len(),
            owned: true,
        };
        mem::forget(s);
        rv
    }

    /// Releases memory held by an unmanaged `StrandCamStr`.
    pub unsafe fn free(&mut self) {
        if self.owned {
            String::from_raw_parts(self.data as *mut _, self.len, self.len);
            self.data = ptr::null_mut();
            self.len = 0;
            self.owned = false;
        }
    }

    /// Returns the Rust string managed by a `StrandCamStr`.
    pub fn as_str(&self) -> &str {
        unsafe {
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(
                self.data as *const _,
                self.len,
            ))
        }
    }
}

impl Drop for StrandCamStr {
    fn drop(&mut self) {
        unsafe { self.free() }
    }
}

impl From<String> for StrandCamStr {
    fn from(string: String) -> StrandCamStr {
        StrandCamStr::from_string(string)
    }
}

impl<'a> From<&'a str> for StrandCamStr {
    fn from(string: &str) -> StrandCamStr {
        StrandCamStr::new(string)
    }
}

impl<'a> From<Cow<'a, str>> for StrandCamStr {
    fn from(cow: Cow<'a, str>) -> StrandCamStr {
        match cow {
            Cow::Borrowed(string) => StrandCamStr::new(string),
            Cow::Owned(string) => StrandCamStr::from_string(string),
        }
    }
}

/// Clears the last error.
#[no_mangle]
pub unsafe extern "C" fn strandcam_err_clear() {
    LAST_ERROR.with(|e| {
        *e.borrow_mut() = None;
    });
}

/// An error  in place of panics.
#[derive(thiserror::Error, Debug)]
#[error("strandcam panicked: {0}")]
pub struct Panic(String);

const APP_NAME: &str = "py-strandcam-pylon";

lazy_static::lazy_static! {
    static ref PYLON_MODULE: ci2_pyloncxx::WrappedModule = ci2_pyloncxx::new_module().unwrap();
}

/// Register a global process frame callback and run the app.
#[no_mangle]
pub unsafe extern "C" fn sc_run_app_with_process_frame_cb(
    process_frame_callback: ProcessFrameFunc,
    data_handle: DataHandle,
) {
    match std::panic::catch_unwind(|| {
        let cb_data = strand_cam::ProcessFrameCbData {
            func_ptr: process_frame_callback,
            data_handle,
        };
        let mut args = strand_cam::StrandCamArgs::default();
        args.process_frame_callback = Some(cb_data);

        let mymod = ci2_async::into_threaded_async(&*PYLON_MODULE);
        match strand_cam::run_app(mymod, args, APP_NAME) {
            Ok(()) => {}
            Err(e) => {
                set_last_error(e.into());
                return;
            }
        }
    }) {
        Ok(_) => {}
        Err(_) => {
            set_last_error(Panic("".to_string()).into());
            return;
        }
    }
}
