#[cfg(feature = "backtrace")]
use std::backtrace::Backtrace;

#[cfg(feature = "do_not_use_ipp")]
use fastfreeimage as fastimage;

pub type Result<M> = std::result::Result<M, Error>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("divide by zero")]
    DivideByZero(#[cfg(feature = "backtrace")] Backtrace),
    #[error("image size changed")]
    ImageSizeChanged(#[cfg(feature = "backtrace")] Backtrace),
    #[error("IncompleteSend")]
    IncompleteSend(#[cfg(feature = "backtrace")] Backtrace),
    #[error("ExpectedObject")]
    ExpectedObject(#[cfg(feature = "backtrace")] Backtrace),
    #[error("ExpectedNull")]
    ExpectedNull(#[cfg(feature = "backtrace")] Backtrace),
    #[error("ExpectedBool")]
    ExpectedBool(#[cfg(feature = "backtrace")] Backtrace),
    #[error("FlydraTypeError")]
    FlydraTypeError(#[cfg(feature = "backtrace")] Backtrace),
    #[error("MainbrainQuit")]
    MainbrainQuit(#[cfg(feature = "backtrace")] Backtrace),
    #[error("unix domain sockets not supported")]
    UnixDomainSocketsNotSupported(#[cfg(feature = "backtrace")] Backtrace),
    #[error("conversion to socket address failed")]
    SocketAddressConversionFailed(#[cfg(feature = "backtrace")] Backtrace),

    #[error("CastError({})", _0)]
    CastError(#[from] cast::Error),
    #[error("UFMFError({})", _0)]
    UFMFError(#[from] ufmf::UFMFError),
    #[error("other error: {msg}")]
    OtherError {
        msg: String,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },

    #[error("FastImageError({0})")]
    FastImageError(#[from] fastimage::Error),
    #[error("{0}")]
    FlydraTypesError(#[from] flydra_types::FlydraTypesError),
    #[error("IoError: {source}")]
    IoError {
        #[from]
        source: std::io::Error,
        #[cfg(feature = "backtrace")]
        backtrace: std::backtrace::Backtrace,
    },
    #[error("TryRecvError")]
    TryRecvError,
    #[error("RecvTimeoutError: {source}")]
    RecvTimeoutError {
        #[from]
        source: std::sync::mpsc::RecvTimeoutError,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("ParseIntError: {source}")]
    ParseIntError {
        #[from]
        source: std::num::ParseIntError,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("{0}")]
    FuturesSendError(#[from] futures::channel::mpsc::SendError),
}
