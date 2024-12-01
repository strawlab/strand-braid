use thiserror::Error;

#[derive(Error, Debug)]
pub enum NvEncError {
    #[error("dynlink-cuda error`")]
    DynlinkCudaError(#[from] dynlink_cuda::CudaError),
    #[error("dynlink-nvidia-encode error")]
    DynlinkNvidiaEncodeError(#[from] dynlink_nvidia_encode::NvencError),
}

#[cfg(test)]
mod test {
    #[test]
    fn test_from_dynlink_cuda_error() {
        let orig = dynlink_cuda::CudaError::ErrCode { status: 2 };
        #[allow(unused_variables)]
        let converted = crate::NvEncError::from(orig);
    }

    #[test]
    fn test_from_dynlink_nvidia_encode_error() {
        let status = 2;
        let orig = dynlink_nvidia_encode::NvencError::ErrCode {
            status,
            line_num: line!(),
            fname: file!(),
            message: dynlink_nvidia_encode::error::code_to_string(status),
        };
        #[allow(unused_variables)]
        let converted = crate::NvEncError::from(orig);
    }
}
