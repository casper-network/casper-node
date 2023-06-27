//! Contains implementation of a Auction contract functionality.
mod bid;
mod constants;
mod delegator;
mod entry_points;
mod era_info;
mod error;
mod seigniorage_recipient;
mod unbonding_purse;
mod validator_bid;
mod withdraw_purse;

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;

use alloc::{boxed::Box, collections::BTreeMap, vec::Vec};
use core::fmt::{Debug, Display, Formatter};
use rand::{
    distributions::{Distribution, Standard},
    Rng,
};
use serde::{Deserialize, Serialize};

pub use bid::{Bid, VESTING_SCHEDULE_LENGTH_MILLIS};
pub use constants::*;
pub use delegator::Delegator;
pub use entry_points::auction_entry_points;
pub use era_info::{EraInfo, SeigniorageAllocation};
pub use error::Error;
pub use seigniorage_recipient::SeigniorageRecipient;
pub use unbonding_purse::UnbondingPurse;
pub use validator_bid::ValidatorBid;
pub use withdraw_purse::WithdrawPurse;

#[cfg(any(feature = "testing", test))]
pub(crate) mod gens {
    pub use super::era_info::gens::*;
}

use crate::{
    account::{AccountHash, ACCOUNT_HASH_LENGTH},
    bytesrepr,
    bytesrepr::{FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    EraId, PublicKey, U512,
};

/// Representation of delegation rate of tokens. Range from 0..=100.
pub type DelegationRate = u8;

/// Validators mapped to their bids.
pub type Bids = BTreeMap<PublicKey, Bid>;

/// Weights of validators. "Weight" in this context means a sum of their stakes.
pub type ValidatorWeights = BTreeMap<PublicKey, U512>;

/// List of era validators
pub type EraValidators = BTreeMap<EraId, ValidatorWeights>;

/// Collection of seigniorage recipients.
pub type SeigniorageRecipients = BTreeMap<PublicKey, SeigniorageRecipient>;

/// Snapshot of `SeigniorageRecipients` for a given era.
pub type SeigniorageRecipientsSnapshot = BTreeMap<EraId, SeigniorageRecipients>;

/// Validators and delegators mapped to their unbonding purses.
pub type UnbondingPurses = BTreeMap<AccountHash, Vec<UnbondingPurse>>;

/// Validators and delegators mapped to their withdraw purses.
pub type WithdrawPurses = BTreeMap<AccountHash, Vec<WithdrawPurse>>;

#[allow(missing_docs)]
#[derive(Debug, Copy, Clone, PartialOrd, Ord, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum BidAddrTag {
    Validator = 0,
    Delegator = 1,
}

/// Bid Address
#[derive(Default, PartialOrd, Ord, PartialEq, Eq, Hash, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct BidAddr(pub AccountHash, pub Option<AccountHash>);

impl BidAddr {
    /// The length in bytes of a [`BidAddr`] for a validator bid.
    pub const VALIDATOR_BID_ADDR_LENGTH: usize = ACCOUNT_HASH_LENGTH;

    /// The length in bytes of a [`BidAddr`] for a delegator bid.
    pub const DELEGATOR_BID_ADDR_LENGTH: usize = ACCOUNT_HASH_LENGTH * 2;

    /// Constructs a new [`BidAddr`] instance from a validator's [`AccountHash`].
    pub const fn new_validator_addr(validator: [u8; ACCOUNT_HASH_LENGTH]) -> Self {
        BidAddr(AccountHash::new(validator), None)
    }

    /// Constructs a new [`BidAddr`] instance from the [`AccountHash`] pair of a validator
    /// and a delegator.
    pub const fn new_delegator_addr(
        pair: ([u8; ACCOUNT_HASH_LENGTH], [u8; ACCOUNT_HASH_LENGTH]),
    ) -> Self {
        BidAddr(AccountHash::new(pair.0), Some(AccountHash::new(pair.1)))
    }

    /// Validator account hash.
    pub fn validator_account_hash(&self) -> AccountHash {
        self.0
    }

    /// Delegator account hash or none.
    pub fn maybe_delegator_account_hash(&self) -> Option<AccountHash> {
        self.1
    }

    /// If true, this instance is the key for a delegator bid record.
    /// Else, it is the key for a validator bid record.
    pub fn is_delegator_bid_addr(&self) -> bool {
        self.1.is_some()
    }

    /// How long will be the serialized value for this instance.
    pub fn serialized_length(&self) -> usize {
        if self.1.is_some() {
            self.1.serialized_length() + self.0.serialized_length() + 1
        } else {
            self.0.serialized_length() + 1
        }
    }
}

impl ToBytes for BidAddr {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        let mut serialized_data = self.validator_account_hash().to_bytes()?;
        if let Some(delegator) = self.1 {
            serialized_data.append(&mut delegator.to_bytes()?)
        }
        result.append(&mut serialized_data);
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.serialized_length()
    }
}

impl FromBytes for BidAddr {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder): (u8, &[u8]) = FromBytes::from_bytes(bytes)?;
        match tag {
            tag if tag == BidAddrTag::Validator as u8 => AccountHash::from_bytes(remainder)
                .map(|(account_hash, remainder)| (BidAddr(account_hash, None), remainder)),
            tag if tag == BidAddrTag::Delegator as u8 => {
                let (validator, remainder) = AccountHash::from_bytes(remainder)?;
                let (delegator, remainder) = AccountHash::from_bytes(remainder)?;
                Ok((BidAddr(validator, Some(delegator)), remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}
//
// impl TryFrom<[u8; BidAddr::VALIDATOR_BID_ADDR_LENGTH]> for BidAddr {
//     type Error = ();
//
//     fn try_from(value: [u8; BidAddr::VALIDATOR_BID_ADDR_LENGTH]) -> Result<Self, Self::Error> {
//         println!("{}.tryfrom", 3);
//         let bytes = <[u8; ACCOUNT_HASH_LENGTH]>::try_from(value.as_ref()).map_err(|_| ())?;
//         Ok(BidAddr(AccountHash::new(bytes), None))
//     }
// }
//
// impl TryFrom<[u8; BidAddr::DELEGATOR_BID_ADDR_LENGTH]> for BidAddr {
//     type Error = String;
//
//     fn try_from(value: [u8; BidAddr::DELEGATOR_BID_ADDR_LENGTH]) -> Result<Self, Self::Error> {
//         println!("{}.tryfrom", 2);
//         let validator_bytes = <[u8; ACCOUNT_HASH_LENGTH]>::try_from(
//             value[0..BidAddr::VALIDATOR_BID_ADDR_LENGTH].as_ref(),
//         )
//         .map_err(|error| error.to_string())?;
//         let delegator_bytes = <[u8; ACCOUNT_HASH_LENGTH]>::try_from(
//             value[BidAddr::VALIDATOR_BID_ADDR_LENGTH..].as_ref(),
//         )
//         .map_err(|error| error.to_string())?;
//         Ok(BidAddr(
//             AccountHash::new(validator_bytes),
//             Some(AccountHash::new(delegator_bytes)),
//         ))
//     }
// }

impl Display for BidAddr {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self.1 {
            Some(delegator) => write!(f, "{}{}", self.0, delegator),
            None => write!(f, "{}", self.0),
        }
    }
}

impl Debug for BidAddr {
    fn fmt(&self, f: &mut Formatter) -> core::fmt::Result {
        match self.1 {
            Some(delegator) => write!(f, "Validator{}-Delegator{}", self.0, delegator),
            None => write!(f, "{:?}", self.0),
        }
    }
}

impl Distribution<BidAddr> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> BidAddr {
        BidAddr(AccountHash::new(rng.gen()), None)
    }
}

#[allow(clippy::large_enum_variant)]
#[repr(u8)]
enum BidKindTag {
    Unified = 0,
    Validator = 1,
    Delegator = 2,
}

/// Auction bid variants.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, Clone)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum BidKind {
    /// A unified record indexed on validator data, with an embedded collection of all delegator
    /// bids assigned to that validator.
    Unified(Box<Bid>),
    /// A bid record containing only validator data.
    Validator(Box<ValidatorBid>),
    /// A bid record containing only delegator data.
    Delegator(Box<Delegator>),
}

impl BidKind {
    fn tag(&self) -> BidKindTag {
        match self {
            BidKind::Unified(_) => BidKindTag::Unified,
            BidKind::Validator(_) => BidKindTag::Validator,
            BidKind::Delegator(_) => BidKindTag::Delegator,
        }
    }
}

impl ToBytes for BidKind {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        let (tag, mut serialized_data) = match self {
            BidKind::Unified(bid) => (BidKindTag::Unified, bid.to_bytes()?),
            BidKind::Validator(validator_bid) => (BidKindTag::Validator, validator_bid.to_bytes()?),
            BidKind::Delegator(delegator_bid) => (BidKindTag::Delegator, delegator_bid.to_bytes()?),
        };
        result.push(tag as u8);
        result.append(&mut serialized_data);
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                BidKind::Unified(bid) => bid.serialized_length(),
                BidKind::Validator(validator_bid) => validator_bid.serialized_length(),
                BidKind::Delegator(delegator_bid) => delegator_bid.serialized_length(),
            }
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        writer.push(self.tag() as u8);
        match self {
            //StoredValue::CLValue(cl_value) => cl_value.write_bytes(writer)?,
            BidKind::Unified(bid) => bid.write_bytes(writer)?,
            BidKind::Validator(validator_bid) => validator_bid.write_bytes(writer)?,
            BidKind::Delegator(delegator_bid) => delegator_bid.write_bytes(writer)?,
        };
        Ok(())
    }
}

impl FromBytes for BidKind {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder): (u8, &[u8]) = FromBytes::from_bytes(bytes)?;
        match tag {
            tag if tag == BidKindTag::Unified as u8 => Bid::from_bytes(remainder)
                .map(|(bid, remainder)| (BidKind::Unified(Box::new(bid)), remainder)),
            tag if tag == BidKindTag::Validator as u8 => {
                ValidatorBid::from_bytes(remainder).map(|(validator_bid, remainder)| {
                    (BidKind::Validator(Box::new(validator_bid)), remainder)
                })
            }
            tag if tag == BidKindTag::Delegator as u8 => {
                Delegator::from_bytes(remainder).map(|(delegator_bid, remainder)| {
                    (BidKind::Delegator(Box::new(delegator_bid)), remainder)
                })
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}
