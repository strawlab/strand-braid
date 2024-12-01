#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("mvg error: {0}")]
    Mvg(#[from] mvg::MvgError),
    #[error("{source}")]
    IoError {
        #[from]
        source: std::io::Error,
    },
    #[error("failed parse 1: {0}")]
    FailedParse1(serde_yaml::Error),
    #[error("failed parse: {err1}, {err2}")]
    FailedParse {
        err1: serde_yaml::Error,
        err2: serde_yaml::Error,
    },
    #[error("obj has no texture coords")]
    ObjHasNoTextureCoords,
    #[error("invalid tex coord")]
    InvalidTexCoord,
    #[error("{source}")]
    SerdeYaml {
        #[from]
        source: serde_yaml::Error,
    },
    #[error("{source}")]
    SerdeJson {
        #[from]
        source: serde_json::Error,
    },
    #[cfg(feature = "opencv")]
    #[error("{source}")]
    OpenCvCalibrate {
        #[from]
        source: opencv_calibrate::Error,
    },
    #[error("{source}")]
    ImageError {
        #[from]
        source: image::ImageError,
    },
    #[error("required tri mesh")]
    RequiredTriMesh,
    #[error("inavlid tri mesh")]
    InvalidTriMesh,
    #[error("virtual display not found")]
    VirtualDisplayNotFound,
    #[error("display size not found")]
    DisplaySizeNotFound,
    #[error("simple obj parse error: {0}")]
    SimpleObjParse(#[from] simple_obj_parse::Error),
    #[error("must have exactle one object")]
    ObjMustHaveExactlyOneObject(usize),
    #[error("csv error {0}")]
    Csv(#[from] csv::Error),
    #[error("svd error: {0}")]
    SvdError(&'static str),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
