use thiserror::Error;

#[derive(Error, Debug)]
pub enum NvEncError {
    #[error("dynlink-cuda returned error `{0}`")]
    DynlinkCudaError(dynlink_cuda::CudaError),
    #[error("dynlink-nvidia-encode returned error `{0}`")]
    DynlinkNvidiaEncodeError(dynlink_nvidia_encode::NvencError),
}

impl From<dynlink_cuda::CudaError> for NvEncError {
    fn from(orig: dynlink_cuda::CudaError) -> NvEncError {
        NvEncError::DynlinkCudaError(orig)
    }
}

impl From<dynlink_nvidia_encode::NvencError> for NvEncError {
    fn from(orig: dynlink_nvidia_encode::NvencError) -> NvEncError {
        NvEncError::DynlinkNvidiaEncodeError(orig)
    }
}
