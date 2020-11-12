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
        Some(ref tl) => tl.as_f64(),
        None => std::f64::NAN,
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
