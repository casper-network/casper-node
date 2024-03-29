use alloc::vec::Vec;

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    CLType, CLTyped, EraId, PublicKey,
};
#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A bridge record pointing to a new `ValidatorBid` after the public key was changed.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, Clone)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Bridge {
    /// Previous validator public key associated with the bid.
    old_validator_public_key: PublicKey,
    /// New validator public key associated with the bid.
    new_validator_public_key: PublicKey,
    /// Era when bridge record was created.
    era_id: EraId,
}

impl Bridge {
    /// Creates new instance of a bridge record.
    pub fn new(
        old_validator_public_key: PublicKey,
        new_validator_public_key: PublicKey,
        era_id: EraId,
    ) -> Self {
        Self {
            old_validator_public_key,
            new_validator_public_key,
            era_id,
        }
    }

    /// Gets the old validator public key
    pub fn old_validator_public_key(&self) -> &PublicKey {
        &self.old_validator_public_key
    }

    /// Gets the new validator public key
    pub fn new_validator_public_key(&self) -> &PublicKey {
        &self.new_validator_public_key
    }

    /// Gets the era when key change happened
    pub fn era_id(&self) -> &EraId {
        &self.era_id
    }
}

impl CLTyped for Bridge {
    fn cl_type() -> CLType {
        CLType::Any
    }
}

impl ToBytes for Bridge {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut result)?;
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.old_validator_public_key.serialized_length()
            + self.new_validator_public_key.serialized_length()
            + self.era_id.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.old_validator_public_key.write_bytes(writer)?;
        self.new_validator_public_key.write_bytes(writer)?;
        self.era_id.write_bytes(writer)?;
        Ok(())
    }
}

impl FromBytes for Bridge {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (old_validator_public_key, bytes) = FromBytes::from_bytes(bytes)?;
        let (new_validator_public_key, bytes) = FromBytes::from_bytes(bytes)?;
        let (era_id, bytes) = FromBytes::from_bytes(bytes)?;
        Ok((
            Bridge {
                old_validator_public_key,
                new_validator_public_key,
                era_id,
            },
            bytes,
        ))
    }
}
