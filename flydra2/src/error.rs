#[cfg(feature = "backtrace")]
use std::backtrace::Backtrace;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("{source}")]
    FlydraTypes {
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
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
        #[cfg_attr(feature = "backtrace", backtrace)]
        source: std::io::Error,
    },
    #[error("{source}")]
    Csv {
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        source: csv::Error,
    },
    #[error("{source}")]
    GetTimezone {
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        source: iana_time_zone::GetTimezoneError,
    },
    #[error("{source}")]
    SerdeJson {
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        source: serde_json::Error,
    },
    #[error("{source}")]
    SerdeYaml {
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        source: serde_yaml::Error,
    },
    #[error("{source}")]
    TomlSerError {
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        source: toml::ser::Error,
    },
    #[error("{source}")]
    TomlDeError {
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        source: toml::de::Error,
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
        write!(f, "Error opening {}: {}", self.filename, self.source)
    }
}

impl std::error::Error for FileErrorInner {
    fn backtrace(&self) -> Option<&Backtrace> {
        self.source.backtrace()
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
    fn backtrace(&self) -> Option<&Backtrace> {
        self.source.backtrace()
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
