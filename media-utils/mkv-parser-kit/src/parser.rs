// Copyright 2017-2022 Brian Langenberger
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use std::io::Read;

use super::error::{Error as MatroskaError, Result};

use bitstream_io::BitRead;
type BitReader<R> = bitstream_io::BitReader<R, bitstream_io::BigEndian>;

pub(crate) fn read_element_id_size<R: Read>(reader: &mut R) -> Result<(u32, u64, u64)> {
    let mut r = BitReader::new(reader);
    let (id, id_len) = read_element_id(&mut r)?;
    let (size, size_len) = read_element_size(&mut r)?;
    Ok((id, size, id_len + size_len))
}

pub(crate) fn read_element_id<R: BitRead>(r: &mut R) -> Result<(u32, u64)> {
    match r.read_unary1() {
        Ok(0) => r
            .read::<u32>(7)
            .map_err(MatroskaError::Io)
            .map(|u| (0b1000_0000 | u, 1)),
        Ok(1) => r
            .read::<u32>(6 + 8)
            .map_err(MatroskaError::Io)
            .map(|u| ((0b0100_0000 << 8) | u, 2)),
        Ok(2) => r
            .read::<u32>(5 + 16)
            .map_err(MatroskaError::Io)
            .map(|u| ((0b0010_0000 << 16) | u, 3)),
        Ok(3) => r
            .read::<u32>(4 + 24)
            .map_err(MatroskaError::Io)
            .map(|u| ((0b0001_0000 << 24) | u, 4)),
        Ok(_) => Err(MatroskaError::InvalidID),
        Err(err) => Err(MatroskaError::Io(err)),
    }
}

pub(crate) fn read_element_size<R: BitRead>(r: &mut R) -> Result<(u64, u64)> {
    match r.read_unary1() {
        Ok(0) => r.read(7).map(|s| (s, 1)).map_err(MatroskaError::Io),
        Ok(1) => r.read(6 + 8).map(|s| (s, 2)).map_err(MatroskaError::Io),
        Ok(2) => r
            .read(5 + (2 * 8))
            .map(|s| (s, 3))
            .map_err(MatroskaError::Io),
        Ok(3) => r
            .read(4 + (3 * 8))
            .map(|s| (s, 4))
            .map_err(MatroskaError::Io),
        Ok(4) => r
            .read(3 + (4 * 8))
            .map(|s| (s, 5))
            .map_err(MatroskaError::Io),
        Ok(5) => r
            .read(2 + (5 * 8))
            .map(|s| (s, 6))
            .map_err(MatroskaError::Io),
        Ok(6) => r
            .read(1 + (6 * 8))
            .map(|s| (s, 7))
            .map_err(MatroskaError::Io),
        Ok(7) => r.read(7 * 8).map(|s| (s, 8)).map_err(MatroskaError::Io),
        Ok(_) => Err(MatroskaError::InvalidSize),
        Err(err) => Err(MatroskaError::Io(err)),
    }
}
