use crate::*;

pub type Result<M> = std::result::Result<M, Error>;

#[derive(Debug)]
pub struct Error {
    inner: Context<ErrorKind>,
}

impl Fail for Error {
    fn cause(&self) -> Option<&dyn Fail> {
        self.inner.cause()
    }

    fn backtrace(&self) -> Option<&Backtrace> {
        self.inner.backtrace()
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.inner, f)
    }
}

#[derive(Debug)]
pub struct WrappedRosRustContext {}

impl std::fmt::Display for WrappedRosRustContext {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        std::fmt::Display::fmt("WrappedRosRustContext", f)
    }
}

#[derive(Fail, Debug)]
pub enum ErrorKind {
    #[fail(display = "divide by zero")]
    DivideByZero,
    #[fail(display = "image size changed")]
    ImageSizeChanged,
    #[fail(display = "IncompleteSend")]
    IncompleteSend,
    #[fail(display = "ExpectedObject")]
    ExpectedObject,
    #[fail(display = "ExpectedNull")]
    ExpectedNull,
    #[fail(display = "ExpectedBool")]
    ExpectedBool,
    #[fail(display = "FlydraTypeError")]
    FlydraTypeError,
    #[fail(display = "MainbrainQuit")]
    MainbrainQuit,
    #[fail(display = "unix domain sockets not supported")]
    UnixDomainSocketsNotSupported,

    #[fail(display = "RosRustError: {}", _0)]
    RosRustError(WrappedRosRustContext), // string context, original error

    // TODO: remove state from all these ErrorKind variants
    // and put it in the context of the Error.
    #[fail(display = "ParseYAMLError({})", _0)]
    ParseYAMLError(serde_yaml::Error),
    #[fail(display = "ParseCBORError({})", _0)]
    ParseCBORError(serde_cbor::error::Error),
    #[fail(display = "CastError({})", _0)]
    CastError(cast::Error),
    #[fail(display = "UFMFError({})", _0)]
    UFMFError(ufmf::UFMFError),
    #[fail(display = "SendError({})", _0)]
    SendError(String),
    #[fail(display = "other error: {}", _0)]
    OtherError(String),

    #[fail(display = "FastImageError({})", _0)]
    FastImageError(#[cause] fastimage::Error),
    #[fail(display = "{}", _0)]
    FlydraTypesError(#[cause] flydra_types::FlydraTypesError),

    #[fail(display = "{}", _0)]
    IoError(#[cause] std::io::Error),
    #[fail(display = "{}", _0)]
    JsonError(#[cause] serde_json::Error),
    #[fail(display = "TryRecvError")]
    TryRecvError,
    #[fail(display = "{}", _0)]
    RecvTimeoutError(#[cause] std::sync::mpsc::RecvTimeoutError),
    #[fail(display = "{}", _0)]
    ParseIntError(#[cause] std::num::ParseIntError),
    #[fail(display = "{}", _0)]
    HyperError(#[cause] hyper::Error),
}

impl Error {
    pub fn kind(&self) -> &ErrorKind {
        self.inner.get_context()
    }
}

impl From<ErrorKind> for Error {
    fn from(kind: ErrorKind) -> Error {
        Error {
            inner: Context::new(kind),
        }
    }
}

impl From<Context<ErrorKind>> for Error {
    fn from(inner: Context<ErrorKind>) -> Error {
        Error { inner: inner }
    }
}

impl From<serde_yaml::Error> for Error {
    fn from(orig: serde_yaml::Error) -> Error {
        Error {
            inner: Context::new(ErrorKind::ParseYAMLError(orig)),
        }
    }
}

impl From<serde_cbor::error::Error> for Error {
    fn from(orig: serde_cbor::error::Error) -> Error {
        Error {
            inner: Context::new(ErrorKind::ParseCBORError(orig)),
        }
    }
}

impl From<cast::Error> for Error {
    fn from(orig: cast::Error) -> Error {
        Error {
            inner: Context::new(ErrorKind::CastError(orig)),
        }
    }
}

impl From<ufmf::UFMFError> for Error {
    fn from(orig: ufmf::UFMFError) -> Error {
        Error {
            inner: Context::new(ErrorKind::UFMFError(orig)),
        }
    }
}

impl From<fastimage::Error> for Error {
    fn from(orig: fastimage::Error) -> Error {
        Error {
            inner: Context::new(ErrorKind::FastImageError(orig)),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(orig: std::io::Error) -> Error {
        Error {
            inner: Context::new(ErrorKind::IoError(orig)),
        }
    }
}

impl From<flydra_types::FlydraTypesError> for Error {
    fn from(orig: flydra_types::FlydraTypesError) -> Error {
        Error {
            inner: Context::new(ErrorKind::FlydraTypesError(orig)),
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(orig: serde_json::Error) -> Error {
        Error {
            inner: Context::new(ErrorKind::JsonError(orig)),
        }
    }
}

impl From<std::sync::mpsc::RecvTimeoutError> for Error {
    fn from(orig: std::sync::mpsc::RecvTimeoutError) -> Error {
        Error {
            inner: Context::new(ErrorKind::RecvTimeoutError(orig)),
        }
    }
}

impl From<std::num::ParseIntError> for Error {
    fn from(orig: std::num::ParseIntError) -> Error {
        Error {
            inner: Context::new(ErrorKind::ParseIntError(orig)),
        }
    }
}

impl From<hyper::Error> for Error {
    fn from(orig: hyper::Error) -> Error {
        Error {
            inner: Context::new(ErrorKind::HyperError(orig)),
        }
    }
}

impl<T> From<std::sync::mpsc::SendError<T>> for Error {
    fn from(orig: std::sync::mpsc::SendError<T>) -> Error {
        Error {
            inner: Context::new(ErrorKind::SendError(format!("{:?}", orig))),
        }
    }
}
