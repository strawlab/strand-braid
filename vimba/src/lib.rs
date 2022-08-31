#![cfg_attr(
    feature = "backtrace",
    feature(error_generic_member_access, provide_any)
)]

#[cfg(feature = "backtrace")]
use std::backtrace::Backtrace;

use std::{convert::TryInto, pin::Pin};

use machine_vision_formats as formats;

use vimba_sys::{
    VmbCameraInfo_t, VmbErrorType, VmbFeaturePersistSettings_t, VmbFrameCallback,
    VmbFrameStatusType, VmbFrame_t, VmbHandle_t, VmbVersionInfo_t,
};

fn err_str(err: i32) -> &'static str {
    use VmbErrorType::*;
    #[allow(non_upper_case_globals)]
    match err {
        VmbErrorSuccess => "VmbErrorSuccess",
        VmbErrorInternalFault => "VmbErrorInternalFault",
        VmbErrorApiNotStarted => "VmbErrorApiNotStarted",
        VmbErrorNotFound => "VmbErrorNotFound",
        VmbErrorBadHandle => "VmbErrorBadHandle",
        VmbErrorDeviceNotOpen => "VmbErrorDeviceNotOpen",
        VmbErrorInvalidAccess => "VmbErrorInvalidAccess",
        VmbErrorBadParameter => "VmbErrorBadParameter",
        VmbErrorStructSize => "VmbErrorStructSize",
        VmbErrorMoreData => "VmbErrorMoreData",
        VmbErrorWrongType => "VmbErrorWrongType",
        VmbErrorInvalidValue => "VmbErrorInvalidValue",
        VmbErrorTimeout => "VmbErrorTimeout",
        VmbErrorOther => "VmbErrorOther",
        VmbErrorResources => "VmbErrorResources",
        VmbErrorInvalidCall => "VmbErrorInvalidCall",
        VmbErrorNoTL => "VmbErrorNoTL",
        VmbErrorNotImplemented => "VmbErrorNotImplemented",
        VmbErrorNotSupported => "VmbErrorNotSupported",
        VmbErrorIncomplete => "VmbErrorIncomplete",
        VmbErrorIO => "VmbErrorIO",
        _ => "unknown error",
    }
}

#[derive(thiserror::Error, Debug)]
#[error("Vimba Error {code}: {msg}")]
pub struct VimbaError {
    pub code: i32,
    pub msg: &'static str,
}

impl From<i32> for VimbaError {
    fn from(code: i32) -> VimbaError {
        VimbaError {
            code,
            msg: err_str(code),
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("{source}")]
    LibLoading {
        #[from]
        source: libloading::Error,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("{source}")]
    Vimba {
        #[from]
        source: VimbaError,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("{source}")]
    NulError {
        #[from]
        source: std::ffi::NulError,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("{source}")]
    Utf8Error {
        #[from]
        source: std::str::Utf8Error,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("unknown pixel format {fmt}")]
    UnknownPixelFormat {
        fmt: String,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("unknown pixel format code 0x{code:X}")]
    UnknownPixelFormatCode {
        code: u32,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("invalid call")]
    InvalidCall {
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
}

#[cfg(feature = "backtrace")]
impl Error {
    pub fn my_backtrace(&self) -> &Backtrace {
        use Error::*;
        match self {
            LibLoading {
                source: _,
                backtrace,
            } => backtrace,
            Vimba {
                source: _,
                backtrace,
            } => backtrace,
            NulError {
                source: _,
                backtrace,
            } => backtrace,
            Utf8Error {
                source: _,
                backtrace,
            } => backtrace,
            UnknownPixelFormat { fmt: _, backtrace } => backtrace,
            UnknownPixelFormatCode { code: _, backtrace } => backtrace,
            InvalidCall { backtrace } => backtrace,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

fn vimba_err(err: i32) -> std::result::Result<(), VimbaError> {
    if err == VmbErrorType::VmbErrorSuccess {
        Ok(())
    } else {
        Err(VimbaError::from(err))
    }
}

macro_rules! vimba_call_no_err {
    ($expr: expr) => {{
        log::debug!("calling: {} {}:{}", stringify!($expr), file!(), line!());
        unsafe { $expr }
    }};
}

macro_rules! vimba_call {
    ($expr: expr) => {{
        let errcode = vimba_call_no_err!($expr);
        log::debug!("  errcode: {}", errcode);

        vimba_err(errcode)
    }};
}

pub struct VimbaLibrary {
    pub vimba_lib: vimba_sys::VimbaC,
    started: bool,
}

impl VimbaLibrary {
    pub fn new() -> std::result::Result<Self, Error> {
        let vimbac_path = match std::env::var_os("VIMBAC_LIB_PATH") {
            Some(vimbac_path) => std::path::PathBuf::from(vimbac_path),
            None => {
                #[cfg(target_os = "windows")]
                let vimbac_path =
                    r#"C:\Program Files\Allied Vision\Vimba_6.0\VimbaC\Lib\Win64\VimbaC.dll"#;

                #[cfg(not(target_os = "windows"))]
                let vimbac_path = "/opt/vimba/Vimba_6_0/VimbaC/DynamicLib/x86_64bit/libVimbaC.so";
                std::path::PathBuf::from(vimbac_path)
            }
        };

        Self::from_dynamic_lib_path(vimbac_path)
    }

    pub fn from_dynamic_lib_path<P: AsRef<std::path::Path>>(
        vimbac_path: P,
    ) -> std::result::Result<Self, Error> {
        let vimba_lib = unsafe { vimba_sys::VimbaC::new(vimbac_path.as_ref()) }?;

        vimba_call!(vimba_lib.VmbStartup())?;
        Ok(VimbaLibrary {
            vimba_lib,
            started: true,
        })
    }

    pub fn n_cameras(&self) -> Result<usize> {
        let mut n_count = 0;
        vimba_call!(self
            .vimba_lib
            .VmbCamerasList(std::ptr::null_mut(), 0, &mut n_count, 0))?;
        Ok(n_count as usize)
    }

    pub fn camera_info(&self, n_count: usize) -> Result<Vec<CameraInfo>> {
        let mut cameras: Vec<VmbCameraInfo_t> = vec![
            VmbCameraInfo_t {
                cameraIdString: std::ptr::null_mut(),
                cameraName: std::ptr::null_mut(),
                modelName: std::ptr::null_mut(),
                serialString: std::ptr::null_mut(),
                permittedAccess: 0,
                interfaceIdString: std::ptr::null_mut(),
            };
            n_count as usize
        ];

        let mut n_found_count = 0;
        vimba_call!(self.vimba_lib.VmbCamerasList(
            cameras[..].as_mut_ptr(),
            n_count.try_into().unwrap(),
            &mut n_found_count,
            std::mem::size_of::<VmbCameraInfo_t>().try_into().unwrap()
        ))?;

        let result = cameras
            .into_iter()
            .map(|ci| CameraInfo {
                camera_id_string: unsafe { std::ffi::CStr::from_ptr(ci.cameraIdString).to_str() }
                    .unwrap()
                    .to_string(),
                camera_name: unsafe { std::ffi::CStr::from_ptr(ci.cameraName).to_str() }
                    .unwrap()
                    .to_string(),
                model_name: unsafe { std::ffi::CStr::from_ptr(ci.modelName).to_str() }
                    .unwrap()
                    .to_string(),
                serial_string: unsafe { std::ffi::CStr::from_ptr(ci.serialString).to_str() }
                    .unwrap()
                    .to_string(),
                permitted_access: AccessMode::new(ci.permittedAccess.try_into().unwrap()),
                interface_id_string: unsafe {
                    std::ffi::CStr::from_ptr(ci.interfaceIdString).to_str()
                }
                .unwrap()
                .to_string(),
            })
            .collect();
        Ok(result)
    }
}

impl Drop for VimbaLibrary {
    fn drop(&mut self) {
        if self.started {
            vimba_call_no_err!(self.vimba_lib.VmbShutdown());
            self.started = false;
        }
    }
}

pub struct VersionInfo {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl VersionInfo {
    pub fn new(vimba_c: &vimba_sys::VimbaC) -> Result<Self> {
        let mut version_info = VmbVersionInfo_t {
            major: 0,
            minor: 0,
            patch: 0,
        };
        vimba_call!(vimba_c.VmbVersionQuery(
            &mut version_info,
            std::mem::size_of::<VmbVersionInfo_t>() as u32
        ))?;
        Ok(Self {
            major: version_info.major,
            minor: version_info.minor,
            patch: version_info.patch,
        })
    }
}

#[derive(Debug)]
pub struct AccessMode {
    code: u32,
}

impl AccessMode {
    pub fn new(code: u32) -> Self {
        Self { code }
    }
    pub fn as_u32(&self) -> u32 {
        self.code
    }
}

pub mod access_mode {
    use vimba_sys::VmbAccessModeType::*;
    pub const FULL: crate::AccessMode = crate::AccessMode {
        code: VmbAccessModeFull,
    };
}

#[derive(Debug)]
pub struct CameraInfo {
    pub camera_id_string: String,
    pub camera_name: String,
    pub model_name: String,
    pub serial_string: String,
    pub permitted_access: AccessMode,
    pub interface_id_string: String,
}

pub struct Camera<'lib> {
    handle: VmbHandle_t,
    is_open: bool,
    vimba_lib: &'lib vimba_sys::VimbaC,
}

unsafe impl<'lib> Send for Camera<'lib> {}

fn _test_camera_is_send() {
    // Compile-time test to ensure Camera implements Send trait.
    fn implements<T: Send>() {}
    implements::<Camera>();
}

impl<'lib> std::fmt::Debug for Camera<'lib> {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        write!(fmt, "Camera {{")?;
        write!(fmt, " self.handle {:p},", self.handle)?;
        write!(fmt, "}}")?;
        Ok(())
    }
}

impl<'lib> Camera<'lib> {
    pub fn open(
        camera_id: &str,
        access_mode: AccessMode,
        vimba_lib: &'lib vimba_sys::VimbaC,
    ) -> Result<Self> {
        let data = std::ffi::CString::new(camera_id)?;
        let mut handle = std::mem::MaybeUninit::<VmbHandle_t>::uninit();
        vimba_call!(vimba_lib.VmbCameraOpen(
            data.as_ptr(),
            access_mode.as_u32(),
            handle.as_mut_ptr()
        ))?;
        let handle = unsafe { handle.assume_init() };
        let result = Self {
            handle,
            is_open: true,
            vimba_lib,
        };
        log::debug!("opening {:?}", result);
        Ok(result)
    }

    pub fn close(mut self) -> Result<()> {
        if self.is_open {
            vimba_call!(self.vimba_lib.VmbCameraClose(self.handle))?;
        }
        self.is_open = false; // prevent closing again on drop
        Ok(())
    }

    pub fn handle(&self) -> VmbHandle_t {
        self.handle
    }

    pub fn feature_enum(&self, feature_name: &str) -> Result<&'static str> {
        let mut result: *const std::os::raw::c_char = std::ptr::null_mut();
        let data = std::ffi::CString::new(feature_name)?;
        vimba_call!(self
            .vimba_lib
            .VmbFeatureEnumGet(self.handle, data.as_ptr(), &mut result))?;
        Ok(unsafe { std::ffi::CStr::from_ptr(result).to_str()? })
    }

    pub fn feature_enum_range_query(&self, feature_name: &str) -> Result<Vec<String>> {
        let name = std::ffi::CString::new(feature_name)?;
        let mut num_filled = 0;
        // initial query: get size of array
        vimba_call!(self.vimba_lib.VmbFeatureEnumRangeQuery(
            self.handle,
            name.as_ptr(),
            std::ptr::null_mut(),
            0,
            &mut num_filled,
        ))?;

        let mut p_name_array = vec![std::ptr::null(); num_filled.try_into().unwrap()];

        let mut num_final = 0;
        vimba_call!(self.vimba_lib.VmbFeatureEnumRangeQuery(
            self.handle,
            name.as_ptr(),
            p_name_array.as_mut_ptr(),
            num_filled,
            &mut num_final,
        ))?;

        (0..num_final as usize)
            .map(|i| {
                let c_str_ptr = p_name_array[i];
                let value = unsafe { std::ffi::CStr::from_ptr(c_str_ptr) }
                    .to_str()?
                    .to_string();
                Ok(value)
            })
            .into_iter()
            .collect()
    }

    pub fn feature_enum_set(&self, feature_name: &str, value: &str) -> Result<()> {
        let value_c = std::ffi::CString::new(value)?;
        let name = std::ffi::CString::new(feature_name)?;
        vimba_call!(self.vimba_lib.VmbFeatureEnumSet(
            self.handle,
            name.as_ptr(),
            value_c.as_ptr()
        ))?;
        Ok(())
    }

    // pub fn features_list(&self) -> Result<Vec<FeatureInfo>> {
    //     let mut num_found = 0;
    //     vimba_call!(self.vimba_lib.VmbFeaturesList(
    //         self.handle,
    //         std::ptr::null_mut(),
    //         0,
    //         &mut num_found,
    //         std::mem::size_of::<VmbFeatureInfo_t>().try_into().unwrap()
    //     ))?;

    //     let mut feature_infos =
    //         vec![std::ptr::null_mut() as *mut VmbFeatureInfo_t; num_found.try_into().unwrap()];
    //     let mut num_filled = 0;
    //     vimba_call!(self.vimba_lib.VmbFeaturesList(
    //         self.handle,
    //         *feature_infos.as_mut_ptr(),
    //         num_found,
    //         &mut num_filled,
    //         std::mem::size_of::<VmbFeatureInfo_t>().try_into().unwrap()
    //     ))?;

    //     let result = feature_infos.into_iter().map(From::from).collect();
    //     Ok(result)
    // }

    /// Query the access permissions of feature with `name`.
    ///
    /// The return value is (is_readable, is_writeable).
    pub fn feature_access_query(&self, name: &str) -> Result<(bool, bool)> {
        let mut is_readable = 0;
        let mut is_writeable = 0;
        vimba_call!(self.vimba_lib.VmbFeatureAccessQuery(
            self.handle,
            name.as_ptr() as _,
            &mut is_readable,
            &mut is_writeable,
        ))?;

        Ok((is_readable != 0, is_writeable != 0))
    }

    // pub fn feature_string(&self, feature_name: &str) -> Result<&str> {
    //     let mut result: *const std::os::raw::c_char = std::ptr::null_mut();
    //     let data = std::ffi::CString::new(feature_name)?;
    //     vimba_call!(self.vimba_lib.VmbFeatureStringGet(self.handle, data.as_ptr(), &mut result))?;
    //     Ok(unsafe { std::ffi::CStr::from_ptr(result).to_str()? })
    // }

    pub fn feature_string_set(&self, feature_name: &str, value: &str) -> Result<()> {
        let value_c = std::ffi::CString::new(value)?;
        let name = std::ffi::CString::new(feature_name)?;
        vimba_call!(self.vimba_lib.VmbFeatureStringSet(
            self.handle,
            name.as_ptr(),
            value_c.as_ptr()
        ))?;
        Ok(())
    }

    pub fn feature_int(&self, feature_name: &str) -> Result<i64> {
        let mut result = 0;
        let data = std::ffi::CString::new(feature_name)?;
        vimba_call!(self
            .vimba_lib
            .VmbFeatureIntGet(self.handle, data.as_ptr(), &mut result))?;
        Ok(result)
    }

    pub fn feature_float(&self, feature_name: &str) -> Result<f64> {
        let mut result = 0.0;
        let data = std::ffi::CString::new(feature_name)?;
        vimba_call!(self
            .vimba_lib
            .VmbFeatureFloatGet(self.handle, data.as_ptr(), &mut result))?;
        Ok(result)
    }

    pub fn feature_float_set(&self, feature_name: &str, value: f64) -> Result<()> {
        let data = std::ffi::CString::new(feature_name)?;
        vimba_call!(self
            .vimba_lib
            .VmbFeatureFloatSet(self.handle, data.as_ptr(), value))?;
        Ok(())
    }

    pub fn feature_float_range_query(&self, feature_name: &str) -> Result<(f64, f64)> {
        let mut min = 0.0;
        let mut max = 0.0;
        let data = std::ffi::CString::new(feature_name)?;
        vimba_call!(self.vimba_lib.VmbFeatureFloatRangeQuery(
            self.handle,
            data.as_ptr(),
            &mut min,
            &mut max
        ))?;
        Ok((min, max))
    }

    pub fn feature_boolean(&self, feature_name: &str) -> Result<bool> {
        let mut result = 0;
        let data = std::ffi::CString::new(feature_name)?;
        vimba_call!(self
            .vimba_lib
            .VmbFeatureBoolGet(self.handle, data.as_ptr(), &mut result))?;
        Ok(result != 0)
    }
    pub fn feature_boolean_set(&self, feature_name: &str, value: bool) -> Result<()> {
        let value_u8 = if value { 1 } else { 0 };
        let data = std::ffi::CString::new(feature_name)?;
        vimba_call!(self
            .vimba_lib
            .VmbFeatureBoolSet(self.handle, data.as_ptr(), value_u8))?;
        Ok(())
    }

    pub fn command_run(&self, command_name: &str) -> Result<()> {
        log::debug!("camera {:?} command_run {}", self, command_name);
        let data = std::ffi::CString::new(command_name)?;
        vimba_call!(self
            .vimba_lib
            .VmbFeatureCommandRun(self.handle, data.as_ptr()))?;
        Ok(())
    }

    pub fn pixel_format(&self) -> Result<formats::pixel_format::PixFmt> {
        let pixel_format = self.feature_enum("PixelFormat")?;
        str_to_pixel_format(pixel_format)
    }

    pub fn allocate_buffer(&self) -> Result<Vec<u8>> {
        let payload_size = self.feature_int("PayloadSize")?;
        Ok(vec![0u8; payload_size.try_into().unwrap()])
    }

    pub fn frame_announce(&self, frame: &mut Frame) -> Result<()> {
        if frame.already_announced {
            return Err(Error::InvalidCall {
                #[cfg(feature = "backtrace")]
                backtrace: Backtrace::capture(),
            });
        }

        log::debug!("camera {:?} announcing frame {:?}", self, frame);

        vimba_call!(self.vimba_lib.VmbFrameAnnounce(
            self.handle,
            &*frame.frame,
            std::mem::size_of::<VmbFrame_t>().try_into().unwrap()
        ))?;

        frame.already_announced = true;
        Ok(())
    }

    pub fn frame_revoke(&self, frame: &mut Frame) -> Result<()> {
        log::debug!("camera {:?} revoking frame {:?}", self, frame);
        vimba_call!(self.vimba_lib.VmbFrameRevoke(self.handle, &*frame.frame,))?;
        frame.already_announced = false;
        Ok(())
    }

    pub fn capture_start(&self) -> Result<()> {
        log::debug!("camera {:?} capture start", self);
        vimba_call!(self.vimba_lib.VmbCaptureStart(self.handle))?;
        Ok(())
    }

    pub fn capture_end(&self) -> Result<()> {
        vimba_call!(self.vimba_lib.VmbCaptureEnd(self.handle))?;
        Ok(())
    }

    pub fn capture_frame_queue(&self, frame: &mut Frame) -> Result<()> {
        log::debug!("camera {:?} queueing frame {:?}", self, frame);
        vimba_call!(self
            .vimba_lib
            .VmbCaptureFrameQueue(self.handle, &*frame.frame, None))?;
        Ok(())
    }
    pub fn capture_frame_queue_with_callback(
        &self,
        frame: &mut Frame,
        callback: VmbFrameCallback,
    ) -> Result<()> {
        log::debug!("camera {:?} queueing frame {:?}", self, frame);
        vimba_call!(self
            .vimba_lib
            .VmbCaptureFrameQueue(self.handle, &*frame.frame, callback))?;
        Ok(())
    }

    pub fn capture_queue_flush(&self) -> Result<()> {
        vimba_call!(self.vimba_lib.VmbCaptureQueueFlush(self.handle))?;
        Ok(())
    }

    pub fn capture_frame_wait(&self, frame: &mut Frame, timeout: u32) -> Result<()> {
        log::debug!("camera {:?} waiting for frame {:?}", self, frame);
        vimba_call!(self
            .vimba_lib
            .VmbCaptureFrameWait(self.handle, &*frame.frame, timeout))?;
        Ok(())
    }

    pub fn camera_settings_save<P: AsRef<std::path::Path>>(
        &self,
        out_path: P,
        p_settings: &mut FeaturePersistentSettings,
    ) -> Result<()> {
        let mut buf = path_to_bytes(out_path);
        buf.push(0);
        let sz = std::mem::size_of::<VmbFeaturePersistSettings_t>();
        let sz = sz.try_into().unwrap(); // convert to u32 from usize
        vimba_call!(self.vimba_lib.VmbCameraSettingsSave(
            self.handle,
            buf.as_ptr() as *const i8,
            (&mut p_settings.inner) as *mut _,
            sz
        ))?;
        Ok(())
    }

    pub fn camera_settings_load<P: AsRef<std::path::Path>>(
        &self,
        in_path: P,
        p_settings: &mut FeaturePersistentSettings,
    ) -> Result<()> {
        let mut buf = path_to_bytes(in_path);
        buf.push(0);
        let sz = std::mem::size_of::<VmbFeaturePersistSettings_t>();
        let sz = sz.try_into().unwrap(); // convert to u32 from usize
        vimba_call!(self.vimba_lib.VmbCameraSettingsLoad(
            self.handle,
            buf.as_ptr() as *const i8,
            (&mut p_settings.inner) as *mut _,
            sz
        ))?;
        Ok(())
    }
}

impl<'lib> Drop for Camera<'lib> {
    fn drop(&mut self) {
        if self.is_open {
            vimba_call!(self.vimba_lib.VmbCameraClose(self.handle)).unwrap();
            self.is_open = false;
        }
    }
}

pub struct FeaturePersistentSettings {
    inner: VmbFeaturePersistSettings_t,
}

impl Default for FeaturePersistentSettings {
    fn default() -> Self {
        // These values are saved in .xml file from the Vimba Viewer 5.1 GUI.
        Self {
            inner: VmbFeaturePersistSettings_t {
                persistType: vimba_sys::VmbFeaturePersistType::VmbFeaturePersistNoLUT
                    as vimba_sys::VmbFeaturePersist_t,
                maxIterations: 5,
                loggingLevel: 4,
            },
        }
    }
}

// TODO: should we use `std::pin::Pin` to ensure that `buffer` is not moved?
pub struct Frame {
    buffer: Vec<u8>,
    // the address of `frame` is used as a key by Vimba to remember locations, so it must remain fixed.
    // `frame` contains a pointer to `buffer`
    frame: Pin<Box<VmbFrame_t>>,
    already_announced: bool,
}

unsafe impl Send for Frame {}

fn _test_frame_is_send() {
    // Compile-time test to ensure Frame implements Send trait.
    fn implements<T: Send>() {}
    implements::<Frame>();
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        write!(fmt, "Frame {{")?;
        write!(
            fmt,
            " frame as *const VmbFrame_t {:p},",
            &*self.frame as *const VmbFrame_t
        )?;
        write!(fmt, " buffer.as_ptr() {:p},", self.buffer.as_ptr())?;
        write!(fmt, " buffer.len() {},", self.buffer.len())?;
        write!(fmt, " frame.buffer {:p},", self.frame.buffer)?;
        write!(fmt, " frame.bufferSize {},", self.frame.bufferSize)?;
        write!(fmt, " already_announced {:?},", self.already_announced)?;
        write!(fmt, "}}")?;
        Ok(())
    }
}

impl Frame {
    pub fn new(mut buffer: Vec<u8>) -> Self {
        let frame = Box::pin(VmbFrame_t {
            buffer: buffer.as_mut_ptr() as _,
            bufferSize: buffer.len().try_into().unwrap(),
            context: [std::ptr::null_mut(); 4],
            receiveStatus: 0,
            receiveFlags: 0,
            imageSize: 0,
            ancillarySize: 0,
            pixelFormat: 0,
            width: 0,
            height: 0,
            offsetX: 0,
            offsetY: 0,
            frameID: 0,
            timestamp: 0,
        });

        Self {
            buffer,
            frame,
            already_announced: false,
        }
    }
    #[inline]
    pub fn is_complete(&self) -> bool {
        self.frame.receiveStatus == VmbFrameStatusType::VmbFrameStatusComplete
    }
    #[inline]
    pub fn width(&self) -> u32 {
        self.frame.width
    }
    #[inline]
    pub fn height(&self) -> u32 {
        self.frame.height
    }
    #[inline]
    pub fn image_size(&self) -> usize {
        self.frame.imageSize.try_into().unwrap()
    }
    #[inline]
    pub fn buffer(&self) -> &[u8] {
        &self.buffer[..self.image_size()]
    }
    #[inline]
    pub fn frame_id(&self) -> u64 {
        self.frame.frameID
    }
    #[inline]
    pub fn timestamp(&self) -> u64 {
        self.frame.timestamp
    }
    #[inline]
    pub fn pixel_format(&self) -> Result<formats::PixFmt> {
        pixel_format_code(self.frame.pixelFormat)
    }
}

pub fn pixel_format_code(code: u32) -> Result<formats::PixFmt> {
    use formats::PixFmt::*;
    use vimba_sys::VmbPixelFormatType::*;
    #[allow(non_upper_case_globals)]
    let fmt = match code {
        VmbPixelFormatMono8 => Mono8,
        VmbPixelFormatBayerGR8 => BayerGR8,
        VmbPixelFormatBayerRG8 => BayerRG8,
        VmbPixelFormatBayerGB8 => BayerGB8,
        VmbPixelFormatBayerBG8 => BayerBG8,
        VmbPixelFormatRgb8 => RGB8,
        // VmbPixelFormatMono10 => Mono10,
        // VmbPixelFormatMono10p => Mono10p,
        // VmbPixelFormatMono12 => Mono12,
        // VmbPixelFormatMono12p => Mono12p,
        // VmbPixelFormatMono16 => Mono16,
        _code_signed => {
            return Err(Error::UnknownPixelFormatCode {
                code,
                #[cfg(feature = "backtrace")]
                backtrace: Backtrace::capture(),
            });
        }
    };
    Ok(fmt)
}

pub fn str_to_pixel_format(pixel_format: &str) -> Result<formats::pixel_format::PixFmt> {
    use formats::pixel_format::PixFmt::*;
    Ok(match pixel_format {
        "Mono8" => Mono8,
        "RGB8" => RGB8,
        "BayerRG8" => BayerRG8,
        // "Mono10" => Mono10,
        // "Mono10p" => Mono10p,
        // "Mono12" => Mono12,
        // "Mono12p" => Mono12p,
        // "Mono16" => Mono16,
        fmt => {
            return Err(Error::UnknownPixelFormat {
                fmt: fmt.to_string(),
                #[cfg(feature = "backtrace")]
                backtrace: Backtrace::capture(),
            });
        }
    })
}

pub fn pixel_format_to_str(pixfmt: formats::pixel_format::PixFmt) -> Result<&'static str> {
    use formats::pixel_format::PixFmt::*;
    Ok(match pixfmt {
        Mono8 => "Mono8",
        RGB8 => "RGB8",
        // Mono10 => "Mono10",
        // Mono10p => "Mono10p",
        // Mono12 => "Mono12",
        // Mono12p => "Mono12p",
        // Mono16 => "Mono16",
        _ => {
            return Err(Error::UnknownPixelFormat {
                fmt: format!("pixfmt {:?}", pixfmt),
                #[cfg(feature = "backtrace")]
                backtrace: Backtrace::capture(),
            });
        }
    })
}

// #[derive(Debug, Clone, PartialEq)]
// pub struct FeatureInfo {
//     pub name: String,
//     pub data_type: DataType,
//     pub access_flags: AccessFlags,
//     // pub category: String,
//     pub display_name: String,
//     // pub polling_time: u32,
//     // pub unit: String,
//     // pub representation: String,
//     // pub visibility: Visibility,
//     // pub tooltip: String,
//     // pub description: String,
//     // pub sfnc_namespace: String,
//     // pub is_streamable: bool,
//     // pub has_affected_features: bool,
//     // pub has_selected_features: bool,
// }

// #[derive(Debug, Clone, PartialEq)]
// pub enum DataType {
//     Unknown,
//     Int,
//     Float,
//     Enum,
//     String,
//     Bool,
//     Command,
//     Raw,
//     None,
// }

// impl DataType {
//     pub fn new(orig: vimba_sys::VmbFeatureData_t) -> Self {
//         use vimba_sys::VmbFeatureDataType::*;
//         use DataType::*;
//         #[allow(non_upper_case_globals)]
//         match orig as i32 {
//             VmbFeatureDataUnknown => Unknown,
//             VmbFeatureDataInt => Int,
//             VmbFeatureDataFloat => Float,
//             VmbFeatureDataEnum => Enum,
//             VmbFeatureDataString => String,
//             VmbFeatureDataBool => Bool,
//             VmbFeatureDataCommand => Command,
//             VmbFeatureDataRaw => Raw,
//             VmbFeatureDataNone => None,
//             o => {
//                 panic!("unknown data type {}", o);
//             }
//         }
//     }
// }

// #[derive(Debug, Clone, PartialEq)]
// pub struct AccessFlags {
//     flags: vimba_sys::VmbFeatureFlags_t,
// }

// impl AccessFlags {
//     pub fn new(orig: vimba_sys::VmbFeatureFlags_t) -> Self {
//         Self { flags: orig }
//     }
// }

// impl From<*mut VmbFeatureInfo_t> for FeatureInfo {
//     fn from(orig: *mut VmbFeatureInfo_t) -> Self {
//         let name = unsafe { std::ffi::CStr::from_ptr((*orig).name).to_str() }
//             .unwrap()
//             .to_string();
//         let display_name = unsafe { std::ffi::CStr::from_ptr((*orig).displayName).to_str() }
//             .unwrap()
//             .to_string();

//         let data_type = DataType::new(unsafe { (*orig).featureDataType });
//         let access_flags = AccessFlags::new(unsafe { (*orig).featureFlags });

//         Self {
//             name,
//             data_type,
//             access_flags,
//             display_name,
//         }
//     }
// }

/// Convert path to bytes
///
/// From https://stackoverflow.com/a/57667836/1633026
#[cfg(unix)]
fn path_to_bytes<P: AsRef<std::path::Path>>(path: P) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_ref().as_os_str().as_bytes().to_vec()
}

/// Convert path to bytes
///
/// From https://stackoverflow.com/a/57667836/1633026
#[cfg(not(unix))]
fn path_to_bytes<P: AsRef<std::path::Path>>(path: P) -> Vec<u8> {
    // On Windows, could use std::os::windows::ffi::OsStrExt to encode_wide(),
    // but you end up with a Vec<u16> instead of a Vec<u8>, so that doesn't
    // really help. This is probably wrong for paths with non ascii characters,
    // but the Vimba docs don't specify what encoding is used, so it is hard to
    // know the right thing to do.
    path.as_ref().to_string_lossy().to_string().into_bytes()
}
