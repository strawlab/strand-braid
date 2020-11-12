
#[derive(Debug)]
enum ErrorKind {
    OpenCvCalibrate(opencv_calibrate::Error),
    OpencvRosCamera(opencv_ros_camera::Error),
    Mvg(mvg::MvgError),
    Other(failure::Error),
}

#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
}

impl From<opencv_calibrate::Error> for Error {
    fn from(orig: opencv_calibrate::Error) -> Error {
        Error { kind: ErrorKind::OpenCvCalibrate(orig)}
    }
}

impl From<opencv_ros_camera::Error> for Error {
    fn from(orig: opencv_ros_camera::Error) -> Error {
        Error { kind: ErrorKind::OpencvRosCamera(orig)}
    }
}

impl From<mvg::MvgError> for Error {
    fn from(orig: mvg::MvgError) -> Error {
        Error { kind: ErrorKind::Mvg(orig)}
    }
}

impl From<failure::Error> for Error {
    fn from(orig: failure::Error) -> Error {
        Error { kind: ErrorKind::Other(orig)}
    }
}

impl std::error::Error for Error {
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self.kind)
    }
}
