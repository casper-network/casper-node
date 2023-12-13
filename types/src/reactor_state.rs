use crate::bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH};
use alloc::vec::Vec;
#[cfg(feature = "datasize")]
use datasize::DataSize;
use derive_more::Display;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(test)]
use rand::Rng;

#[cfg(test)]
use crate::testing::TestRng;

/// The state of the reactor.
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Debug, Display)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum ReactorState {
    /// Get all components and reactor state set up on start.
    Initialize,
    /// Orient to the network and attempt to catch up to tip.
    CatchUp,
    /// Running commit upgrade and creating immediate switch block.
    Upgrading,
    /// Stay caught up with tip.
    KeepUp,
    /// Node is currently caught up and is an active validator.
    Validate,
    /// Node should be shut down for upgrade.
    ShutdownForUpgrade,
}

impl ReactorState {
    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..6) {
            0 => Self::Initialize,
            1 => Self::CatchUp,
            2 => Self::Upgrading,
            3 => Self::KeepUp,
            4 => Self::Validate,
            5 => Self::ShutdownForUpgrade,
            _ => panic!(),
        }
    }
}

const INITIALIZE_TAG: u8 = 0;
const CATCHUP_TAG: u8 = 1;
const UPGRADING_TAG: u8 = 2;
const KEEPUP_TAG: u8 = 3;
const VALIDATE_TAG: u8 = 4;
const SHUTDOWN_FOR_UPGRADE_TAG: u8 = 5;

impl ToBytes for ReactorState {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            ReactorState::Initialize => INITIALIZE_TAG,
            ReactorState::CatchUp => CATCHUP_TAG,
            ReactorState::Upgrading => UPGRADING_TAG,
            ReactorState::KeepUp => KEEPUP_TAG,
            ReactorState::Validate => VALIDATE_TAG,
            ReactorState::ShutdownForUpgrade => SHUTDOWN_FOR_UPGRADE_TAG,
        }
        .write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
    }
}

impl FromBytes for ReactorState {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        let reactor_state = match tag {
            INITIALIZE_TAG => ReactorState::Initialize,
            CATCHUP_TAG => ReactorState::CatchUp,
            UPGRADING_TAG => ReactorState::Upgrading,
            KEEPUP_TAG => ReactorState::KeepUp,
            VALIDATE_TAG => ReactorState::Validate,
            SHUTDOWN_FOR_UPGRADE_TAG => ReactorState::ShutdownForUpgrade,
            _ => return Err(bytesrepr::Error::NotRepresentable),
        };
        Ok((reactor_state, remainder))
    }
}
