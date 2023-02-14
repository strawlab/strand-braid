// Copyright 2022-2023 Andrew D. Straw.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT
// or http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

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
