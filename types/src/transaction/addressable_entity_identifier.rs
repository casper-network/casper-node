use alloc::{string::String, vec::Vec};
use core::fmt::{self, Debug, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(doc)]
use super::{ExecutableDeployItem, TransactionTarget};
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    AddressableEntityHash,
};

const HASH_TAG: u8 = 0;
const NAME_TAG: u8 = 1;

/// Identifier for the contract object within a [`TransactionTarget::Stored`] or an
/// [`ExecutableDeployItem`].
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(
        description = "Identifier for the contract object within a `Stored` transaction target \
        or an `ExecutableDeployItem`."
    )
)]
#[serde(deny_unknown_fields)]
pub enum AddressableEntityIdentifier {
    /// The hash identifying the addressable entity.
    Hash(AddressableEntityHash),
    /// The name identifying the addressable entity.
    Name(String),
}

impl AddressableEntityIdentifier {
    /// Returns a random `AddressableEntityIdentifier`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        if rng.gen() {
            AddressableEntityIdentifier::Hash(AddressableEntityHash::new(rng.gen()))
        } else {
            AddressableEntityIdentifier::Name(rng.random_string(1..21))
        }
    }
}

impl Display for AddressableEntityIdentifier {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            AddressableEntityIdentifier::Hash(hash) => write!(formatter, "entity-hash({})", hash),
            AddressableEntityIdentifier::Name(name) => write!(formatter, "entity-name({})", name),
        }
    }
}

impl ToBytes for AddressableEntityIdentifier {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            AddressableEntityIdentifier::Hash(hash) => {
                HASH_TAG.write_bytes(writer)?;
                hash.write_bytes(writer)
            }
            AddressableEntityIdentifier::Name(name) => {
                NAME_TAG.write_bytes(writer)?;
                name.write_bytes(writer)
            }
        }
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                AddressableEntityIdentifier::Hash(hash) => hash.serialized_length(),
                AddressableEntityIdentifier::Name(name) => name.serialized_length(),
            }
    }
}

impl FromBytes for AddressableEntityIdentifier {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            HASH_TAG => {
                let (hash, remainder) = AddressableEntityHash::from_bytes(remainder)?;
                Ok((AddressableEntityIdentifier::Hash(hash), remainder))
            }
            NAME_TAG => {
                let (name, remainder) = String::from_bytes(remainder)?;
                Ok((AddressableEntityIdentifier::Name(name), remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        for _ in 0..10 {
            bytesrepr::test_serialization_roundtrip(&AddressableEntityIdentifier::random(rng));
        }
    }
}
