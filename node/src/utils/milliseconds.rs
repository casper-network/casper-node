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

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};
    use std::time::Duration;

    #[test]
    fn round_trip() {
        #[derive(Debug, Deserialize, PartialEq, Serialize)]
        struct Example(#[serde(with = "super")] Duration);

        let value = Example(Duration::from_millis(12345));

        let json = serde_json::to_string(&value).expect("serialization failed");
        assert_eq!(json, "12345");
        let deserialized = serde_json::from_str(&json).expect("deserialization failed");

        assert_eq!(value, deserialized);
    }
}
