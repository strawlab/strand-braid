use ::std;

pub struct String {
    inner: Vec<u8>,
}

#[derive(Debug, Clone)]
pub enum Error {
    NotAscii,
}

impl std::error::Error for Error {}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        match self {
            Error::NotAscii => write!(f, "ascii::Error::NotAscii"),
        }
    }
}

impl String {
    pub fn empty() -> Self {
        Self { inner: vec![] }
    }
    pub fn from_vec(input: Vec<u8>) -> Result<Self, Error> {
        for c in input.iter() {
            if !c.is_ascii() {
                return Err(Error::NotAscii);
            }
        }
        Ok(Self { inner: input })
    }
}

impl std::fmt::Display for String {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        let s = unsafe { std::str::from_utf8_unchecked(&self.inner) };
        write!(f, "{}", s)
    }
}
