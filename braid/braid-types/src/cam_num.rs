// Copyright 2020-2023 Andrew D. Straw.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT
// or http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CamNum(pub u8);

impl std::fmt::Display for CamNum {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::result::Result<(), std::fmt::Error> {
        std::fmt::Display::fmt(&self.0, fmt)
    }
}

impl From<u8> for CamNum {
    fn from(val: u8) -> CamNum {
        CamNum(val)
    }
}

// ---------------------------------------------------------------------------
// serde helpers for `CamNum`, which is represented as u8.

impl serde::Serialize for CamNum {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u8(self.0)
    }
}

impl<'de> serde::Deserialize<'de> for CamNum {
    fn deserialize<D>(deserializer: D) -> std::result::Result<CamNum, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let val: u8 = deserializer.deserialize_u8(CamNumVisitor)?;
        Ok(CamNum(val))
    }
}

struct CamNumVisitor;

impl serde::de::Visitor<'_> for CamNumVisitor {
    type Value = u8;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("an unsigned 8-bit integer")
    }

    fn visit_u8<E>(self, value: u8) -> std::result::Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(value)
    }
}
