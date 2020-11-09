//! Storage serialization.
//!
//! Centralizes settings and methods for serialization for all parts of storage.

use std::io;

use serde::{de::DeserializeOwned, Serialize};

/// Deserialization helper.
///
/// # Panics
///
/// Panics if deserialization fails. Storage deserialization is infallibe, unless corruption occurs.
#[inline(always)]
pub(crate) fn deser<T: DeserializeOwned>(raw: &[u8]) -> T {
    bincode::deserialize(raw)
        .expect("deserialization failed. this is a bug, or your database has been corrupted")
}

/// Serialization helper
///
/// # Panics
///
/// Panics if serialization fails, for reasons other than IO errors.
#[inline(always)]
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
