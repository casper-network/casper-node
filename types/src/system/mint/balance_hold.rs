use core::fmt::{Debug, Display, Formatter};

use alloc::vec::Vec;
#[cfg(any(feature = "std", test))]
use std::convert::TryFrom;

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::distributions::{Distribution, Standard};
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr,
    bytesrepr::{FromBytes, ToBytes},
    system::auction::Error,
    BlockTime, Key, KeyTag, Timestamp, URefAddr, UREF_ADDR_LENGTH,
};

const BALANCE_HOLD_ADDR_TAG_LENGTH: u8 = 1;

const GAS_TAG: u8 = 0;

/// Serialization tag for BalanceHold variants.
#[derive(
    Debug, Default, PartialOrd, Ord, PartialEq, Eq, Hash, Clone, Copy, Serialize, Deserialize,
)]
#[repr(u8)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum BalanceHoldAddrTag {
    #[default]
    /// Tag for gas variant.
    Gas = GAS_TAG,
}

impl BalanceHoldAddrTag {
    /// The length in bytes of a [`BalanceHoldAddrTag`].
    pub const BALANCE_HOLD_ADDR_TAG_LENGTH: usize = 1;

    /// Attempts to map `BalanceHoldAddrTag` from a u8.
    pub fn try_from_u8(value: u8) -> Option<Self> {
        // TryFrom requires std, so doing this instead.
        if value == GAS_TAG {
            return Some(BalanceHoldAddrTag::Gas);
        }
        None
    }

    /// Returns key prefix for a purse by balance hold addr tag.
    pub fn purse_prefix_by_tag(&self, purse_addr: URefAddr) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut ret = Vec::with_capacity(purse_addr.serialized_length() + 2);
        ret.push(KeyTag::BalanceHold as u8);
        ret.push(*self as u8);
        purse_addr.write_bytes(&mut ret)?;
        Ok(ret)
    }
}

impl Display for BalanceHoldAddrTag {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let tag = match self {
            BalanceHoldAddrTag::Gas => GAS_TAG,
        };
        write!(f, "{}", base16::encode_lower(&[tag]))
    }
}

/// Balance hold address.
#[derive(PartialOrd, Ord, PartialEq, Eq, Hash, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum BalanceHoldAddr {
    /// Gas hold variant.
    Gas {
        /// The address of the purse this hold is on.
        purse_addr: URefAddr,
        /// The block time this hold was placed.
        block_time: BlockTime,
    },
    // future balance hold variants might allow punitive lockup or settlement periods, etc
}

impl BalanceHoldAddr {
    /// The length in bytes of a [`BalanceHoldAddr`] for a gas hold address.
    pub const GAS_HOLD_ADDR_LENGTH: usize =
        UREF_ADDR_LENGTH + BalanceHoldAddrTag::BALANCE_HOLD_ADDR_TAG_LENGTH;

    /// Creates a Gas variant instance of [`BalanceHoldAddr`].
    #[cfg(any(feature = "testing", test))]
    pub(crate) const fn new_gas(purse_addr: URefAddr, block_time: BlockTime) -> BalanceHoldAddr {
        BalanceHoldAddr::Gas {
            purse_addr,
            block_time,
        }
    }

    /// How long is be the serialized value for this instance.
    pub fn serialized_length(&self) -> usize {
        match self {
            BalanceHoldAddr::Gas {
                purse_addr,
                block_time,
            } => {
                BALANCE_HOLD_ADDR_TAG_LENGTH as usize
                    + ToBytes::serialized_length(purse_addr)
                    + ToBytes::serialized_length(block_time)
            }
        }
    }

    /// Returns the tag of this instance.
    pub fn tag(&self) -> BalanceHoldAddrTag {
        match self {
            BalanceHoldAddr::Gas { .. } => BalanceHoldAddrTag::Gas,
        }
    }

    /// Returns the `[URefAddr]` for the purse associated with this hold.
    pub fn purse_addr(&self) -> URefAddr {
        match self {
            BalanceHoldAddr::Gas { purse_addr, .. } => *purse_addr,
        }
    }

    /// Returns the `[BlockTime]` when this hold was written.
    pub fn block_time(&self) -> BlockTime {
        match self {
            BalanceHoldAddr::Gas { block_time, .. } => *block_time,
        }
    }

    /// Returns the common prefix of all holds on the cited purse.
    pub fn balance_hold_prefix(&self) -> Result<Vec<u8>, Error> {
        let purse_addr_bytes = self.purse_addr().to_bytes()?;
        let size = 1 + purse_addr_bytes.len();
        let mut ret = Vec::with_capacity(size);
        ret.push(KeyTag::BalanceHold as u8);
        ret.extend(purse_addr_bytes);
        Ok(ret)
    }
}

impl ToBytes for BalanceHoldAddr {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.push(self.tag() as u8);
        match self {
            BalanceHoldAddr::Gas {
                purse_addr,
                block_time,
            } => {
                buffer.append(&mut purse_addr.to_bytes()?);
                buffer.append(&mut block_time.to_bytes()?)
            }
        }
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.serialized_length()
    }
}

impl FromBytes for BalanceHoldAddr {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder): (u8, &[u8]) = FromBytes::from_bytes(bytes)?;
        match tag {
            tag if tag == BalanceHoldAddrTag::Gas as u8 => {
                let (purse_addr, rem) = URefAddr::from_bytes(remainder)?;
                let (block_time, rem) = BlockTime::from_bytes(rem)?;
                Ok((
                    BalanceHoldAddr::Gas {
                        purse_addr,
                        block_time,
                    },
                    rem,
                ))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

impl Default for BalanceHoldAddr {
    fn default() -> Self {
        BalanceHoldAddr::Gas {
            purse_addr: URefAddr::default(),
            block_time: BlockTime::default(),
        }
    }
}

impl From<BalanceHoldAddr> for Key {
    fn from(balance_hold_addr: BalanceHoldAddr) -> Self {
        Key::BalanceHold(balance_hold_addr)
    }
}

#[cfg(any(feature = "std", test))]
impl TryFrom<Key> for BalanceHoldAddr {
    type Error = ();

    fn try_from(value: Key) -> Result<Self, Self::Error> {
        if let Key::BalanceHold(balance_hold_addr) = value {
            Ok(balance_hold_addr)
        } else {
            Err(())
        }
    }
}

impl Display for BalanceHoldAddr {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let tag = self.tag();
        match self {
            BalanceHoldAddr::Gas {
                purse_addr,
                block_time,
            } => {
                write!(
                    f,
                    "{}-{}-{}",
                    tag,
                    base16::encode_lower(&purse_addr),
                    Timestamp::from(block_time.value())
                )
            }
        }
    }
}

impl Debug for BalanceHoldAddr {
    fn fmt(&self, f: &mut Formatter) -> core::fmt::Result {
        match self {
            BalanceHoldAddr::Gas {
                purse_addr,
                block_time,
            } => write!(
                f,
                "BidAddr::Gas({}, {})",
                base16::encode_lower(&purse_addr),
                Timestamp::from(block_time.value())
            ),
        }
    }
}

#[cfg(any(feature = "testing", test))]
impl Distribution<BalanceHoldAddr> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> BalanceHoldAddr {
        BalanceHoldAddr::new_gas(rng.gen(), BlockTime::new(rng.gen()))
    }
}

#[cfg(test)]
mod tests {
    use crate::{bytesrepr, system::mint::BalanceHoldAddr, BlockTime, Timestamp};

    #[test]
    fn serialization_roundtrip() {
        let addr = BalanceHoldAddr::new_gas([1; 32], BlockTime::new(Timestamp::now().millis()));
        bytesrepr::test_serialization_roundtrip(&addr);
    }
}

#[cfg(test)]
mod prop_test_gas {
    use proptest::prelude::*;

    use crate::{bytesrepr, gens};

    proptest! {
        #[test]
        fn test_variant_gas(addr in gens::balance_hold_addr_arb()) {
            bytesrepr::test_serialization_roundtrip(&addr);
        }
    }
}
