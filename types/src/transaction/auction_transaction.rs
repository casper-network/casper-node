use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(doc)]
use super::Transaction;
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    RuntimeArgs,
};

const ADD_BID_TAG: u8 = 0;
const WITHDRAW_BID_TAG: u8 = 1;
const DELEGATE_TAG: u8 = 2;
const UNDELEGATE_TAG: u8 = 3;
const REDELEGATE_TAG: u8 = 4;
const GET_ERA_VALIDATORS_TAG: u8 = 5;
const READ_ERA_ID_TAG: u8 = 6;

/// A [`Transaction`] targeting the auction.
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "A Transaction targeting the auction.")
)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub enum AuctionTransaction {
    /// Calls the `add_bid` entry point to create or top off a bid purse.
    ///
    /// Requires the following runtime args:
    ///   * "public_key": `PublicKey`
    ///   * "delegation_rate": `u8`
    ///   * "amount": `U512`
    #[cfg_attr(
        feature = "json-schema",
        schemars(
            description = "Calls the `add_bid` entry point to create or top off a bid purse."
        )
    )]
    AddBid(RuntimeArgs),

    /// Calls the `withdraw_bid` entry point to decrease a stake.
    ///
    /// Requires the following runtime args:
    ///   * "public_key": `PublicKey`
    ///   * "amount": `U512`
    #[cfg_attr(
        feature = "json-schema",
        schemars(description = "Calls the `withdraw_bid` entry point to decrease a stake.")
    )]
    WithdrawBid(RuntimeArgs),

    /// Calls the `delegate` entry point to add a new delegator or increase an existing delegator's
    /// stake.
    ///
    /// Requires the following runtime args:
    ///   * "delegator": `PublicKey`
    ///   * "validator": `PublicKey`
    ///   * "amount": `U512`
    #[cfg_attr(
        feature = "json-schema",
        schemars(
            description = "Calls the `delegate` entry point to add a new delegator or increase an \
            existing delegator's stake."
        )
    )]
    Delegate(RuntimeArgs),

    /// Calls the `undelegate` entry point to reduce a delegator's stake or remove the delegator if
    /// the remaining stake is 0.
    ///
    /// Requires the following runtime args:
    ///   * "delegator": `PublicKey`
    ///   * "validator": `PublicKey`
    ///   * "amount": `U512`
    #[cfg_attr(
        feature = "json-schema",
        schemars(
            description = "Calls the `undelegate` entry point to reduce a delegator's stake or \
            remove the delegator if the remaining stake is 0."
        )
    )]
    Undelegate(RuntimeArgs),

    /// Calls the `redelegate` entry point to reduce a delegator's stake or remove the delegator if
    /// the remaining stake is 0, and after the unbonding delay, automatically delegate to a new
    /// validator.
    ///
    /// Requires the following runtime args:
    ///   * "delegator": `PublicKey`
    ///   * "validator": `PublicKey`
    ///   * "amount": `U512`
    ///   * "new_validator": `PublicKey`
    #[cfg_attr(
        feature = "json-schema",
        schemars(
            description = "Calls the `redelegate` entry point to reduce a delegator's stake or \
            remove the delegator if the remaining stake is 0, and after the unbonding delay, \
            automatically delegate to a new validator."
        )
    )]
    Redelegate(RuntimeArgs),

    /// Calls the `get_era_validators` entry point to provide the validators for the current era
    /// and configured number of future eras.
    ///
    /// Requires no runtime args.
    #[cfg_attr(
        feature = "json-schema",
        schemars(
            description = "Calls the `get_era_validators` entry point to provide the validators \
            for the current era and configured number of future eras."
        )
    )]
    GetEraValidators(RuntimeArgs),

    /// Calls the `read_era_id` entry point to provide the current `EraId`.
    ///
    /// Requires no runtime args.
    #[cfg_attr(
        feature = "json-schema",
        schemars(
            description = "Calls the `read_era_id` entry point to provide the current era ID."
        )
    )]
    ReadEraId(RuntimeArgs),
}

impl AuctionTransaction {
    /// Returns the runtime arguments.
    pub fn args(&self) -> &RuntimeArgs {
        match self {
            AuctionTransaction::AddBid(args)
            | AuctionTransaction::WithdrawBid(args)
            | AuctionTransaction::Delegate(args)
            | AuctionTransaction::Undelegate(args)
            | AuctionTransaction::Redelegate(args)
            | AuctionTransaction::GetEraValidators(args)
            | AuctionTransaction::ReadEraId(args) => args,
        }
    }

    /// Returns a random `AuctionTransaction`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..7) {
            0 => AuctionTransaction::AddBid(RuntimeArgs::random(rng)),
            1 => AuctionTransaction::WithdrawBid(RuntimeArgs::random(rng)),
            2 => AuctionTransaction::Delegate(RuntimeArgs::random(rng)),
            3 => AuctionTransaction::Undelegate(RuntimeArgs::random(rng)),
            4 => AuctionTransaction::Redelegate(RuntimeArgs::random(rng)),
            5 => AuctionTransaction::GetEraValidators(RuntimeArgs::random(rng)),
            6 => AuctionTransaction::ReadEraId(RuntimeArgs::random(rng)),
            _ => unreachable!(),
        }
    }
}

impl Display for AuctionTransaction {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            AuctionTransaction::AddBid(_) => write!(formatter, "auction add bid"),
            AuctionTransaction::WithdrawBid(_) => write!(formatter, "auction withdraw bid"),
            AuctionTransaction::Delegate(_) => write!(formatter, "auction delegate"),
            AuctionTransaction::Undelegate(_) => write!(formatter, "auction undelegate"),
            AuctionTransaction::Redelegate(_) => write!(formatter, "auction redelegate"),
            AuctionTransaction::GetEraValidators(_) => {
                write!(formatter, "auction get era validators")
            }
            AuctionTransaction::ReadEraId(_) => write!(formatter, "auction read era id"),
        }
    }
}

impl ToBytes for AuctionTransaction {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            AuctionTransaction::AddBid(args) => {
                ADD_BID_TAG.write_bytes(writer)?;
                args.write_bytes(writer)
            }
            AuctionTransaction::WithdrawBid(args) => {
                WITHDRAW_BID_TAG.write_bytes(writer)?;
                args.write_bytes(writer)
            }
            AuctionTransaction::Delegate(args) => {
                DELEGATE_TAG.write_bytes(writer)?;
                args.write_bytes(writer)
            }
            AuctionTransaction::Undelegate(args) => {
                UNDELEGATE_TAG.write_bytes(writer)?;
                args.write_bytes(writer)
            }
            AuctionTransaction::Redelegate(args) => {
                REDELEGATE_TAG.write_bytes(writer)?;
                args.write_bytes(writer)
            }
            AuctionTransaction::GetEraValidators(args) => {
                GET_ERA_VALIDATORS_TAG.write_bytes(writer)?;
                args.write_bytes(writer)
            }
            AuctionTransaction::ReadEraId(args) => {
                READ_ERA_ID_TAG.write_bytes(writer)?;
                args.write_bytes(writer)
            }
        }
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                AuctionTransaction::AddBid(args)
                | AuctionTransaction::WithdrawBid(args)
                | AuctionTransaction::Delegate(args)
                | AuctionTransaction::Undelegate(args)
                | AuctionTransaction::Redelegate(args)
                | AuctionTransaction::GetEraValidators(args)
                | AuctionTransaction::ReadEraId(args) => args.serialized_length(),
            }
    }
}

impl FromBytes for AuctionTransaction {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        let (args, remainder) = RuntimeArgs::from_bytes(remainder)?;
        match tag {
            ADD_BID_TAG => Ok((AuctionTransaction::AddBid(args), remainder)),
            WITHDRAW_BID_TAG => Ok((AuctionTransaction::WithdrawBid(args), remainder)),
            DELEGATE_TAG => Ok((AuctionTransaction::Delegate(args), remainder)),
            UNDELEGATE_TAG => Ok((AuctionTransaction::Undelegate(args), remainder)),
            REDELEGATE_TAG => Ok((AuctionTransaction::Redelegate(args), remainder)),
            GET_ERA_VALIDATORS_TAG => Ok((AuctionTransaction::GetEraValidators(args), remainder)),
            READ_ERA_ID_TAG => Ok((AuctionTransaction::ReadEraId(args), remainder)),
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        for _ in 0..10 {
            bytesrepr::test_serialization_roundtrip(&AuctionTransaction::random(rng));
        }
    }
}
