// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

/// Synchronized frame number across all cameras.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SyncFno(pub u64);

impl std::fmt::Display for SyncFno {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::result::Result<(), std::fmt::Error> {
        std::fmt::Display::fmt(&self.0, fmt)
    }
}

impl From<u64> for SyncFno {
    fn from(val: u64) -> SyncFno {
        SyncFno(val)
    }
}

// ---------------------------------------------------------------------------
// serde helpers for `SyncFno`, which is represented as u64.

impl serde::Serialize for SyncFno {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u64(self.0)
    }
}

impl<'de> serde::Deserialize<'de> for SyncFno {
    fn deserialize<D>(deserializer: D) -> std::result::Result<SyncFno, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let val: u64 = deserializer.deserialize_u64(SyncFnoVisitor)?;
        Ok(SyncFno(val))
    }
}

struct SyncFnoVisitor;

impl serde::de::Visitor<'_> for SyncFnoVisitor {
    type Value = u64;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("an unsigned 64-bit integer")
    }

    fn visit_u64<E>(self, value: u64) -> std::result::Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(value)
    }
}
