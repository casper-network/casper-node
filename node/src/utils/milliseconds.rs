//! Load and store milliseconds directly as `Duration` using serde.

use serde::{ser::Error, Deserialize, Deserializer, Serializer};
use std::{convert::TryFrom, time::Duration};

/// Serializes a `Duration` as milliseconds.
///
/// Limited to 64 bit.
pub fn serialize<S>(value: &Duration, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let ms = u64::try_from(value.as_millis()).map_err(|_err| {
        S::Error::custom(format!(
            "duration {:?} is too large to be convert down to 64-bit milliseconds",
            value
        ))
    })?;
    serializer.serialize_u64(ms)
}

/// Deserializes a `Duration` as 64-bit milliseconds.
pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
where
    D: Deserializer<'de>,
{
    let ms = u64::deserialize(deserializer)?;

    Ok(Duration::from_millis(ms))
}
