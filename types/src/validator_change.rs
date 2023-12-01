use alloc::vec::Vec;
#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::bytesrepr::{self, FromBytes, ToBytes};

/// A change to a validator's status between two eras.
#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Ord, PartialOrd)]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub enum ValidatorChange {
    /// The validator got newly added to the validator set.
    Added,
    /// The validator was removed from the validator set.
    Removed,
    /// The validator was banned from this era.
    Banned,
    /// The validator was excluded from proposing new blocks in this era.
    CannotPropose,
    /// We saw the validator misbehave in this era.
    SeenAsFaulty,
}

const ADDED_TAG: u8 = 0;
const REMOVED_TAG: u8 = 1;
const BANNED_TAG: u8 = 2;
const CANNOT_PROPOSE_TAG: u8 = 3;
const SEEN_AS_FAULTY_TAG: u8 = 4;

impl ToBytes for ValidatorChange {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            ValidatorChange::Added => ADDED_TAG,
            ValidatorChange::Removed => REMOVED_TAG,
            ValidatorChange::Banned => BANNED_TAG,
            ValidatorChange::CannotPropose => CANNOT_PROPOSE_TAG,
            ValidatorChange::SeenAsFaulty => SEEN_AS_FAULTY_TAG,
        }
        .write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        bytesrepr::U8_SERIALIZED_LENGTH
    }
}

impl FromBytes for ValidatorChange {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        let id = match tag {
            ADDED_TAG => ValidatorChange::Added,
            REMOVED_TAG => ValidatorChange::Removed,
            BANNED_TAG => ValidatorChange::Banned,
            CANNOT_PROPOSE_TAG => ValidatorChange::CannotPropose,
            SEEN_AS_FAULTY_TAG => ValidatorChange::SeenAsFaulty,
            _ => return Err(bytesrepr::Error::NotRepresentable),
        };
        Ok((id, remainder))
    }
}
