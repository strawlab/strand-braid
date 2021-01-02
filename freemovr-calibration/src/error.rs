
#[derive(Debug)]
pub enum ErrorKind {
    Mvg(mvg::MvgError),
    IoError(std::io::Error),
    FailedParse1(serde_yaml::Error),
    FailedParse((serde_yaml::Error,serde_yaml::Error)),
    ObjHasNoTextureCoords,
    InvalidTexCoord,
    SerdeYaml(serde_yaml::Error),
    SerdeJson(serde_json::Error),
    #[cfg(feature="opencv")]
    OpenCvCalibrate(opencv_calibrate::Error),
    Other(failure::Error),
    OtherBox(Box<dyn failure::Fail>),
    RequiredTriMesh,
    InvalidTriMesh,
    VirtualDisplayNotFound,
    DisplaySizeNotFound,
    SimpleObjParse(simple_obj_parse::Error),
    ObjMustHaveExactlyOneObject(usize),
    Csv(csv::Error),
    SvdError(&'static str),
}

#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
}

impl Error {
    pub fn new(kind: ErrorKind) -> Self {
        Self { kind }
    }
}

impl From<mvg::MvgError> for Error {
    fn from(orig: mvg::MvgError) -> Error {
        Error { kind: ErrorKind::Mvg(orig)}
    }
}

impl From<std::io::Error> for Error {
    fn from(orig: std::io::Error) -> Error {
        Error { kind: ErrorKind::IoError(orig)}
    }
}

impl From<serde_yaml::Error> for Error {
    fn from(orig: serde_yaml::Error) -> Error {
        Error { kind: ErrorKind::SerdeYaml(orig)}
    }
}

impl From<serde_json::Error> for Error {
    fn from(orig: serde_json::Error) -> Error {
        Error { kind: ErrorKind::SerdeJson(orig)}
    }
}

#[cfg(feature="opencv")]
impl From<opencv_calibrate::Error> for Error {
    fn from(orig: opencv_calibrate::Error) -> Error {
        Error { kind: ErrorKind::OpenCvCalibrate(orig)}
    }
}

impl From<failure::Error> for Error {
    fn from(orig: failure::Error) -> Error {
        Error { kind: ErrorKind::Other(orig)}
    }
}

impl From<simple_obj_parse::Error> for Error {
    fn from(orig: simple_obj_parse::Error) -> Error {
        Error { kind: ErrorKind::SimpleObjParse(orig)}
    }
}

impl From<csv::Error> for Error {
    fn from(orig: csv::Error) -> Error {
        Error { kind: ErrorKind::Csv(orig)}
    }
}

impl<C: std::fmt::Display + Send + Sync + 'static> From<failure::Context<C>> for Error {
    fn from(orig: failure::Context<C>) -> Error {
        Error { kind: ErrorKind::OtherBox(Box::new(orig))}
    }
}

impl std::error::Error for Error {
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self.kind)
    }
}
