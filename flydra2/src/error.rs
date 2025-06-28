#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("{source}")]
    FlydraTypes {
        #[from]
        source: braid_types::FlydraTypesError,
    },
    #[error("{source}")]
    Mvg {
        #[from]
        source: braid_mvg::MvgError,
    },
    #[error("{0}")]
    FlydraMvg(#[from] flydra_mvg::FlydraMvgError),
    #[error("{source}")]
    Io {
        #[from]
        source: std::io::Error,
    },
    #[error("{source}")]
    Csv {
        #[from]
        source: csv::Error,
    },
    #[error("{source}")]
    GetTimezone {
        #[from]
        source: iana_time_zone::GetTimezoneError,
    },
    #[error("{source}")]
    SerdeJson {
        #[from]
        source: serde_json::Error,
    },
    #[error("{source}")]
    SerdeYaml {
        #[from]
        source: serde_yaml::Error,
    },
    #[error("{source}")]
    TomlSerError {
        #[from]
        source: toml::ser::Error,
    },
    #[error("{source}")]
    TomlDeError {
        #[from]
        source: toml::de::Error,
    },
    #[error("{source}")]
    SendToDiskError {
        #[from]
        source: tokio::sync::mpsc::error::SendError<crate::SaveToDiskMsg>,
    },
    #[error("invalid hypothesis testing parameters")]
    InvalidHypothesisTestingParameters,
    #[error("insufficient data to calculate FPS")]
    InsufficientDataToCalculateFps,
    #[error(transparent)]
    FileError(#[from] FileErrorInner),
    #[error(transparent)]
    WrappedError(#[from] WrappedErrorInner),
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

impl std::error::Error for FileErrorInner {}

#[derive(Debug)]
pub struct WrappedErrorInner {
    source: Box<dyn std::error::Error + Sync + Send>, // Box::new(source),
}

impl std::fmt::Display for WrappedErrorInner {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.source)
    }
}

impl std::error::Error for WrappedErrorInner {}

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
