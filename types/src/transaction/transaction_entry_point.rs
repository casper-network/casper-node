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
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    alloc::string::ToString,
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    system::{auction, entity, mint},
};

const CUSTOM_TAG: u8 = 0;
const TRANSFER_TAG: u8 = 1;
const ADD_BID_TAG: u8 = 2;
const WITHDRAW_BID_TAG: u8 = 3;
const DELEGATE_TAG: u8 = 4;
const UNDELEGATE_TAG: u8 = 5;
const REDELEGATE_TAG: u8 = 6;
const ACTIVATE_BID_TAG: u8 = 7;
const CHANGE_BID_PUBLIC_KEY_TAG: u8 = 8;
const ADD_ASSOCIATED_KEY_TAG: u8 = 9;
const REMOVE_ASSOCIATED_KEY_TAG: u8 = 10;
const UPDATE_ASSOCIATED_KEY_TAG: u8 = 11;

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

    /// The `activate_bid` native entry point, used to reactivate an inactive bid.
    ///
    /// Requires the following runtime args:
    ///   * "validator_public_key": `PublicKey`
    #[cfg_attr(
        feature = "json-schema",
        schemars(
            description = "The `activate_bid` native entry point, used to reactivate an \
            inactive bid."
        )
    )]
    ActivateBid,

    /// The `change_bid_public_key` native entry point, used to change a bid's public key.
    ///
    /// Requires the following runtime args:
    ///   * "public_key": `PublicKey`
    ///   * "new_public_key": `PublicKey`
    #[cfg_attr(
        feature = "json-schema",
        schemars(
            description = "The `change_bid_public_key` native entry point, used to change a bid's public key."
        )
    )]
    ChangeBidPublicKey,

    /// The `add_associated_key` native entry point, used to add an associated key.
    ///
    /// Requires the following runtime args:
    ///   * "account": `PublicKey`
    ///   * "weight": `Weight`
    #[cfg_attr(
        feature = "json-schema",
        schemars(description = "The `add_associated_key` native entry point, used to add an associated key.")
    )]
    AddAssociatedKey,

    /// The `remove_associated_key` native entry point, used to remove an associated key.
    ///
    /// Requires the following runtime args:
    ///   * "account": `PublicKey`
    #[cfg_attr(
        feature = "json-schema",
        schemars(description = "The `remove_associated_key` native entry point, used to remove an associated key.")
    )]
    RemoveAssociatedKey,

    /// The `update_associated_key` native entry point, used to update an associated key.
    ///
    /// Requires the following runtime args:
    ///   * "account": `PublicKey`
    ///   * "weight": `Weight`
    #[cfg_attr(
        feature = "json-schema",
        schemars(description = "The `update_associated_key` native entry point, used to update an associated key.")
    )]
    UpdateAssociatedKey,
}

impl TransactionEntryPoint {
    /// Returns a random `TransactionEntryPoint`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..11) {
            CUSTOM_TAG => Self::Custom(rng.random_string(1..21)),
            TRANSFER_TAG => Self::Transfer,
            ADD_BID_TAG => Self::AddBid,
            WITHDRAW_BID_TAG => Self::WithdrawBid,
            DELEGATE_TAG => Self::Delegate,
            UNDELEGATE_TAG => Self::Undelegate,
            REDELEGATE_TAG => Self::Redelegate,
            ACTIVATE_BID_TAG => Self::ActivateBid,
            CHANGE_BID_PUBLIC_KEY_TAG => TransactionEntryPoint::ChangeBidPublicKey,
            ADD_ASSOCIATED_KEY_TAG => Self::AddAssociatedKey,
            REMOVE_ASSOCIATED_KEY_TAG => Self::RemoveAssociatedKey,
            UPDATE_ASSOCIATED_KEY_TAG => Self::UpdateAssociatedKey,
            _ => unreachable!(),
        }
    }

    /// Does this entry point kind require holds epoch?
    pub fn requires_holds_epoch(&self) -> bool {
        match self {
            Self::AddBid
            | Self::Delegate
            | Self::Custom(_)
            | Self::Transfer => true,
            Self::WithdrawBid
            | Self::Undelegate
            | Self::Redelegate
            | Self::ActivateBid
            | Self::ChangeBidPublicKey
            | Self::AddAssociatedKey
            | Self::RemoveAssociatedKey
            | Self::UpdateAssociatedKey => false,
        }
    }
}

impl Display for TransactionEntryPoint {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            Self::Custom(entry_point) => {
                write!(formatter, "custom({entry_point})")
            }
            Self::Transfer => write!(formatter, "transfer"),
            Self::AddBid => write!(formatter, "add_bid"),
            Self::WithdrawBid => write!(formatter, "withdraw_bid"),
            Self::Delegate => write!(formatter, "delegate"),
            Self::Undelegate => write!(formatter, "undelegate"),
            Self::Redelegate => write!(formatter, "redelegate"),
            Self::ActivateBid => write!(formatter, "activate_bid"),
            Self::ChangeBidPublicKey => write!(formatter, "change_bid_public_key"),
            Self::AddAssociatedKey => write!(formatter, "add_associated_key"),
            Self::RemoveAssociatedKey => write!(formatter, "remove_associated_key"),
            Self::UpdateAssociatedKey => write!(formatter, "update_associated_key"),
        }
    }
}

impl ToBytes for TransactionEntryPoint {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            Self::Custom(entry_point) => {
                CUSTOM_TAG.write_bytes(writer)?;
                entry_point.write_bytes(writer)
            }
            Self::Transfer => TRANSFER_TAG.write_bytes(writer),
            Self::AddBid => ADD_BID_TAG.write_bytes(writer),
            Self::WithdrawBid => WITHDRAW_BID_TAG.write_bytes(writer),
            Self::Delegate => DELEGATE_TAG.write_bytes(writer),
            Self::Undelegate => UNDELEGATE_TAG.write_bytes(writer),
            Self::Redelegate => REDELEGATE_TAG.write_bytes(writer),
            Self::ActivateBid => ACTIVATE_BID_TAG.write_bytes(writer),
            Self::ChangeBidPublicKey =>  CHANGE_BID_PUBLIC_KEY_TAG.write_bytes(writer),
            Self::AddAssociatedKey => ADD_ASSOCIATED_KEY_TAG.write_bytes(writer),
            Self::RemoveAssociatedKey => REMOVE_ASSOCIATED_KEY_TAG.write_bytes(writer),
            Self::UpdateAssociatedKey => UPDATE_ASSOCIATED_KEY_TAG.write_bytes(writer),
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
                Self::Custom(entry_point) => entry_point.serialized_length(),
                Self::Transfer
                | Self::AddBid
                | Self::WithdrawBid
                | Self::Delegate
                | Self::Undelegate
                | Self::Redelegate
                | Self::ActivateBid
                | Self::ChangeBidPublicKey
                | Self::AddAssociatedKey
                | Self::RemoveAssociatedKey 
                | Self::UpdateAssociatedKey  => 0,
            }
    }
}

impl FromBytes for TransactionEntryPoint {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            CUSTOM_TAG => {
                let (entry_point, remainder) = String::from_bytes(remainder)?;
                Ok((Self::Custom(entry_point), remainder))
            }
            TRANSFER_TAG => Ok((Self::Transfer, remainder)),
            ADD_BID_TAG => Ok((Self::AddBid, remainder)),
            WITHDRAW_BID_TAG => Ok((Self::WithdrawBid, remainder)),
            DELEGATE_TAG => Ok((Self::Delegate, remainder)),
            UNDELEGATE_TAG => Ok((Self::Undelegate, remainder)),
            REDELEGATE_TAG => Ok((Self::Redelegate, remainder)),
            ACTIVATE_BID_TAG => Ok((Self::ActivateBid, remainder)),
            CHANGE_BID_PUBLIC_KEY_TAG => Ok((TransactionEntryPoint::ChangeBidPublicKey, remainder)),
            ADD_ASSOCIATED_KEY_TAG => Ok((Self::AddAssociatedKey, remainder)),
            REMOVE_ASSOCIATED_KEY_TAG => Ok((Self::RemoveAssociatedKey, remainder)),
            UPDATE_ASSOCIATED_KEY_TAG => Ok((Self::UpdateAssociatedKey, remainder)),
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

impl From<&str> for TransactionEntryPoint {
    fn from(value: &str) -> Self {
        match value.to_lowercase().as_ref() {
            mint::METHOD_TRANSFER => Self::Transfer,
            auction::METHOD_ACTIVATE_BID => Self::ActivateBid,
            auction::METHOD_ADD_BID => Self::AddBid,
            auction::METHOD_WITHDRAW_BID => Self::WithdrawBid,
            auction::METHOD_DELEGATE => Self::Delegate,
            auction::METHOD_UNDELEGATE => Self::Undelegate,
            auction::METHOD_REDELEGATE => Self::Redelegate,
            auction::METHOD_CHANGE_BID_PUBLIC_KEY => Self::ChangeBidPublicKey,
            entity::METHOD_ADD_ASSOCIATED_KEY => Self::AddAssociatedKey,
            entity::METHOD_REMOVE_ASSOCIATED_KEY => Self::RemoveAssociatedKey,
            entity::METHOD_UPDATE_ASSOCIATED_KEY => Self::UpdateAssociatedKey,
            _ => Self::Custom(value.to_string()),
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
