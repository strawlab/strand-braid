
#[derive(Debug)]
pub enum ErrorKind {
    IoError(std::io::Error),
    ObjHasNoTextureCoords,
    ObjError(obj::ObjError),
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

impl From<std::io::Error> for Error {
    fn from(orig: std::io::Error) -> Error {
        Error { kind: ErrorKind::IoError(orig)}
    }
}

impl From<obj::ObjError> for Error {
    fn from(orig: obj::ObjError) -> Error {
        Error { kind: ErrorKind::ObjError(orig)}
    }
}

impl std::error::Error for Error {
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self.kind)
    }
}
