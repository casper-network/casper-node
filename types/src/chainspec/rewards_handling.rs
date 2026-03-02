/// Configuration options of reward handling that are executed as part of rewards distribution.
use num_rational::Ratio;
use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{self, Error, FromBytes, ToBytes},
    uref::FromStrError,
    URef,
};

pub const REWARDS_HANDLING_RATIO_TAG: u8 = 0;

const REWARDS_HANDLING_STANDARD_TAG: u8 = 0;

const REWARDS_HANDLING_SUSTAIN_TAG: u8 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RewardsHandling {
    #[default]
    Standard,

    Sustain {
        ratio: Ratio<u64>,
        purse_address: String,
    },
}

impl RewardsHandling {
    pub fn purse_address(&self) -> Option<Result<URef, FromStrError>> {
        match self {
            Self::Standard => None,
            Self::Sustain { purse_address, .. } => Some(URef::from_formatted_str(purse_address)),
        }
    }

    pub fn maybe_ratio(&self) -> Option<Ratio<u64>> {
        match self {
            Self::Standard => None,
            Self::Sustain { ratio, .. } => Some(*ratio),
        }
    }

    pub fn is_valid_configuration(&self) -> bool {
        match self {
            Self::Standard => true,
            Self::Sustain {
                ratio,
                purse_address,
            } => {
                if *ratio.numer() > *ratio.denom() {
                    return false;
                }

                if URef::from_formatted_str(purse_address).is_err() {
                    return false;
                }

                true
            }
        }
    }
}

impl ToBytes for RewardsHandling {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;

        match self {
            Self::Standard => {
                buffer.push(REWARDS_HANDLING_STANDARD_TAG);
            }
            Self::Sustain {
                ratio,
                purse_address,
            } => {
                buffer.push(REWARDS_HANDLING_SUSTAIN_TAG);
                buffer.extend(ratio.to_bytes()?);
                buffer.extend(purse_address.to_bytes()?);
            }
        }

        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        1 + match self {
            RewardsHandling::Standard => 0,
            RewardsHandling::Sustain {
                ratio,
                purse_address,
            } => ratio.serialized_length() + purse_address.serialized_length(),
        }
    }
}

impl FromBytes for RewardsHandling {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (tag, rem) = u8::from_bytes(bytes)?;

        match tag {
            REWARDS_HANDLING_STANDARD_TAG => Ok((RewardsHandling::Standard, rem)),
            REWARDS_HANDLING_SUSTAIN_TAG => {
                let (ratio, rem) = FromBytes::from_bytes(rem)?;
                let (purse_address, rem) = String::from_bytes(rem)?;
                Ok((
                    RewardsHandling::Sustain {
                        ratio,
                        purse_address,
                    },
                    rem,
                ))
            }
            _ => Err(Error::Formatting),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::URef;

    #[test]
    fn bytesrepr_roundtrip_for_sustain() {
        let rewards_handling = RewardsHandling::Sustain {
            ratio: Ratio::new(49, 313),
            purse_address: URef::default().to_formatted_string(),
        };
        bytesrepr::test_serialization_roundtrip(&rewards_handling);
    }

    #[test]
    fn bytesrepr_roundtrip_for_standard() {
        let rewards_handling = RewardsHandling::Standard;
        bytesrepr::test_serialization_roundtrip(&rewards_handling);
    }
}
