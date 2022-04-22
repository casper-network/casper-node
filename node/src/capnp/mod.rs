//! Capnproto serialization and deserialization.

use capnp::Error as CapnpError;

mod types;

/// Capnproto serialization error.
#[derive(Debug)]
pub enum SerializeError {
    /// Capnproto serialization error.
    Capnp(CapnpError),
    /// There are more than u32::MAX items in the container.
    TooManyItems,
}

impl From<CapnpError> for SerializeError {
    fn from(err: CapnpError) -> Self {
        Self::Capnp(err)
    }
}

/// Capnproto deserialization error.
#[derive(Debug)]
pub enum DeserializeError {
    /// Capnproto serialization error.
    Capnp(CapnpError),
    /// Encountered a field that is not in the type schema.
    NotInSchema(capnp::NotInSchema),
    /// Error originating from `casper_types`.
    Types(casper_types::Error),
}

impl From<CapnpError> for DeserializeError {
    fn from(err: CapnpError) -> Self {
        Self::Capnp(err)
    }
}

impl From<casper_types::Error> for DeserializeError {
    fn from(err: casper_types::Error) -> Self {
        Self::Types(err)
    }
}

impl From<capnp::NotInSchema> for DeserializeError {
    fn from(err: capnp::NotInSchema) -> Self {
        Self::NotInSchema(err)
    }
}

/// A type which can be serialized to a `Vec<u8>` using `capnproto`.
pub trait ToCapnpBytes {
    /// Serializes `&self` to a `Vec<u8>`.
    fn try_to_capnp_bytes(&self) -> Result<Vec<u8>, SerializeError>;
}

/// A type which can be deserialized from a `&[u8]` using `capnproto`.
pub trait FromCapnpBytes
where
    Self: Sized,
{
    /// Deserializes the slice into `Self`.
    fn try_from_capnp_bytes(bytes: &[u8]) -> Result<Self, DeserializeError>;
}
