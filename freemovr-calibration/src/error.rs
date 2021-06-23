#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("...")]
    Mvg(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        mvg::MvgError,
    ),
    #[error("...")]
    IoError {
        #[from]
        source: std::io::Error,
        #[cfg(feature = "backtrace")]
        backtrace: std::backtrace::Backtrace,
    },
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
    #[error("...")]
    Other(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        anyhow::Error,
    ),
}
