use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_execution_engine::core::engine_state::engine_config;
use casper_types::bytesrepr::{self, Error, FromBytes, ToBytes};

const FEE_HANDLING_PROPOSER_TAG: u8 = 0;
const FEE_HANDLING_ACCUMULATE_TAG: u8 = 1;

/// Fee handling config
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum FeeHandlingConfig {
    /// Transaction fees are paid to the block proposer.
    ///
    /// This is the default option for public chains.
    PayToProposer,
    /// Transaction fees are accumulated in a special rewards purse.
    ///
    /// This setting makes sense for some private chains.
    Accumulate,
}

impl DataSize for FeeHandlingConfig {
    const IS_DYNAMIC: bool = false;

    const STATIC_HEAP_SIZE: usize = 0;

    fn estimate_heap_size(&self) -> usize {
        0
    }
}

impl From<FeeHandlingConfig> for engine_config::FeeHandling {
    fn from(v: FeeHandlingConfig) -> Self {
        match v {
            FeeHandlingConfig::PayToProposer => engine_config::FeeHandling::PayToProposer,
            FeeHandlingConfig::Accumulate => engine_config::FeeHandling::Accumulate,
        }
    }
}

impl FromBytes for FeeHandlingConfig {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (tag, rem) = u8::from_bytes(bytes)?;
        match tag {
            FEE_HANDLING_PROPOSER_TAG => Ok((FeeHandlingConfig::PayToProposer, rem)),
            FEE_HANDLING_ACCUMULATE_TAG => Ok((FeeHandlingConfig::Accumulate, rem)),
            _ => Err(Error::Formatting),
        }
    }
}

impl ToBytes for FeeHandlingConfig {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;

        match self {
            FeeHandlingConfig::PayToProposer => {
                buffer.push(FEE_HANDLING_PROPOSER_TAG);
            }
            FeeHandlingConfig::Accumulate => {
                buffer.push(FEE_HANDLING_ACCUMULATE_TAG);
            }
        }

        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        1
    }
}

#[cfg(test)]
mod tests {
    use casper_types::bytesrepr;

    use super::*;

    #[test]
    fn bytesrepr_roundtrip_for_refund() {
        let fee_config = FeeHandlingConfig::PayToProposer;
        bytesrepr::test_serialization_roundtrip(&fee_config);
    }

    #[test]
    fn bytesrepr_roundtrip_for_accumulate() {
        let fee_config = FeeHandlingConfig::Accumulate;
        bytesrepr::test_serialization_roundtrip(&fee_config);
    }

    #[test]
    fn does_it_inline() {
        let f = FeeHandlingConfig::PayToProposer;
        eprintln!("{}", toml::to_string(&f).unwrap());
    }
}
