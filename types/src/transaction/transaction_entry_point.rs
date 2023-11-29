use alloc::{string::String, vec::Vec};
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
use crate::bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH};
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;

const CUSTOM_TAG: u8 = 0;
const TRANSFER_TAG: u8 = 1;
const ADD_BID_TAG: u8 = 2;
const WITHDRAW_BID_TAG: u8 = 3;
const DELEGATE_TAG: u8 = 4;
const UNDELEGATE_TAG: u8 = 5;
const REDELEGATE_TAG: u8 = 6;

/// The entry point of a [`Transaction`].
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "Entry point of a Transaction.")
)]
#[serde(deny_unknown_fields)]
pub enum TransactionEntryPoint {
    /// A non-native, arbitrary entry point.
    Custom(String),
    /// The `transfer` native entry point, used to transfer `Motes` from a source purse to a target
    /// purse.
    ///
    /// Requires the following runtime args:
    ///   * "source": `URef`
    ///   * "target": `URef`
    ///   * "amount": `U512`
    ///
    /// The following optional runtime args can also be provided:
    ///   * "to": `Option<AccountHash>`
    ///   * "id": `Option<u64>`
    #[cfg_attr(
        feature = "json-schema",
        schemars(
            description = "The `transfer` native entry point, used to transfer `Motes` from a \
            source purse to a target purse."
        )
    )]
    Transfer,
    /// The `add_bid` native entry point, used to create or top off a bid purse.
    ///
    /// Requires the following runtime args:
    ///   * "public_key": `PublicKey`
    ///   * "delegation_rate": `u8`
    ///   * "amount": `U512`
    #[cfg_attr(
        feature = "json-schema",
        schemars(
            description = "The `add_bid` native entry point, used to create or top off a bid purse."
        )
    )]
    AddBid,
    /// The `withdraw_bid` native entry point, used to decrease a stake.
    ///
    /// Requires the following runtime args:
    ///   * "public_key": `PublicKey`
    ///   * "amount": `U512`
    #[cfg_attr(
        feature = "json-schema",
        schemars(description = "The `withdraw_bid` native entry point, used to decrease a stake.")
    )]
    WithdrawBid,

    /// The `delegate` native entry point, used to add a new delegator or increase an existing
    /// delegator's stake.
    ///
    /// Requires the following runtime args:
    ///   * "delegator": `PublicKey`
    ///   * "validator": `PublicKey`
    ///   * "amount": `U512`
    #[cfg_attr(
        feature = "json-schema",
        schemars(
            description = "The `delegate` native entry point, used to add a new delegator or \
            increase an existing delegator's stake."
        )
    )]
    Delegate,

    /// The `undelegate` native entry point, used to reduce a delegator's stake or remove the
    /// delegator if the remaining stake is 0.
    ///
    /// Requires the following runtime args:
    ///   * "delegator": `PublicKey`
    ///   * "validator": `PublicKey`
    ///   * "amount": `U512`
    #[cfg_attr(
        feature = "json-schema",
        schemars(
            description = "The `undelegate` native entry point, used to reduce a delegator's \
            stake or remove the delegator if the remaining stake is 0."
        )
    )]
    Undelegate,

    /// The `redelegate` native entry point, used to reduce a delegator's stake or remove the
    /// delegator if the remaining stake is 0, and after the unbonding delay, automatically
    /// delegate to a new validator.
    ///
    /// Requires the following runtime args:
    ///   * "delegator": `PublicKey`
    ///   * "validator": `PublicKey`
    ///   * "amount": `U512`
    ///   * "new_validator": `PublicKey`
    #[cfg_attr(
        feature = "json-schema",
        schemars(
            description = "The `redelegate` native entry point, used to reduce a delegator's stake \
            or remove the delegator if the remaining stake is 0, and after the unbonding delay, \
            automatically delegate to a new validator."
        )
    )]
    Redelegate,
}

impl TransactionEntryPoint {
    /// Returns a random `TransactionEntryPoint`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..7) {
            CUSTOM_TAG => TransactionEntryPoint::Custom(rng.random_string(1..21)),
            TRANSFER_TAG => TransactionEntryPoint::Transfer,
            ADD_BID_TAG => TransactionEntryPoint::AddBid,
            WITHDRAW_BID_TAG => TransactionEntryPoint::WithdrawBid,
            DELEGATE_TAG => TransactionEntryPoint::Delegate,
            UNDELEGATE_TAG => TransactionEntryPoint::Undelegate,
            REDELEGATE_TAG => TransactionEntryPoint::Redelegate,
            _ => unreachable!(),
        }
    }
}

impl Display for TransactionEntryPoint {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            TransactionEntryPoint::Custom(entry_point) => {
                write!(formatter, "custom({entry_point})")
            }
            TransactionEntryPoint::Transfer => write!(formatter, "transfer"),
            TransactionEntryPoint::AddBid => write!(formatter, "add_bid"),
            TransactionEntryPoint::WithdrawBid => write!(formatter, "withdraw_bid"),
            TransactionEntryPoint::Delegate => write!(formatter, "delegate"),
            TransactionEntryPoint::Undelegate => write!(formatter, "undelegate"),
            TransactionEntryPoint::Redelegate => write!(formatter, "redelegate"),
        }
    }
}

impl ToBytes for TransactionEntryPoint {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            TransactionEntryPoint::Custom(entry_point) => {
                CUSTOM_TAG.write_bytes(writer)?;
                entry_point.write_bytes(writer)
            }
            TransactionEntryPoint::Transfer => TRANSFER_TAG.write_bytes(writer),
            TransactionEntryPoint::AddBid => ADD_BID_TAG.write_bytes(writer),
            TransactionEntryPoint::WithdrawBid => WITHDRAW_BID_TAG.write_bytes(writer),
            TransactionEntryPoint::Delegate => DELEGATE_TAG.write_bytes(writer),
            TransactionEntryPoint::Undelegate => UNDELEGATE_TAG.write_bytes(writer),
            TransactionEntryPoint::Redelegate => REDELEGATE_TAG.write_bytes(writer),
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
                TransactionEntryPoint::Custom(entry_point) => entry_point.serialized_length(),
                TransactionEntryPoint::Transfer
                | TransactionEntryPoint::AddBid
                | TransactionEntryPoint::WithdrawBid
                | TransactionEntryPoint::Delegate
                | TransactionEntryPoint::Undelegate
                | TransactionEntryPoint::Redelegate => 0,
            }
    }
}

impl FromBytes for TransactionEntryPoint {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            CUSTOM_TAG => {
                let (entry_point, remainder) = String::from_bytes(remainder)?;
                Ok((TransactionEntryPoint::Custom(entry_point), remainder))
            }
            TRANSFER_TAG => Ok((TransactionEntryPoint::Transfer, remainder)),
            ADD_BID_TAG => Ok((TransactionEntryPoint::AddBid, remainder)),
            WITHDRAW_BID_TAG => Ok((TransactionEntryPoint::WithdrawBid, remainder)),
            DELEGATE_TAG => Ok((TransactionEntryPoint::Delegate, remainder)),
            UNDELEGATE_TAG => Ok((TransactionEntryPoint::Undelegate, remainder)),
            REDELEGATE_TAG => Ok((TransactionEntryPoint::Redelegate, remainder)),
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
            bytesrepr::test_serialization_roundtrip(&TransactionEntryPoint::random(rng));
        }
    }
}
