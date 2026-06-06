// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

pub use ebml_types::{BoxData, EbmlElement, Tag};
pub use error::{Error, Result};

mod de;
mod ebml_types;
mod error;
mod parser;

pub fn ebml_parse<R>(mut rdr: R) -> Result<(Vec<EbmlElement>, R)>
where
    R: std::io::Read + std::io::Seek,
{
    let elements = crate::de::Deserializer::from_reader(&mut rdr, 0, 0)
        .collect::<Result<Vec<EbmlElement>>>()?;
    Ok((elements, rdr))
}
