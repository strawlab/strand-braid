use thiserror::Error;

#[derive(Error, Debug)]
pub enum CudaError {
    #[error("dynamic library `{0}` could not be loaded: `{1}`")]
    DynLibLoadError(String, std::io::Error),
    #[error("CUDA returned code `{0}`")]
    ErrCode(i32),
    #[error("Name `{0}` could not be opened: `{1}`")]
    NameFFIError(String, std::io::Error),
}
