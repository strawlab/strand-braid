use crate::ffi::*;
use crate::load::SharedLibrary;
use crate::{NvInt, NvencError};
use std::ptr::addr_of_mut;
use std::{
    fmt::{Debug, Formatter},
    mem::MaybeUninit,
    pin::Pin,
    rc::Rc,
};

#[cfg(feature = "backtrace")]
use std::backtrace::Backtrace;

macro_rules! api_call {
    ($expr:expr) => {{
        let status = $expr;
        if status != _NVENCSTATUS::NV_ENC_SUCCESS {
            return Err(NvencError::ErrCode {
                status,
                message: crate::error::code_to_string(status),
                #[cfg(feature = "backtrace")]
                backtrace: Backtrace::capture(),
            });
        }
    }};
}

macro_rules! load_func {
    ($inner:expr, $ident:ident) => {{
        let func = if let Some(func) = $inner.$ident {
            func
        } else {
            return Err(NvencError::NameFFIError {
                name: stringify!($ident).to_string(),
                #[cfg(feature = "backtrace")]
                backtrace: Backtrace::capture(),
            });
        };

        Ok(func)
    }};
}

macro_rules! get_func {
    ($lib:expr, $name:expr) => {{
        unsafe { $lib.library.get($name) }.map_err(|source| NvencError::NameFFIError2 {
            name: String::from_utf8_lossy($name).to_string(),
            source,
            #[cfg(feature = "backtrace")]
            backtrace: Backtrace::capture(),
        })
        // format!(
        //     "the name {} could not be opened: {}", String::from_utf8_lossy($name), e))?
    }};
}

pub fn init(library: &SharedLibrary) -> Result<Rc<LibNvEncode<'_>>, NvencError> {
    let lib_nv_encode = LibNvEncode {
        NvEncodeAPICreateInstance: get_func!(library, b"NvEncodeAPICreateInstance\0")?,
        NvEncodeAPIGetMaxSupportedVersion: get_func!(
            library,
            b"NvEncodeAPIGetMaxSupportedVersion\0"
        )?,
    };

    Ok(Rc::new(lib_nv_encode))
}

#[allow(non_snake_case, dead_code)]
pub struct LibNvEncode<'lib> {
    NvEncodeAPICreateInstance:
        libloading::Symbol<'lib, extern "C" fn(*mut NV_ENCODE_API_FUNCTION_LIST) -> NVENCSTATUS>,
    NvEncodeAPIGetMaxSupportedVersion:
        libloading::Symbol<'lib, extern "C" fn(*mut u32) -> NVENCSTATUS>,
}

impl<'lib> LibNvEncode<'lib> {
    pub fn api_create_instance(
        self_: Rc<Self>,
    ) -> Result<NvEncodeApiFunctionList<'lib>, NvencError> {
        let function_list = MaybeUninit::zeroed();
        let mut function_list: NV_ENCODE_API_FUNCTION_LIST = unsafe { function_list.assume_init() };

        function_list.version = NV_ENCODE_API_FUNCTION_LIST_VER;

        api_call!((*self_.NvEncodeAPICreateInstance)(&mut function_list));
        Ok(NvEncodeApiFunctionList {
            inner: function_list,
            _libnvencode: self_.clone(),
        })
    }
    pub fn api_get_max_supported_version(&self) -> Result<ApiVersion, NvencError> {
        let mut value = 0;
        api_call!((*self.NvEncodeAPIGetMaxSupportedVersion)(&mut value));
        Ok(ApiVersion {
            major: value >> 4,
            minor: value & 0xf,
        })
    }
}

#[derive(Debug)]
pub struct ApiVersion {
    pub major: u32,
    pub minor: u32,
}

/// The lifetime 'lib refers to the shared library.
#[derive(Clone)]
pub struct NvEncodeApiFunctionList<'lib> {
    inner: NV_ENCODE_API_FUNCTION_LIST,
    _libnvencode: Rc<LibNvEncode<'lib>>,
}

struct EncoderPtr(*mut std::ffi::c_void);

impl<'lib> NvEncodeApiFunctionList<'lib> {
    pub fn new_encoder(
        &self,
        mut ctx: dynlink_cuda::CudaContext,
    ) -> Result<Rc<Encoder<'lib>>, NvencError> {
        let func = load_func!(self.inner, nvEncOpenEncodeSessionEx)?;
        let params = MaybeUninit::zeroed();
        let mut params: NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS = unsafe { params.assume_init() };

        params.version = NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS_VER;
        params.apiVersion = NVENCAPI_VERSION;
        params.deviceType = _NV_ENC_DEVICE_TYPE::NV_ENC_DEVICE_TYPE_CUDA;
        params.device = ctx.as_mut_void_ptr();

        let mut encoder: *mut std::ffi::c_void = std::ptr::null_mut();
        api_call!(unsafe { func(&mut params, &mut encoder) });
        Ok(Rc::new(Encoder {
            parent: self.clone(),
            inner: EncoderPtr(encoder),
            destroyed: false,
        }))
    }
}

/// The lifetime 'lib refers to the shared library.
pub struct Encoder<'lib> {
    parent: NvEncodeApiFunctionList<'lib>,
    inner: EncoderPtr,
    destroyed: bool,
}

impl<'lib> Encoder<'lib> {
    pub fn get_encode_guid_count(&self) -> Result<u32, NvencError> {
        let func = load_func!(self.parent.inner, nvEncGetEncodeGUIDCount)?;
        let mut value = 0;
        api_call!(unsafe { func(self.inner.0, &mut value) });
        Ok(value)
    }

    pub fn get_encode_preset_config(
        &self,
        encode: GUID,
        preset: GUID,
    ) -> Result<EncodeConfig, NvencError> {
        let func = load_func!(self.parent.inner, nvEncGetEncodePresetConfig)?;

        let config = MaybeUninit::zeroed();
        let mut config: NV_ENC_PRESET_CONFIG = unsafe { config.assume_init() };

        config.presetCfg.version = NV_ENC_CONFIG_VER;
        config.version = NV_ENC_PRESET_CONFIG_VER;

        api_call!(unsafe { func(self.inner.0, encode, preset, &mut config) });
        Ok(EncodeConfig {
            config: config.presetCfg,
        })
    }

    // TODO: return an InitializedEncoder type (forces the encoder to be initialized).
    pub fn initialize(&self, init_params: &InitParams) -> Result<(), NvencError> {
        // There seem to be under-documented minimum width requirements.
        // https://forums.developer.nvidia.com/t/minimum-width-in-turing-gpus/155566
        let func = load_func!(self.parent.inner, nvEncInitializeEncoder)?;
        // We can safely assume the params won't be changed by the API
        // according to the API documentation
        let params = init_params.init_params;
        let params = &params as *const NV_ENC_INITIALIZE_PARAMS;
        let params = params as *mut NV_ENC_INITIALIZE_PARAMS;

        api_call!(unsafe { func(self.inner.0, params) });

        Ok(())
    }

    /// Allocate a new buffer managed by NVIDIA Video SDK
    pub fn alloc_input_buffer(
        self_: &Rc<Self>,
        width: u32,
        height: u32,
        format: BufferFormat,
    ) -> Result<InputBuffer<'lib>, NvencError> {
        let func = load_func!(self_.parent.inner, nvEncCreateInputBuffer)?;

        let params = MaybeUninit::zeroed();
        let mut params: NV_ENC_CREATE_INPUT_BUFFER = unsafe { params.assume_init() };

        params.version = NV_ENC_CREATE_INPUT_BUFFER_VER;
        params.width = width;
        params.height = height;
        params.bufferFmt = format as NvInt;

        api_call!(unsafe { func(self_.inner.0, &mut params) });

        Ok(InputBuffer {
            encoder: self_.clone(),
            ptr: params.inputBuffer,
            format,
            width,
            height,
            destroyed: false,
        })
    }

    pub fn alloc_output_buffer(self_: &Rc<Self>) -> Result<OutputBuffer<'lib>, NvencError> {
        let func = load_func!(self_.parent.inner, nvEncCreateBitstreamBuffer)?;

        let params = MaybeUninit::zeroed();
        let mut params: NV_ENC_CREATE_BITSTREAM_BUFFER = unsafe { params.assume_init() };

        params.version = NV_ENC_CREATE_BITSTREAM_BUFFER_VER;
        api_call!(unsafe { func(self_.inner.0, &mut params) });
        Ok(OutputBuffer {
            encoder: self_.clone(),
            ptr: params.bitstreamBuffer,
            destroyed: false,
        })
    }

    /// Main entry to encode a video frame with a given presentation time stamp.
    ///
    /// Note that since enablePTD is true, this may return
    /// NV_ENC_ERR_NEED_MORE_INPUT which should not be treated as a fatal error.
    pub fn encode_picture(
        &self,
        input: &InputBuffer,
        output: &OutputBuffer,
        pitch: usize,
        pts: std::time::Duration,
    ) -> Result<(), NvencError> {
        let func = load_func!(self.parent.inner, nvEncEncodePicture)?;

        let params = MaybeUninit::zeroed();
        let mut params: NV_ENC_PIC_PARAMS = unsafe { params.assume_init() };

        params.version = NV_ENC_PIC_PARAMS_VER;
        params.inputTimeStamp = dur2raw(&pts);
        params.inputBuffer = input.ptr;
        params.bufferFmt = input.format as NvInt;
        params.inputWidth = input.width;
        params.inputHeight = input.height;
        params.inputPitch = pitch as u32;
        params.pictureStruct = _NV_ENC_PIC_STRUCT::NV_ENC_PIC_STRUCT_FRAME;
        params.outputBitstream = output.ptr;

        api_call!(unsafe { func(self.inner.0, &mut params) });
        Ok(())
    }

    /// End the encoder stream
    ///
    /// According to the nvenc docs, this can be called multiple times.
    pub fn end_stream(&self) -> Result<(), NvencError> {
        let func = load_func!(self.parent.inner, nvEncEncodePicture)?;

        let params = MaybeUninit::zeroed();
        let mut params: NV_ENC_PIC_PARAMS = unsafe { params.assume_init() };

        params.version = NV_ENC_PIC_PARAMS_VER;
        params.encodePicFlags = _NV_ENC_PIC_FLAGS::NV_ENC_PIC_FLAG_EOS;
        api_call!(unsafe { func(self.inner.0, &mut params) });
        Ok(())
    }

    pub fn get_sequence_parameter_sets(&self) -> Result<Vec<u8>, NvencError> {
        let func = load_func!(self.parent.inner, nvEncGetSequenceParams)?;

        let mut buf: Vec<u8> = vec![0; 256];
        let mut new_len: u32 = 0;

        let mut params = NV_ENC_SEQUENCE_PARAM_PAYLOAD {
            version: NV_ENC_SEQUENCE_PARAM_PAYLOAD_VER,
            inBufferSize: buf.len().try_into().unwrap(),
            spsId: 0,
            ppsId: 0,
            spsppsBuffer: buf.as_mut_ptr() as *mut std::ffi::c_void,
            outSPSPPSPayloadSize: &mut new_len,
            reserved: [0; 250],
            reserved2: [std::ptr::null_mut(); 64],
        };

        api_call!(unsafe { func(self.inner.0, &mut params) });

        buf.truncate(new_len.try_into().unwrap());

        Ok(buf)
    }
}

pub const H264_RATE: u32 = 1_000_000;

// same as std::time::Duration::from_secs_f64 in rust 1.38
fn from_secs_f64(secs: f64) -> std::time::Duration {
    let whole_secs = secs.floor() as u64;
    let subsec_nanos = ((secs - whole_secs as f64) * 1e9).round() as u32;
    std::time::Duration::new(whole_secs, subsec_nanos)
}

fn dur2raw(dur: &std::time::Duration) -> u64 {
    (dur.as_secs_f64() * H264_RATE as f64).round() as u64
}

fn raw2dur(raw: u64) -> std::time::Duration {
    from_secs_f64((raw as f64) / H264_RATE as f64)
}

#[test]
fn test_timestamp_conversion() {
    for expected in &[0, 1, 100, 100_000, 100_000_000] {
        let dur = raw2dur(*expected);
        let actual = dur2raw(&dur);
        assert_eq!(*expected, actual);
    }
}

impl<'lib> Drop for Encoder<'lib> {
    fn drop(&mut self) {
        if !self.destroyed {
            let func = if let Some(func) = self.parent.inner.nvEncDestroyEncoder {
                func
            } else {
                panic!("No function 'nvEncDestroyEncoder'");
            };
            let status = unsafe { func(self.inner.0) };
            assert!(
                !(status != _NVENCSTATUS::NV_ENC_SUCCESS),
                "NV_ENC error code: {}",
                status
            );
            self.destroyed = true;
        }
    }
}

/// A simple wrapper of a buffer
pub struct InputBuffer<'lib> {
    encoder: Rc<Encoder<'lib>>,
    ptr: NV_ENC_INPUT_PTR,
    format: BufferFormat,
    width: u32,
    height: u32,
    destroyed: bool,
}

/// Acquired by calling `InputBuffer::lock()`
///
/// Implements Drop to automatically unlock the InputBuffer.
pub struct LockedInputBuffer<'lock, 'lib> {
    inner: &'lock InputBuffer<'lib>,
    mem: &'lock mut [u8],
    pitch: usize,
    dropped: bool,
}

impl<'lib> InputBuffer<'lib> {
    pub fn lock<'lock>(&'lock self) -> Result<LockedInputBuffer<'lock, 'lib>, NvencError> {
        let func = load_func!(self.encoder.parent.inner, nvEncLockInputBuffer)?;

        let params = MaybeUninit::zeroed();
        let mut params: NV_ENC_LOCK_INPUT_BUFFER = unsafe { params.assume_init() };

        params.version = NV_ENC_LOCK_INPUT_BUFFER_VER;
        params.inputBuffer = self.ptr;

        api_call!(unsafe { func(self.encoder.inner.0, &mut params) });

        let sz = self.format.calculate_size(params.pitch, self.height)?;

        let mem = unsafe { std::slice::from_raw_parts_mut(params.bufferDataPtr as *mut u8, sz) };

        Ok(LockedInputBuffer {
            inner: self,
            mem,
            pitch: params.pitch as usize,
            dropped: false,
        })
    }
}

impl<'lock, 'lib> Drop for LockedInputBuffer<'lock, 'lib> {
    fn drop(&mut self) {
        if !self.dropped {
            let func = if let Some(func) = self.inner.encoder.parent.inner.nvEncUnlockInputBuffer {
                func
            } else {
                panic!("No function 'nvEncUnlockInputBuffer'");
            };

            let status = unsafe { func(self.inner.encoder.inner.0, self.inner.ptr) };

            assert!(
                !(status != _NVENCSTATUS::NV_ENC_SUCCESS),
                "NV_ENC error code: {}",
                status
            );

            // As far as I understand it, slices (e.g. `self.mem` do not
            // implement Drop, so we do not need to call `std::mem::forget`
            // on our slice. Presumably the nvidia driver deallocates the
            // backing memory in this case.

            self.dropped = true;
        }
    }
}

impl<'lock, 'lib> LockedInputBuffer<'lock, 'lib> {
    pub fn mem(&self) -> &[u8] {
        self.mem
    }
    pub fn mem_mut(&mut self) -> &mut [u8] {
        self.mem
    }
    pub fn pitch(&self) -> usize {
        self.pitch
    }
}

impl<'lib> Drop for InputBuffer<'lib> {
    fn drop(&mut self) {
        if !self.destroyed {
            let func = if let Some(func) = self.encoder.parent.inner.nvEncDestroyInputBuffer {
                func
            } else {
                panic!("No function 'nvEncDestroyInputBuffer'");
            };

            let status = unsafe { func(self.encoder.inner.0, self.ptr) };
            assert!(
                !(status != _NVENCSTATUS::NV_ENC_SUCCESS),
                "NV_ENC error code: {}",
                status
            );

            self.destroyed = true;
        }
    }
}

pub struct OutputBuffer<'lib> {
    encoder: Rc<Encoder<'lib>>,
    ptr: NV_ENC_OUTPUT_PTR,
    destroyed: bool,
}

/// Acquired by calling `OutputBuffer::lock()`
///
/// Implements Drop to automatically unlock the OutputBuffer.
pub struct LockedOutputBuffer<'lock, 'lib> {
    inner: &'lock OutputBuffer<'lib>,
    mem: &'lock [u8],
    picture_type: NvInt,
    /// presentation timestamp (from onset)
    pts: std::time::Duration,
    output_time_stamp: u64,
    output_duration: u64,
    dropped: bool,
}

impl<'lib> Drop for OutputBuffer<'lib> {
    fn drop(&mut self) {
        if !self.destroyed {
            let func = if let Some(func) = self.encoder.parent.inner.nvEncDestroyBitstreamBuffer {
                func
            } else {
                panic!("No function 'nvEncDestroyBitstreamBuffer'");
            };

            let status = unsafe { func(self.encoder.inner.0, self.ptr) };
            assert!(
                !(status != _NVENCSTATUS::NV_ENC_SUCCESS),
                "NV_ENC error code: {}",
                status
            );

            self.destroyed = true;
        }
    }
}

impl<'lock, 'lib> LockedOutputBuffer<'lock, 'lib> {
    pub fn mem(&self) -> &[u8] {
        self.mem
    }
    pub fn pts(&self) -> &std::time::Duration {
        &self.pts
    }
    pub fn output_time_stamp(&self) -> u64 {
        self.output_time_stamp
    }
    pub fn output_duration(&self) -> u64 {
        self.output_duration
    }
    pub fn is_keyframe(&self) -> bool {
        use crate::ffi::_NV_ENC_PIC_TYPE::*;
        matches!(self.picture_type, NV_ENC_PIC_TYPE_I | NV_ENC_PIC_TYPE_IDR)
    }
}

impl<'lock, 'lib> Drop for LockedOutputBuffer<'lock, 'lib> {
    fn drop(&mut self) {
        if !self.dropped {
            let func = if let Some(func) = self.inner.encoder.parent.inner.nvEncUnlockBitstream {
                func
            } else {
                panic!("No function 'nvEncUnlockBitstream'");
            };

            let status = unsafe { func(self.inner.encoder.inner.0, self.inner.ptr) };

            assert!(
                !(status != _NVENCSTATUS::NV_ENC_SUCCESS),
                "NV_ENC error code: {}",
                status
            );

            // As far as I understand it, slices (e.g. `self.mem` do not
            // implement Drop, so we do not need to call `std::mem::forget`
            // on our slice. Presumably the nvidia driver deallocates the
            // backing memory in this case.

            self.dropped = true;
        }
    }
}

impl<'lib> OutputBuffer<'lib> {
    pub fn lock<'lock>(&'lock self) -> Result<LockedOutputBuffer<'lock, 'lib>, NvencError> {
        let func = load_func!(self.encoder.parent.inner, nvEncLockBitstream)?;

        let params = MaybeUninit::zeroed();
        let mut params: NV_ENC_LOCK_BITSTREAM = unsafe { params.assume_init() };

        params.version = NV_ENC_LOCK_BITSTREAM_VER;
        params.outputBitstream = self.ptr;

        api_call!(unsafe { func(self.encoder.inner.0, &mut params) });

        let output_time_stamp = params.outputTimeStamp;
        let output_duration = params.outputDuration;
        let pts = raw2dur(output_time_stamp);
        let picture_type = params.pictureType;

        let mem = unsafe {
            std::slice::from_raw_parts(
                params.bitstreamBufferPtr as *mut u8,
                params.bitstreamSizeInBytes as usize,
            )
        };

        Ok(LockedOutputBuffer {
            inner: self,
            mem,
            pts,
            output_time_stamp,
            output_duration,
            picture_type,
            dropped: false,
        })
    }
}

/// Data format of input and output buffer
#[repr(u32)]
#[derive(Copy, Clone, Debug)]
pub enum BufferFormat {
    Undefined = _NV_ENC_BUFFER_FORMAT::NV_ENC_BUFFER_FORMAT_UNDEFINED,
    NV12 = _NV_ENC_BUFFER_FORMAT::NV_ENC_BUFFER_FORMAT_NV12,
    YV12 = _NV_ENC_BUFFER_FORMAT::NV_ENC_BUFFER_FORMAT_YV12,
    IYUV = _NV_ENC_BUFFER_FORMAT::NV_ENC_BUFFER_FORMAT_IYUV,
    YUV444 = _NV_ENC_BUFFER_FORMAT::NV_ENC_BUFFER_FORMAT_YUV444,
    YUV444_10Bit = _NV_ENC_BUFFER_FORMAT::NV_ENC_BUFFER_FORMAT_YUV444_10BIT,
    YUV420_10Bit = _NV_ENC_BUFFER_FORMAT::NV_ENC_BUFFER_FORMAT_YUV420_10BIT,
    ARGB = _NV_ENC_BUFFER_FORMAT::NV_ENC_BUFFER_FORMAT_ARGB,
    ARGB10 = _NV_ENC_BUFFER_FORMAT::NV_ENC_BUFFER_FORMAT_ARGB10,
    ABGR = _NV_ENC_BUFFER_FORMAT::NV_ENC_BUFFER_FORMAT_ABGR,
    AYUV = _NV_ENC_BUFFER_FORMAT::NV_ENC_BUFFER_FORMAT_AYUV,
    ABGR10 = _NV_ENC_BUFFER_FORMAT::NV_ENC_BUFFER_FORMAT_ABGR10,
    // U8 = _NV_ENC_BUFFER_FORMAT::NV_ENC_BUFFER_FORMAT_U8,
}

impl BufferFormat {
    fn calculate_size(&self, stride: u32, height: u32) -> Result<usize, NvencError> {
        match self {
            &BufferFormat::NV12 | &BufferFormat::YV12 | &BufferFormat::IYUV => {
                Ok((stride as usize) * (height as usize) * 3 / 2)
            }
            &BufferFormat::ARGB => Ok((stride as usize) * (height as usize) * 4),
            _ => Err(NvencError::UnableToComputeSize {
                #[cfg(feature = "backtrace")]
                backtrace: Backtrace::capture(),
            }),
        }
    }
}

/// Parameters used to initialize the encoder
pub struct InitParams {
    init_params: NV_ENC_INITIALIZE_PARAMS,
    encode_config: EncodeConfig,
}

impl Debug for InitParams {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        let p = &self.init_params;
        write!(
            f,
            "InitParams{{version: {}, encodeGUID: {}, encodeWidth: {}, encodeHeight: {}, darWidth: {}, darHeight: {}, enablePTD: {}, presetGUID: {}, encodeConfig: {:?}, frameRateNum: {}, frameRateDen: {}, maxEncodeWidth: {}, maxEncodeHeight: {} }}",
            p.version,
            guid_string(&p.encodeGUID),
            p.encodeWidth,
            p.encodeHeight,
            p.darWidth,
            p.darHeight,
            p.enablePTD,
            guid_string(&p.presetGUID),
            self.encode_config,
            p.frameRateNum,
            p.frameRateDen,
            p.maxEncodeWidth,
            p.maxEncodeHeight,
        )
    }
}

fn guid_string(guid: &GUID) -> String {
    format!(
        "{{ 0x{:x}, 0x{:x}, 0x{:x}, {} }}",
        guid.Data1,
        guid.Data2,
        guid.Data3,
        arr_string(&guid.Data4)
    )
}

fn arr_string(arr: &[u8; 8]) -> String {
    format!(
        "{{ 0x{:x}, 0x{:x}, 0x{:x}, 0x{:x}, 0x{:x}, 0x{:x}, 0x{:x}, 0x{:x} }}",
        arr[0], arr[1], arr[2], arr[3], arr[4], arr[5], arr[6], arr[7]
    )
}

pub struct InitParamsBuilder {
    init_params: std::mem::MaybeUninit<NV_ENC_INITIALIZE_PARAMS>,
    encode_config: Option<EncodeConfig>,
}

impl InitParamsBuilder {
    pub fn new(encode: GUID, width: u32, height: u32) -> Self {
        let mut uninit = std::mem::MaybeUninit::<NV_ENC_INITIALIZE_PARAMS>::zeroed();

        let ptr = uninit.as_mut_ptr();

        unsafe {
            addr_of_mut!((*ptr).version).write(NV_ENC_INITIALIZE_PARAMS_VER);
            addr_of_mut!((*ptr).encodeGUID).write(encode);
            addr_of_mut!((*ptr).encodeWidth).write(width);
            addr_of_mut!((*ptr).encodeHeight).write(height);
            addr_of_mut!((*ptr).darWidth).write(width);
            addr_of_mut!((*ptr).darHeight).write(height);
            addr_of_mut!((*ptr).enablePTD).write(1);
            addr_of_mut!((*ptr).maxEncodeWidth).write(width);
            addr_of_mut!((*ptr).maxEncodeHeight).write(height);
        }
        Self {
            init_params: uninit,
            encode_config: None,
        }
    }

    // display aspect ratio width
    pub fn dar_width(mut self, width: u32) -> Self {
        let ptr = self.init_params.as_mut_ptr();
        unsafe {
            addr_of_mut!((*ptr).darWidth).write(width);
        }
        self
    }

    // display aspect ratio height
    pub fn dar_height(mut self, height: u32) -> Self {
        let ptr = self.init_params.as_mut_ptr();
        unsafe {
            addr_of_mut!((*ptr).darHeight).write(height);
        }
        self
    }

    pub fn preset_guid(mut self, preset: GUID) -> Self {
        let ptr = self.init_params.as_mut_ptr();
        unsafe {
            addr_of_mut!((*ptr).presetGUID).write(preset);
        }
        self
    }

    pub fn set_encode_config(mut self, config: EncodeConfig) -> Self {
        self.encode_config = Some(config);
        // We will set the `(*ptr).encodeConfig` when `Self::build` is called.
        self
    }

    /// Set the frame rate (numerator and denominator)
    ///
    /// Note: "The frame rate has no meaning in NVENC other than deciding rate
    /// control parameters." <https://devtalk.nvidia.com/default/topic/1023473>
    pub fn set_framerate(mut self, num: u32, den: u32) -> Self {
        let ptr = self.init_params.as_mut_ptr();
        unsafe {
            addr_of_mut!((*ptr).frameRateNum).write(num);
            addr_of_mut!((*ptr).frameRateDen).write(den);
        }
        self
    }

    pub fn build(self) -> Result<Pin<Box<InitParams>>, NvencError> {
        let encode_config = match self.encode_config {
            Some(c) => c,
            None => {
                return Err(NvencError::EncodeConfigRequired {
                    #[cfg(feature = "backtrace")]
                    backtrace: Backtrace::capture(),
                });
            }
        };
        let params = InitParams {
            init_params: unsafe { self.init_params.assume_init() },
            encode_config,
        };
        let mut boxed = Box::pin(params);

        let ptr: *mut NV_ENC_CONFIG = &mut boxed.encode_config.config;
        // we know this is safe because modifying a field doesn't move the whole struct
        unsafe {
            let mut_ref: Pin<&mut InitParams> = Pin::as_mut(&mut boxed);
            Pin::get_unchecked_mut(mut_ref).init_params.encodeConfig = ptr;
        }
        Ok(boxed)
    }
}

/// Encoder configuration for a encode session
pub struct EncodeConfig {
    config: NV_ENC_CONFIG,
}

impl Debug for EncodeConfig {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(f, "{{rcParams.rateControlMode: {}, rcParams.averageBitRate: {}, rcParams.maxBitRate: {} }}",
        self.config.rcParams.rateControlMode,
        self.config.rcParams.averageBitRate,
        self.config.rcParams.maxBitRate,
    )
    }
}

impl EncodeConfig {
    pub fn set_rate_control_mode(&mut self, mode: RateControlMode) {
        self.config.rcParams.rateControlMode = mode.to_c();
    }
    pub fn set_average_bit_rate(&mut self, value: u32) {
        self.config.rcParams.averageBitRate = value;
    }
    pub fn set_max_bit_rate(&mut self, value: u32) {
        self.config.rcParams.maxBitRate = value;
    }
}

#[derive(Clone, Copy, Debug)]
pub enum RateControlMode {
    /// Constant QP mode
    Constqp,
    /// Variable bitrate mode
    Vbr,
    /// Constant bitrate mode
    Cbr,
    /// low-delay CBR, high quality
    LowdelayHq,
    /// CBR, high quality (slower)
    CbrHq,
    /// VBR, high quality (slower)
    VbrHq,
}

impl RateControlMode {
    fn to_c(self) -> NvInt {
        use RateControlMode::*;
        match self {
            Constqp => _NV_ENC_PARAMS_RC_MODE::NV_ENC_PARAMS_RC_CONSTQP,
            Vbr => _NV_ENC_PARAMS_RC_MODE::NV_ENC_PARAMS_RC_VBR,
            Cbr => _NV_ENC_PARAMS_RC_MODE::NV_ENC_PARAMS_RC_CBR,
            LowdelayHq => _NV_ENC_PARAMS_RC_MODE::NV_ENC_PARAMS_RC_CBR_LOWDELAY_HQ,
            CbrHq => _NV_ENC_PARAMS_RC_MODE::NV_ENC_PARAMS_RC_CBR_HQ,
            VbrHq => _NV_ENC_PARAMS_RC_MODE::NV_ENC_PARAMS_RC_VBR_HQ,
        }
    }
}
