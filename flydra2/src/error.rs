#[cfg(feature = "backtrace")]
use std::backtrace::Backtrace;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("{source}")]
    FlydraTypes {
        #[from]
        source: flydra_types::FlydraTypesError,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("{source}")]
    Mvg {
        #[from]
        source: mvg::MvgError,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
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
    FuturesSendError {
        #[from]
        source: futures::channel::mpsc::SendError,
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
    #[error("invalid hypothesis testing parameters")]
    InvalidHypothesisTestingParameters,
    #[error("insufficient data to calculate FPS")]
    InsufficientDataToCalculateFps,
    #[error("{source}")]
    ZipDir {
        #[from]
        source: zip_or_dir::Error,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("Error opening {filename}: {source}")]
    FileError {
        what: &'static str,
        filename: String,
        source: Box<dyn std::error::Error + Sync + Send>,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("{source}")]
    WrappedError {
        source: Box<dyn std::error::Error + Sync + Send>,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("output filename must end with '.braidz'")]
    OutputFilenameMustEndInBraidz {
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
}

pub fn file_error<E>(what: &'static str, filename: String, source: E) -> Error
where
    E: 'static + std::error::Error + Sync + Send,
{
    Error::FileError {
        what,
        filename,
        source: Box::new(source),
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace::capture(),
    }
}

pub fn wrap_error<E>(source: E) -> Error
where
    E: 'static + std::error::Error + Sync + Send,
{
    Error::WrappedError {
        source: Box::new(source),
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace::capture(),
    }
}
