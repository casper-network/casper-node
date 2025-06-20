use alloc::vec::Vec;

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::bytesrepr::{self, Bytes, Error, FromBytes, ToBytes};

/// A container for arbitrary bytes, tagged with a unique type id.
#[derive(PartialEq, Eq, Clone, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct TaggedBytes {
    type_uid: u64,
    bytes: Bytes,
}

impl TaggedBytes {
    // Ctor.
    pub fn new(type_uid: u64, bytes: Bytes) -> Self {
        Self { type_uid, bytes }
    }

    pub fn deconstruct(self) -> (u64, Bytes) {
        (self.type_uid, self.bytes)
    }

    /// Constructs a `TaggedBytes` from its raw parts.
    pub fn type_uid(&self) -> u64 {
        self.type_uid
    }

    /// Returns the bytes contained in this `TaggedBytes`.
    pub fn bytes(&self) -> &Bytes {
        &self.bytes
    }
}

impl ToBytes for TaggedBytes {
    fn to_bytes(&self) -> Result<Vec<u8>, crate::bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.type_uid.serialized_length() + self.bytes.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        self.type_uid.write_bytes(writer)?;
        self.bytes.write_bytes(writer)
    }
}

impl FromBytes for TaggedBytes {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (type_uid, remainder) = u64::from_bytes(bytes)?;
        let (bytes, remainder) = Bytes::from_bytes(remainder)?;

        Ok((Self { type_uid, bytes }, remainder))
    }
}

/// Generators for [`TaggedBytes`].
#[cfg(any(feature = "testing", feature = "gens", test))]
pub(crate) mod gens {
    use crate::{bytesrepr::Bytes, tagged_bytes::TaggedBytes};
    use proptest::{
        collection,
        prelude::{any, Strategy},
    };

    pub fn tagged_bytes_arb() -> impl Strategy<Value = TaggedBytes> {
        (any::<u64>(), collection::vec(any::<u8>(), 0..1000)).prop_map(|(type_uid, bytes)| {
            TaggedBytes {
                type_uid,
                bytes: Bytes::from(bytes),
            }
        })
    }
}
