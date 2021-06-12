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

#[cfg(test)]
mod test {
    #[cfg(feature = "backtrace")]
    use std::{backtrace::Backtrace, error::Error};

    #[test]
    fn test_from_dynlink_cuda_error() {
        let orig = dynlink_cuda::CudaError::ErrCode {
            status: 2,
            #[cfg(feature = "backtrace")]
            backtrace: Backtrace::capture(),
        };
        #[allow(unused_variables)]
        let converted = crate::NvEncError::from(orig);
        #[cfg(feature = "backtrace")]
        assert!(converted.backtrace().is_some());
    }

    #[test]
    fn test_from_dynlink_nvidia_encode_error() {
        let orig = dynlink_nvidia_encode::NvencError::ErrCode {
            status: 2,
            message: "error",
            #[cfg(feature = "backtrace")]
            backtrace: Backtrace::capture(),
        };
        #[allow(unused_variables)]
        let converted = crate::NvEncError::from(orig);
        #[cfg(feature = "backtrace")]
        assert!(converted.backtrace().is_some());
    }
}
