#[cfg(feature = "backtrace")]
use std::backtrace::Backtrace;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("{source}")]
    FlydraTypes {
        #[from]
        source: flydra_types::FlydraTypesError,
    },
    #[error("{source}")]
    Mvg {
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        source: mvg::MvgError,
    },
    #[error("{source}")]
    Io {
        #[from]
        source: std::io::Error,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("{source}")]
    Csv {
        #[from]
        source: csv::Error,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("{source}")]
    GetTimezone {
        #[from]
        source: iana_time_zone::GetTimezoneError,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("{source}")]
    SerdeJson {
        #[from]
        source: serde_json::Error,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("{source}")]
    SerdeYaml {
        #[from]
        source: serde_yaml::Error,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("{source}")]
    HyperError {
        #[from]
        source: hyper::Error,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("{source}")]
    TomlSerError {
        #[from]
        source: toml::ser::Error,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("{source}")]
    TomlDeError {
        #[from]
        source: toml::de::Error,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("{source}")]
    SendToDiskError {
        #[from]
        source: tokio::sync::mpsc::error::SendError<crate::SaveToDiskMsg>,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("invalid hypothesis testing parameters")]
    InvalidHypothesisTestingParameters,
    #[error("insufficient data to calculate FPS")]
    InsufficientDataToCalculateFps,
    #[error(transparent)]
    FileError(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        FileErrorInner,
    ),
    #[error(transparent)]
    WrappedError(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        WrappedErrorInner,
    ),
}

#[derive(Debug)]
pub struct FileErrorInner {
    what: &'static str,
    filename: String,
    source: Box<dyn std::error::Error + Sync + Send>,
}

impl std::fmt::Display for FileErrorInner {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "Error \"{}\" opening {}: {}",
            self.what, self.filename, self.source
        )
    }
}

impl std::error::Error for FileErrorInner {
    #[cfg(feature = "backtrace")]
    fn provide<'a>(&'a self, req: &mut std::error::Request<'a>) {
        self.source.provide(req)
    }
}

#[derive(Debug)]
pub struct WrappedErrorInner {
    source: Box<dyn std::error::Error + Sync + Send>, // Box::new(source),
}

impl std::fmt::Display for WrappedErrorInner {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.source)
    }
}

impl std::error::Error for WrappedErrorInner {
    #[cfg(feature = "backtrace")]
    fn provide<'a>(&'a self, req: &mut std::error::Request<'a>) {
        self.source.provide(req)
    }
}

pub fn file_error<E>(what: &'static str, filename: String, source: E) -> Error
where
    E: 'static + std::error::Error + Sync + Send,
{
    FileErrorInner {
        what,
        filename,
        source: Box::new(source),
    }
    .into()
}

pub fn wrap_error<E>(source: E) -> Error
where
    E: 'static + std::error::Error + Sync + Send,
{
    WrappedErrorInner {
        source: Box::new(source),
    }
    .into()
}
