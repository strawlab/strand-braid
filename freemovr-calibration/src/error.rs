#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("...")]
    Mvg(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        mvg::MvgError,
    ),
    #[error("...")]
    IoError(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        std::io::Error,
    ),
    #[error("...")]
    FailedParse1(serde_yaml::Error),
    #[error("...")]
    FailedParse((serde_yaml::Error, serde_yaml::Error)),
    #[error("...")]
    ObjHasNoTextureCoords,
    #[error("...")]
    InvalidTexCoord,
    #[error("...")]
    SerdeYaml(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        serde_yaml::Error,
    ),
    #[error("...")]
    SerdeJson(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        serde_json::Error,
    ),
    #[cfg(feature = "opencv")]
    #[error("...")]
    OpenCvCalibrate(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        opencv_calibrate::Error,
    ),
    #[error("...")]
    Other(failure::Error),
    #[error("...")]
    OtherBox(Box<dyn failure::Fail>),
    #[error("...")]
    RequiredTriMesh,
    #[error("...")]
    InvalidTriMesh,
    #[error("...")]
    VirtualDisplayNotFound,
    #[error("...")]
    DisplaySizeNotFound,
    #[error("...")]
    SimpleObjParse(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        simple_obj_parse::Error,
    ),
    #[error("...")]
    ObjMustHaveExactlyOneObject(usize),
    #[error("...")]
    Csv(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        csv::Error,
    ),
    #[error("...")]
    SvdError(&'static str),
}

impl From<failure::Error> for Error {
    fn from(orig: failure::Error) -> Error {
        Error::Other(orig)
    }
}

impl<C: std::fmt::Display + Send + Sync + 'static> From<failure::Context<C>> for Error {
    fn from(orig: failure::Context<C>) -> Error {
        Error::OtherBox(Box::new(orig))
    }
}
