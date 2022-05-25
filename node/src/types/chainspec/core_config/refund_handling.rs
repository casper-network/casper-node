use datasize::DataSize;
use num_rational::Ratio;
use serde::{Deserialize, Serialize};

use casper_execution_engine::core::engine_state::engine_config;
use casper_types::bytesrepr::{self, Error, FromBytes, ToBytes};

use crate::utils::serde_helpers;

const REFUND_HANDLING_REFUND_TAG: u8 = 0;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RefundHandlingConfig {
    Refund {
        #[serde(deserialize_with = "serde_helpers::proper_fraction_deserializer")]
        refund_ratio: Ratio<u64>,
    },
}

impl DataSize for RefundHandlingConfig {
    const IS_DYNAMIC: bool = false;

    const STATIC_HEAP_SIZE: usize = 0;

    fn estimate_heap_size(&self) -> usize {
        0
    }
}

impl From<RefundHandlingConfig> for engine_config::RefundHandling {
    fn from(v: RefundHandlingConfig) -> Self {
        match v {
            RefundHandlingConfig::Refund { refund_ratio } => {
                engine_config::RefundHandling::Refund { refund_ratio }
            }
        }
    }
}

impl FromBytes for RefundHandlingConfig {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (tag, rem) = u8::from_bytes(bytes)?;
        match tag {
            REFUND_HANDLING_REFUND_TAG => {
                let (refund_ratio, rem) = FromBytes::from_bytes(rem)?;
                Ok((RefundHandlingConfig::Refund { refund_ratio }, rem))
            }
            _ => Err(Error::Formatting),
        }
    }
}

impl ToBytes for RefundHandlingConfig {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;

        match self {
            RefundHandlingConfig::Refund { refund_ratio } => {
                buffer.push(REFUND_HANDLING_REFUND_TAG);
                buffer.extend(refund_ratio.to_bytes()?);
            }
        }

        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        1 + match self {
            RefundHandlingConfig::Refund { refund_ratio } => refund_ratio.serialized_length(),
        }
    }
}

#[cfg(test)]
mod tests {
    use casper_types::bytesrepr;

    use super::*;

    #[test]
    fn bytesrepr_roundtrip_for_refund() {
        let refund_config = RefundHandlingConfig::Refund {
            refund_ratio: Ratio::new(49, 313),
        };
        bytesrepr::test_serialization_roundtrip(&refund_config);
    }
}
