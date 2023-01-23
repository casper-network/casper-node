// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use std::fmt::{self, Display, Formatter};

use datasize::DataSize;
#[cfg(test)]
use rand::Rng;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(test)]
use casper_types::testing::TestRng;
use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    EraId, Timestamp,
};

const ERA_ID_TAG: u8 = 0;
const GENESIS_TAG: u8 = 1;

/// The first era to which the associated protocol version applies.
#[derive(Copy, Clone, DataSize, PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[serde(untagged)]
pub enum ActivationPoint {
    EraId(EraId),
    Genesis(Timestamp),
}

impl ActivationPoint {
    /// Returns whether we should upgrade the node due to the next era being the upgrade activation
    /// point.
    pub(crate) fn should_upgrade(&self, era_being_deactivated: &EraId) -> bool {
        match self {
            ActivationPoint::EraId(era_id) => era_being_deactivated.successor() >= *era_id,
            ActivationPoint::Genesis(_) => false,
        }
    }

    /// Returns the Era ID if `self` is of `EraId` variant, or else 0 if `Genesis`.
    pub(crate) fn era_id(&self) -> EraId {
        match self {
            ActivationPoint::EraId(era_id) => *era_id,
            ActivationPoint::Genesis(_) => EraId::from(0),
        }
    }

    /// Returns the timestamp if `self` is of `Genesis` variant, or else `None`.
    pub(crate) fn genesis_timestamp(&self) -> Option<Timestamp> {
        match self {
            ActivationPoint::EraId(_) => None,
            ActivationPoint::Genesis(timestamp) => Some(*timestamp),
        }
    }
}

impl Display for ActivationPoint {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ActivationPoint::EraId(era_id) => write!(formatter, "activation point {}", era_id),
            ActivationPoint::Genesis(timestamp) => {
                write!(formatter, "activation point {}", timestamp)
            }
        }
    }
}

impl ToBytes for ActivationPoint {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        match self {
            ActivationPoint::EraId(era_id) => {
                let mut buffer = vec![ERA_ID_TAG];
                buffer.extend(era_id.to_bytes()?);
                Ok(buffer)
            }
            ActivationPoint::Genesis(timestamp) => {
                let mut buffer = vec![GENESIS_TAG];
                buffer.extend(timestamp.to_bytes()?);
                Ok(buffer)
            }
        }
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                ActivationPoint::EraId(era_id) => era_id.serialized_length(),
                ActivationPoint::Genesis(timestamp) => timestamp.serialized_length(),
            }
    }
}

impl FromBytes for ActivationPoint {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            ERA_ID_TAG => {
                let (era_id, remainder) = EraId::from_bytes(remainder)?;
                Ok((ActivationPoint::EraId(era_id), remainder))
            }
            GENESIS_TAG => {
                let (timestamp, remainder) = Timestamp::from_bytes(remainder)?;
                Ok((ActivationPoint::Genesis(timestamp), remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

#[cfg(test)]
impl ActivationPoint {
    /// Generates a random instance using a `TestRng`.
    pub fn random(rng: &mut TestRng) -> Self {
        if rng.gen() {
            ActivationPoint::EraId(rng.gen())
        } else {
            ActivationPoint::Genesis(Timestamp::random(rng))
        }
    }
}
