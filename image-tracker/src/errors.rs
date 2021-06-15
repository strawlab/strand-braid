pub type Result<M> = std::result::Result<M, Error>;

#[derive(Debug)]
pub struct WrappedRosRustContext {}

impl std::fmt::Display for WrappedRosRustContext {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        std::fmt::Display::fmt("WrappedRosRustContext", f)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("divide by zero")]
    DivideByZero,
    #[error("image size changed")]
    ImageSizeChanged,
    #[error("IncompleteSend")]
    IncompleteSend,
    #[error("ExpectedObject")]
    ExpectedObject,
    #[error("ExpectedNull")]
    ExpectedNull,
    #[error("ExpectedBool")]
    ExpectedBool,
    #[error("FlydraTypeError")]
    FlydraTypeError,
    #[error("MainbrainQuit")]
    MainbrainQuit,
    #[error("unix domain sockets not supported")]
    UnixDomainSocketsNotSupported,

    #[error("RosRustError: {0}")]
    RosRustError(WrappedRosRustContext), // string context, original error

    // TODO: remove state from all these ErrorKind variants
    // and put it in the context of the Error.
    #[error("ParseYAMLError({})", _0)]
    ParseYAMLError(serde_yaml::Error),
    #[error("ParseCBORError({})", _0)]
    ParseCBORError(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        serde_cbor::error::Error,
    ),
    #[error("CastError({})", _0)]
    CastError(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        cast::Error,
    ),
    #[error("UFMFError({})", _0)]
    UFMFError(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        ufmf::UFMFError,
    ),
    #[error("SendError({})", _0)]
    SendError(String),
    #[error("other error: {0}")]
    OtherError(String),

    #[error("FastImageError({0})")]
    FastImageError(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        fastimage::Error,
    ),
    #[error("{0}")]
    FlydraTypesError(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        flydra_types::FlydraTypesError,
    ),

    #[error("{0}")]
    IoError(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        std::io::Error,
    ),
    #[error("{0}")]
    JsonError(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        serde_json::Error,
    ),
    #[error("TryRecvError")]
    TryRecvError,
    #[error("{0}")]
    RecvTimeoutError(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        std::sync::mpsc::RecvTimeoutError,
    ),
    #[error("{0}")]
    ParseIntError(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        std::num::ParseIntError,
    ),
    #[error("{0}")]
    HyperError(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        hyper::Error,
    ),
}
