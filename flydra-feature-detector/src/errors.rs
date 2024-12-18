use crate::fastim_mod;

pub type Result<M> = std::result::Result<M, Error>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("divide by zero")]
    DivideByZero,
    #[error("image size changed")]
    ImageSizeChanged,
    #[error("BackgroundProcessingThreadDisconnected")]
    BackgroundProcessingThreadDisconnected,

    #[error("CastError({})", _0)]
    CastError(#[from] cast::Error),
    #[error("UFMFError({})", _0)]
    UFMFError(#[from] ufmf::UFMFError),
    #[error("other error: {msg}")]
    OtherError { msg: String },

    #[error("FastImageError({0})")]
    FastImageError(#[from] fastim_mod::Error),
    #[error("IoError: {source}")]
    IoError {
        #[from]
        source: std::io::Error,
    },
}
