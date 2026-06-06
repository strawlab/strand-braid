// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::fmt::{self, Display};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    InvalidID,
    InvalidSize,
    BufSizeError(usize),
    Io(std::io::Error),
    Eof,
}

impl Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Eof => formatter.write_str("unexpected end of input"),
            other => {
                write!(formatter, "{other:?}")
            } /* and so forth */
        }
    }
}

impl std::error::Error for Error {}
