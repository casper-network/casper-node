use crate::{
    account::{AccountHash, ACCOUNT_HASH_LENGTH},
    bytesrepr,
    bytesrepr::{FromBytes, ToBytes},
    system::auction::error::Error,
    Key, KeyTag, PublicKey,
};
use alloc::vec::Vec;
use core::fmt::{Debug, Display, Formatter};
#[cfg(feature = "datasize")]
use datasize::DataSize;
use rand::{
    distributions::{Distribution, Standard},
    Rng,
};
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const UNIFIED_TAG: u8 = 0;
const VALIDATOR_TAG: u8 = 1;
const DELEGATOR_TAG: u8 = 2;

/// Serialization tag for BidAddr variants.
#[derive(
    Debug, Default, PartialOrd, Ord, PartialEq, Eq, Hash, Clone, Copy, Serialize, Deserialize,
)]
#[repr(u8)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum BidAddrTag {
    /// BidAddr for legacy unified bid.
    Unified = UNIFIED_TAG,
    /// BidAddr for validator bid.
    #[default]
    Validator = VALIDATOR_TAG,
    /// BidAddr for delegator bid.
    Delegator = DELEGATOR_TAG,
}

impl Display for BidAddrTag {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let tag = match self {
            BidAddrTag::Unified => UNIFIED_TAG,
            BidAddrTag::Validator => VALIDATOR_TAG,
            BidAddrTag::Delegator => DELEGATOR_TAG,
        };
        write!(f, "{}", base16::encode_lower(&[tag]))
    }
}

impl BidAddrTag {
    /// The length in bytes of a [`BidAddrTag`].
    pub const BID_ADDR_TAG_LENGTH: usize = 1;

    /// Attempts to map `BidAddrTag` from a u8.
    pub fn try_from_u8(value: u8) -> Option<Self> {
        // TryFrom requires std, so doing this instead.
        if value == UNIFIED_TAG {
            return Some(BidAddrTag::Unified);
        }
        if value == VALIDATOR_TAG {
            return Some(BidAddrTag::Validator);
        }
        if value == DELEGATOR_TAG {
            return Some(BidAddrTag::Delegator);
        }

        None
    }
}

/// Bid Address
#[derive(PartialOrd, Ord, PartialEq, Eq, Hash, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum BidAddr {
    /// Unified BidAddr.
    Unified(AccountHash),
    /// Validator BidAddr.
    Validator(AccountHash),
    /// Delegator BidAddr.
    Delegator {
        /// The validator addr.
        validator: AccountHash,
        /// The delegator addr.
        delegator: AccountHash,
    },
}

impl BidAddr {
    /// The length in bytes of a [`BidAddr`] for a validator bid.
    pub const VALIDATOR_BID_ADDR_LENGTH: usize =
        ACCOUNT_HASH_LENGTH + BidAddrTag::BID_ADDR_TAG_LENGTH;

    /// The length in bytes of a [`BidAddr`] for a delegator bid.
    pub const DELEGATOR_BID_ADDR_LENGTH: usize =
        (ACCOUNT_HASH_LENGTH * 2) + BidAddrTag::BID_ADDR_TAG_LENGTH;

    /// Constructs a new [`BidAddr`] instance from a validator's [`AccountHash`].
    pub const fn new_validator_addr(validator: [u8; ACCOUNT_HASH_LENGTH]) -> Self {
        BidAddr::Validator(AccountHash::new(validator))
    }

    /// Constructs a new [`BidAddr`] instance from the [`AccountHash`] pair of a validator
    /// and a delegator.
    pub const fn new_delegator_addr(
        pair: ([u8; ACCOUNT_HASH_LENGTH], [u8; ACCOUNT_HASH_LENGTH]),
    ) -> Self {
        BidAddr::Delegator {
            validator: AccountHash::new(pair.0),
            delegator: AccountHash::new(pair.1),
        }
    }

    #[allow(missing_docs)]
    pub const fn legacy(validator: [u8; ACCOUNT_HASH_LENGTH]) -> Self {
        BidAddr::Unified(AccountHash::new(validator))
    }

    /// Create a new instance of a [`BidAddr`].
    pub fn new_from_public_keys(
        validator: &PublicKey,
        maybe_delegator: Option<&PublicKey>,
    ) -> Self {
        if let Some(delegator) = maybe_delegator {
            BidAddr::Delegator {
                validator: AccountHash::from(validator),
                delegator: AccountHash::from(delegator),
            }
        } else {
            BidAddr::Validator(AccountHash::from(validator))
        }
    }

    /// Returns the common prefix of all delegators to the cited validator.
    pub fn delegators_prefix(&self) -> Result<Vec<u8>, Error> {
        let validator = self.validator_account_hash();
        let mut ret = Vec::with_capacity(validator.serialized_length() + 2);
        ret.push(KeyTag::BidAddr as u8);
        ret.push(BidAddrTag::Delegator as u8);
        validator.write_bytes(&mut ret)?;
        Ok(ret)
    }

    /// Validator account hash.
    pub fn validator_account_hash(&self) -> AccountHash {
        match self {
            BidAddr::Unified(account_hash) | BidAddr::Validator(account_hash) => *account_hash,
            BidAddr::Delegator { validator, .. } => *validator,
        }
    }

    /// Delegator account hash or none.
    pub fn maybe_delegator_account_hash(&self) -> Option<AccountHash> {
        match self {
            BidAddr::Unified(_) | BidAddr::Validator(_) => None,
            BidAddr::Delegator { delegator, .. } => Some(*delegator),
        }
    }

    /// If true, this instance is the key for a delegator bid record.
    /// Else, it is the key for a validator bid record.
    pub fn is_delegator_bid_addr(&self) -> bool {
        match self {
            BidAddr::Unified(_) | BidAddr::Validator(_) => false,
            BidAddr::Delegator { .. } => true,
        }
    }

    /// How long will be the serialized value for this instance.
    pub fn serialized_length(&self) -> usize {
        match self {
            BidAddr::Unified(account_hash) | BidAddr::Validator(account_hash) => {
                ToBytes::serialized_length(account_hash) + 1
            }
            BidAddr::Delegator {
                validator,
                delegator,
            } => ToBytes::serialized_length(validator) + ToBytes::serialized_length(delegator) + 1,
        }
    }

    /// Returns the BiddAddrTag of this instance.
    pub fn tag(&self) -> BidAddrTag {
        match self {
            BidAddr::Unified(_) => BidAddrTag::Unified,
            BidAddr::Validator(_) => BidAddrTag::Validator,
            BidAddr::Delegator { .. } => BidAddrTag::Delegator,
        }
    }
}

impl ToBytes for BidAddr {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.push(self.tag() as u8);
        buffer.append(&mut self.validator_account_hash().to_bytes()?);
        if let Some(delegator) = self.maybe_delegator_account_hash() {
            buffer.append(&mut delegator.to_bytes()?);
        }
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.serialized_length()
    }
}

impl FromBytes for BidAddr {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder): (u8, &[u8]) = FromBytes::from_bytes(bytes)?;
        match tag {
            tag if tag == BidAddrTag::Unified as u8 => AccountHash::from_bytes(remainder)
                .map(|(account_hash, remainder)| (BidAddr::Unified(account_hash), remainder)),
            tag if tag == BidAddrTag::Validator as u8 => AccountHash::from_bytes(remainder)
                .map(|(account_hash, remainder)| (BidAddr::Validator(account_hash), remainder)),
            tag if tag == BidAddrTag::Delegator as u8 => {
                let (validator, remainder) = AccountHash::from_bytes(remainder)?;
                let (delegator, remainder) = AccountHash::from_bytes(remainder)?;
                Ok((
                    BidAddr::Delegator {
                        validator,
                        delegator,
                    },
                    remainder,
                ))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

impl Default for BidAddr {
    fn default() -> Self {
        BidAddr::Validator(AccountHash::default())
    }
}

impl From<BidAddr> for Key {
    fn from(bid_addr: BidAddr) -> Self {
        Key::BidAddr(bid_addr)
    }
}

impl From<AccountHash> for BidAddr {
    fn from(account_hash: AccountHash) -> Self {
        BidAddr::Validator(account_hash)
    }
}

impl From<PublicKey> for BidAddr {
    fn from(public_key: PublicKey) -> Self {
        BidAddr::Validator(public_key.to_account_hash())
    }
}

impl Display for BidAddr {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let tag = self.tag();
        match self {
            BidAddr::Unified(account_hash) | BidAddr::Validator(account_hash) => {
                write!(f, "{}{}", tag, account_hash)
            }
            BidAddr::Delegator {
                validator,
                delegator,
            } => write!(f, "{}{}{}", tag, validator, delegator),
        }
    }
}

impl Debug for BidAddr {
    fn fmt(&self, f: &mut Formatter) -> core::fmt::Result {
        match self {
            BidAddr::Unified(validator) => write!(f, "BidAddr::Unified({:?})", validator),
            BidAddr::Validator(validator) => write!(f, "BidAddr::Validator({:?})", validator),
            BidAddr::Delegator {
                validator,
                delegator,
            } => {
                write!(f, "BidAddr::Delegator({:?}{:?})", validator, delegator)
            }
        }
    }
}

impl Distribution<BidAddr> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> BidAddr {
        BidAddr::Validator(AccountHash::new(rng.gen()))
    }
}

#[cfg(test)]
mod tests {
    use crate::{bytesrepr, system::auction::BidAddr};

    #[test]
    fn serialization_roundtrip() {
        let bid_addr = BidAddr::legacy([1; 32]);
        bytesrepr::test_serialization_roundtrip(&bid_addr);
        let bid_addr = BidAddr::new_validator_addr([1; 32]);
        bytesrepr::test_serialization_roundtrip(&bid_addr);
        let bid_addr = BidAddr::new_delegator_addr(([1; 32], [2; 32]));
        bytesrepr::test_serialization_roundtrip(&bid_addr);
    }
}

#[cfg(test)]
mod prop_test_validator_addr {
    use proptest::prelude::*;

    use crate::{bytesrepr, gens};

    proptest! {
        #[test]
        fn test_value_bid_addr_validator(validator_bid_addr in gens::bid_addr_validator_arb()) {
            bytesrepr::test_serialization_roundtrip(&validator_bid_addr);
        }
    }
}

#[cfg(test)]
mod prop_test_delegator_addr {
    use proptest::prelude::*;

    use crate::{bytesrepr, gens};

    proptest! {
        #[test]
        fn test_value_bid_addr_delegator(delegator_bid_addr in gens::bid_addr_delegator_arb()) {
            bytesrepr::test_serialization_roundtrip(&delegator_bid_addr);
        }
    }
}
