/// serde helpers for `FlydraFloatTimestampLocal` to store as f64
///
/// attempting to load a NaN will result in an error
use crate::*;

struct FlydraF64TimestampLocalVisitor;

impl<'de> serde::de::Visitor<'de> for FlydraF64TimestampLocalVisitor {
    type Value = f64;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a double precision float")
    }

    fn visit_f64<E>(self, value: f64) -> std::result::Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(value)
    }
}

/// serialize to f64 when annotating a field with this for serde auto derive
pub fn serialize<S, CLK>(
    orig: &FlydraFloatTimestampLocal<CLK>,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error>
where
    S: serde::Serializer,
    CLK: Source,
{
    serializer.serialize_f64(orig.as_f64())
}

/// deserialize from f64 when annotating a field with this for serde auto derive
pub fn deserialize<'de, D, S>(de: D) -> std::result::Result<FlydraFloatTimestampLocal<S>, D::Error>
where
    D: serde::de::Deserializer<'de>,
    S: Source,
{
    let val: f64 = de.deserialize_f64(FlydraF64TimestampLocalVisitor)?;
    if val.is_nan() {
        use serde::de::Error;
        return Err(D::Error::custom(
            "cannot convert f64 NaN into FlydraFloatTimestampLocal",
        ));
    }
    Ok(FlydraFloatTimestampLocal::from_notnan_f64(val.into()))
}
