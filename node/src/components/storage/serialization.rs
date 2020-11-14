//! Storage serialization.
//!
//! Centralizes settings and methods for serialization for all parts of storage.

use std::{error, io};

use serde::{de::DeserializeOwned, Serialize};

/// Deserializes from a buffer.
#[inline(always)]
pub(crate) fn deser<T: DeserializeOwned>(raw: &[u8]) -> Result<T, Box<dyn error::Error>> {
    Ok(bincode::deserialize(raw)?)
}

/// Serialization helper
///
/// # Panics
///
/// Panics if serialization fails, for reasons other than IO errors.
#[inline]
pub(crate) fn ser<T: Serialize, W: io::Write>(writer: W, value: &T) -> Result<(), io::Error> {
    match bincode::serialize_into(writer, value) {
        Ok(_) => Ok(()),
        Err(err) => {
            if let bincode::ErrorKind::Io(io_err) = *err {
                Err(io_err)
            } else {
                panic!("serialization error. this is a bug: {}", err)
            }
        }
    }
}

/// Serializes into a buffer.
#[inline(always)]
pub(crate) fn ser_to_bytes<T: Serialize>(value: &T) -> Vec<u8> {
    let mut buffer = Vec::new();
    ser(&mut buffer, value).expect(
        "serialization for type failed. this should never happen (add more test coverage!)",
    );

    buffer
}
