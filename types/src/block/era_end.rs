mod era_end_v1;
mod era_end_v2;

use alloc::{collections::BTreeMap, vec::Vec};
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    PublicKey, Rewards, U512,
};
pub use era_end_v1::{EraEndV1, EraReport};
pub use era_end_v2::EraEndV2;

const TAG_LENGTH: usize = U8_SERIALIZED_LENGTH;

/// Tag for block body v1.
pub const ERA_END_V1_TAG: u8 = 0;
/// Tag for block body v2.
pub const ERA_END_V2_TAG: u8 = 1;

/// The versioned era end of a block, storing the data for a switch block.
/// It encapsulates different variants of the EraEnd struct.
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(any(feature = "testing", test), derive(PartialEq))]
#[derive(Clone, Hash, Serialize, Deserialize, Debug)]
pub enum EraEnd {
    /// The legacy, initial version of the body portion of a block.
    V1(EraEndV1),
    /// The version 2 of the body portion of a block, which includes the
    /// `past_finality_signatures`.
    V2(EraEndV2),
}

impl EraEnd {
    /// Retrieves the deploy hashes within the block.
    pub fn equivocators(&self) -> &[PublicKey] {
        match self {
            EraEnd::V1(v1) => v1.equivocators(),
            EraEnd::V2(v2) => v2.equivocators(),
        }
    }

    /// Retrieves the transfer hashes within the block.
    pub fn inactive_validators(&self) -> &[PublicKey] {
        match self {
            EraEnd::V1(v1) => v1.inactive_validators(),
            EraEnd::V2(v2) => v2.inactive_validators(),
        }
    }

    /// Returns the deploy and transfer hashes in the order in which they were executed.
    pub fn next_era_validator_weights(&self) -> &BTreeMap<PublicKey, U512> {
        match self {
            EraEnd::V1(v1) => v1.next_era_validator_weights(),
            EraEnd::V2(v2) => v2.next_era_validator_weights(),
        }
    }

    /// Returns the deploy and transfer hashes in the order in which they were executed.
    pub fn rewards(&self) -> Rewards {
        match self {
            EraEnd::V1(v1) => Rewards::V1(v1.rewards()),
            EraEnd::V2(v2) => Rewards::V2(v2.rewards()),
        }
    }
}

impl Display for EraEnd {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            EraEnd::V1(v1) => Display::fmt(&v1, formatter),
            EraEnd::V2(v2) => Display::fmt(&v2, formatter),
        }
    }
}

impl From<EraEndV1> for EraEnd {
    fn from(era_end: EraEndV1) -> Self {
        EraEnd::V1(era_end)
    }
}

impl From<EraEndV2> for EraEnd {
    fn from(era_end: EraEndV2) -> Self {
        EraEnd::V2(era_end)
    }
}

impl ToBytes for EraEnd {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        match self {
            EraEnd::V1(v1) => {
                buffer.insert(0, ERA_END_V1_TAG);
                buffer.extend(v1.to_bytes()?);
            }
            EraEnd::V2(v2) => {
                buffer.insert(0, ERA_END_V2_TAG);
                buffer.extend(v2.to_bytes()?);
            }
        }
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        TAG_LENGTH
            + match self {
                EraEnd::V1(v1) => v1.serialized_length(),
                EraEnd::V2(v2) => v2.serialized_length(),
            }
    }
}

impl FromBytes for EraEnd {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            ERA_END_V1_TAG => {
                let (body, remainder): (EraEndV1, _) = FromBytes::from_bytes(remainder)?;
                Ok((Self::V1(body), remainder))
            }
            ERA_END_V2_TAG => {
                let (body, remainder): (EraEndV2, _) = FromBytes::from_bytes(remainder)?;
                Ok((Self::V2(body), remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}
