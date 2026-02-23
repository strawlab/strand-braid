// Copyright 2020-2023 Andrew D. Straw.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT
// or http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

/// serde helpers for `Option<FlydraFloatTimestampLocal>` to store as f64.
///
/// A `None` value will be represented as a floating point NaN.
use crate::*;

/// serialize to f64 when annotating a field with this for serde auto derive
pub fn serialize<S, CLK>(
    orig: &Option<FlydraFloatTimestampLocal<CLK>>,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error>
where
    S: serde::Serializer,
    CLK: Source,
{
    let val = match orig {
        Some(tl) => tl.as_f64(),
        None => f64::NAN,
    };
    serializer.serialize_f64(val)
}

/// deserialize from f64 when annotating a field with this for serde auto derive
pub fn deserialize<'de, D, S>(
    de: D,
) -> std::result::Result<Option<FlydraFloatTimestampLocal<S>>, D::Error>
where
    D: serde::de::Deserializer<'de>,
    S: Source,
{
    match timestamp_f64::deserialize(de) {
        Ok(valid) => Ok(Some(valid)),
        Err(_) => {
            // TODO: should match on DeserializeError with empty field only,
            // otherwise, return error. The way this is written, anything
            // will return a nan.
            Ok(None)
        }
    }
}
