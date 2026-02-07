use std::rc::Rc;

mod error;

pub use error::NvEncError;

pub use dynlink_cuda::api::CudaDevice;
pub use dynlink_nvidia_encode::{
    api::{
        ApiVersion, BufferFormat, Encoder, InitParamsBuilder, InputBuffer, LibNvEncode,
        OutputBuffer, RateControlMode,
    },
    guids::*,
    Queue, NV_ENC_CODEC_H264_GUID, NV_ENC_PRESET_HP_GUID,
};

pub struct NvEnc<'lib> {
    pub libcuda: dynlink_cuda::api::LibCuda<'lib>,
    pub libnvenc: Rc<LibNvEncode<'lib>>,
    pub functions: dynlink_nvidia_encode::api::NvEncodeApiFunctionList<'lib>,
}

impl<'lib> NvEnc<'lib> {
    pub fn new(libs: &'lib Dynlibs) -> Result<NvEnc<'lib>, NvEncError> {
        let libcuda = dynlink_cuda::api::init(&libs.cuda_shlib)?;
        libcuda.init(0)?;

        let libnvenc = dynlink_nvidia_encode::api::init(&libs.nvenc_shlib)?;
        let functions = LibNvEncode::api_create_instance(libnvenc.clone())?;
        Ok(NvEnc {
            libcuda,
            libnvenc,
            functions,
        })
    }
    pub fn cuda_version(&self) -> Result<i32, NvEncError> {
        Ok(self.libcuda.driver_get_version()?)
    }
    pub fn cuda_device_count(&self) -> Result<i32, NvEncError> {
        Ok(self.libcuda.device_get_count()?)
    }
    pub fn new_cuda_device(&self, idx: i32) -> Result<dynlink_cuda::CudaDevice<'_>, NvEncError> {
        Ok(self.libcuda.new_device(idx)?)
    }
}

pub struct Dynlibs {
    pub cuda_shlib: dynlink_cuda::load::SharedLibrary,
    pub nvenc_shlib: dynlink_nvidia_encode::load::SharedLibrary,
}

impl Dynlibs {
    pub fn new() -> Result<Self, NvEncError> {
        let cuda_shlib = dynlink_cuda::load::load()?;
        let nvenc_shlib = dynlink_nvidia_encode::load::load()?;
        Ok(Self {
            cuda_shlib,
            nvenc_shlib,
        })
    }
}

#[ignore = "requires NVENC shared libraries to be present at runtime"]
#[test]
fn test_basics() {
    check_basics().unwrap();
}

#[cfg(test)]
fn check_basics() -> Result<(), NvEncError> {
    let libs = Dynlibs::new()?;
    let _nvenc = NvEnc::new(&libs)?;
    Ok(())
}
