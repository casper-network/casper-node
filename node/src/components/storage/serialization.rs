//! Storage serialization.
//!
//! Centralizes settings and methods for serialization for all parts of storage.
//!
//! Errors are unified into a generic, type erased `std` error to allow for easy interchange of the
//! serialization format if desired.

use std::error;

use serde::{de::DeserializeOwned, Serialize};

/// Deserializes from a buffer.
#[inline(always)]
pub(crate) fn deser<T: DeserializeOwned>(
    raw: &[u8],
) -> Result<T, Box<dyn error::Error + Send + Sync>> {
    Ok(bincode::deserialize(raw)?)
}

/// Serializes into a buffer.
#[inline(always)]
pub(crate) fn ser<T: Serialize>(value: &T) -> Result<Vec<u8>, Box<dyn error::Error + Send + Sync>> {
    Ok(bincode::serialize(value)?)
}
