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
use super::TransactionV1;
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

/// A [`TransactionV1`] targeting the auction.
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "A TransactionV1 targeting the auction.")
)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub enum AuctionTransactionV1 {
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

impl AuctionTransactionV1 {
    /// Returns the runtime arguments.
    pub fn args(&self) -> &RuntimeArgs {
        match self {
            AuctionTransactionV1::AddBid(args)
            | AuctionTransactionV1::WithdrawBid(args)
            | AuctionTransactionV1::Delegate(args)
            | AuctionTransactionV1::Undelegate(args)
            | AuctionTransactionV1::Redelegate(args)
            | AuctionTransactionV1::GetEraValidators(args)
            | AuctionTransactionV1::ReadEraId(args) => args,
        }
    }

    /// Returns a random `AuctionTransactionV1`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..7) {
            0 => AuctionTransactionV1::AddBid(RuntimeArgs::random(rng)),
            1 => AuctionTransactionV1::WithdrawBid(RuntimeArgs::random(rng)),
            2 => AuctionTransactionV1::Delegate(RuntimeArgs::random(rng)),
            3 => AuctionTransactionV1::Undelegate(RuntimeArgs::random(rng)),
            4 => AuctionTransactionV1::Redelegate(RuntimeArgs::random(rng)),
            5 => AuctionTransactionV1::GetEraValidators(RuntimeArgs::random(rng)),
            6 => AuctionTransactionV1::ReadEraId(RuntimeArgs::random(rng)),
            _ => unreachable!(),
        }
    }
}

impl Display for AuctionTransactionV1 {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            AuctionTransactionV1::AddBid(_) => write!(formatter, "auction add bid"),
            AuctionTransactionV1::WithdrawBid(_) => write!(formatter, "auction withdraw bid"),
            AuctionTransactionV1::Delegate(_) => write!(formatter, "auction delegate"),
            AuctionTransactionV1::Undelegate(_) => write!(formatter, "auction undelegate"),
            AuctionTransactionV1::Redelegate(_) => write!(formatter, "auction redelegate"),
            AuctionTransactionV1::GetEraValidators(_) => {
                write!(formatter, "auction get era validators")
            }
            AuctionTransactionV1::ReadEraId(_) => write!(formatter, "auction read era id"),
        }
    }
}

impl ToBytes for AuctionTransactionV1 {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            AuctionTransactionV1::AddBid(args) => {
                ADD_BID_TAG.write_bytes(writer)?;
                args.write_bytes(writer)
            }
            AuctionTransactionV1::WithdrawBid(args) => {
                WITHDRAW_BID_TAG.write_bytes(writer)?;
                args.write_bytes(writer)
            }
            AuctionTransactionV1::Delegate(args) => {
                DELEGATE_TAG.write_bytes(writer)?;
                args.write_bytes(writer)
            }
            AuctionTransactionV1::Undelegate(args) => {
                UNDELEGATE_TAG.write_bytes(writer)?;
                args.write_bytes(writer)
            }
            AuctionTransactionV1::Redelegate(args) => {
                REDELEGATE_TAG.write_bytes(writer)?;
                args.write_bytes(writer)
            }
            AuctionTransactionV1::GetEraValidators(args) => {
                GET_ERA_VALIDATORS_TAG.write_bytes(writer)?;
                args.write_bytes(writer)
            }
            AuctionTransactionV1::ReadEraId(args) => {
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
                AuctionTransactionV1::AddBid(args)
                | AuctionTransactionV1::WithdrawBid(args)
                | AuctionTransactionV1::Delegate(args)
                | AuctionTransactionV1::Undelegate(args)
                | AuctionTransactionV1::Redelegate(args)
                | AuctionTransactionV1::GetEraValidators(args)
                | AuctionTransactionV1::ReadEraId(args) => args.serialized_length(),
            }
    }
}

impl FromBytes for AuctionTransactionV1 {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        let (args, remainder) = RuntimeArgs::from_bytes(remainder)?;
        match tag {
            ADD_BID_TAG => Ok((AuctionTransactionV1::AddBid(args), remainder)),
            WITHDRAW_BID_TAG => Ok((AuctionTransactionV1::WithdrawBid(args), remainder)),
            DELEGATE_TAG => Ok((AuctionTransactionV1::Delegate(args), remainder)),
            UNDELEGATE_TAG => Ok((AuctionTransactionV1::Undelegate(args), remainder)),
            REDELEGATE_TAG => Ok((AuctionTransactionV1::Redelegate(args), remainder)),
            GET_ERA_VALIDATORS_TAG => Ok((AuctionTransactionV1::GetEraValidators(args), remainder)),
            READ_ERA_ID_TAG => Ok((AuctionTransactionV1::ReadEraId(args), remainder)),
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
            bytesrepr::test_serialization_roundtrip(&AuctionTransactionV1::random(rng));
        }
    }
}
