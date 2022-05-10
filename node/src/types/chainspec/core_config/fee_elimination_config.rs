use datasize::DataSize;
use num_rational::Ratio;
use serde::{Deserialize, Serialize};

use casper_execution_engine::core::engine_state::engine_config;
use casper_types::bytesrepr::{self, Error, FromBytes, ToBytes};

use crate::utils::serde_helpers;

const FEE_ELIMINATION_REFUND_TAG: u8 = 0;
const FEE_ELIMINATION_ACCUMULATE_TAG: u8 = 1;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum FeeEliminationConfig {
    Refund {
        #[serde(deserialize_with = "serde_helpers::proper_fraction_deserializer")]
        refund_ratio: Ratio<u64>,
    },
    Accumulate,
}

impl DataSize for FeeEliminationConfig {
    const IS_DYNAMIC: bool = false;

    const STATIC_HEAP_SIZE: usize = 0;

    fn estimate_heap_size(&self) -> usize {
        0
    }
}

impl From<FeeEliminationConfig> for engine_config::FeeElimination {
    fn from(v: FeeEliminationConfig) -> Self {
        match v {
            FeeEliminationConfig::Refund { refund_ratio } => {
                engine_config::FeeElimination::Refund { refund_ratio }
            }
            FeeEliminationConfig::Accumulate => engine_config::FeeElimination::Accumulate,
        }
    }
}

impl FromBytes for FeeEliminationConfig {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (tag, rem) = u8::from_bytes(bytes)?;
        match tag {
            FEE_ELIMINATION_REFUND_TAG => {
                let (refund_ratio, rem) = FromBytes::from_bytes(rem)?;
                Ok((FeeEliminationConfig::Refund { refund_ratio }, rem))
            }
            FEE_ELIMINATION_ACCUMULATE_TAG => Ok((FeeEliminationConfig::Accumulate, rem)),
            _ => Err(Error::Formatting),
        }
    }
}

impl ToBytes for FeeEliminationConfig {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;

        match self {
            FeeEliminationConfig::Refund { refund_ratio } => {
                buffer.push(FEE_ELIMINATION_REFUND_TAG);
                buffer.extend(refund_ratio.to_bytes()?);
            }
            FeeEliminationConfig::Accumulate => {
                buffer.push(FEE_ELIMINATION_ACCUMULATE_TAG);
            }
        }

        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        1 + match self {
            FeeEliminationConfig::Refund { refund_ratio } => refund_ratio.serialized_length(),
            FeeEliminationConfig::Accumulate => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use casper_types::bytesrepr;

    use super::*;

    #[test]
    fn bytesrepr_roundtrip_for_refund() {
        let refund_config = FeeEliminationConfig::Refund {
            refund_ratio: Ratio::new(49, 313),
        };
        bytesrepr::test_serialization_roundtrip(&refund_config);
    }

    #[test]
    fn bytesrepr_roundtrip_for_accumulate() {
        let refund_config = FeeEliminationConfig::Accumulate;
        bytesrepr::test_serialization_roundtrip(&refund_config);
    }
}
