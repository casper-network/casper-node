use alloc::{string::String, vec::Vec};
use core::fmt::{self, Display, Formatter};

#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    alloc::string::ToString,
    bytesrepr::{
        Error::{self, Formatting},
        FromBytes, ToBytes,
    },
    system::{auction, mint},
    transaction::serialization::{
        CalltableSerializationEnvelope, CalltableSerializationEnvelopeBuilder,
    },
};
#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The entry point of a [`crate::Transaction`].
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "Entry point of a Transaction.")
)]
#[serde(deny_unknown_fields)]
pub enum TransactionEntryPoint {
    /// The standard `call` entry point used in session code.
    Call,
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
    ///   * "minimum_delegation_amount": `u64`
    ///   * "maximum_delegation_amount": `u64`
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

    /// The `activate bid` native entry point, used to reactivate an inactive bid.
    ///
    /// Requires the following runtime args:
    ///   * "validator_public_key": `PublicKey`
    #[cfg_attr(
        feature = "json-schema",
        schemars(
            description = "The `activate_bid` native entry point, used to used to reactivate an \
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
    /// The `add_reservations` native entry point, used to add delegators to validator's reserve
    /// list.
    ///
    /// Requires the following runtime args:
    ///   * "validator": `PublicKey`
    ///   * "delegators": `&[PublicKey]`
    #[cfg_attr(
        feature = "json-schema",
        schemars(
            description = "The `add_reservations` native entry point, used to add delegator to \
            validator's reserve list"
        )
    )]
    AddReservations,
    /// The `cancel_reservations` native entry point, used to remove delegators from validator's
    /// reserve list.
    ///
    /// Requires the following runtime args:
    ///   * "validator": `PublicKey`
    ///   * "delegators": `&[PublicKey]`
    #[cfg_attr(
        feature = "json-schema",
        schemars(
            description = "The `cancel_reservations` native entry point, used to remove delegator \
            from validator's reserve list"
        )
    )]
    CancelReservations,
}

impl TransactionEntryPoint {
    /// Returns a random `TransactionEntryPoint`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..12) {
            0 => TransactionEntryPoint::Custom(rng.random_string(1..21)),
            1 => TransactionEntryPoint::Transfer,
            2 => TransactionEntryPoint::AddBid,
            3 => TransactionEntryPoint::WithdrawBid,
            4 => TransactionEntryPoint::Delegate,
            5 => TransactionEntryPoint::Undelegate,
            6 => TransactionEntryPoint::Redelegate,
            7 => TransactionEntryPoint::ActivateBid,
            8 => TransactionEntryPoint::ChangeBidPublicKey,
            9 => TransactionEntryPoint::Call,
            10 => TransactionEntryPoint::AddReservations,
            11 => TransactionEntryPoint::CancelReservations,
            _ => unreachable!(),
        }
    }

    /// Does this entry point kind require holds epoch?
    pub fn requires_holds_epoch(&self) -> bool {
        match self {
            TransactionEntryPoint::AddBid
            | TransactionEntryPoint::Delegate
            | TransactionEntryPoint::Custom(_)
            | TransactionEntryPoint::Call
            | TransactionEntryPoint::Transfer => true,
            TransactionEntryPoint::WithdrawBid
            | TransactionEntryPoint::Undelegate
            | TransactionEntryPoint::Redelegate
            | TransactionEntryPoint::ActivateBid
            | TransactionEntryPoint::ChangeBidPublicKey
            | TransactionEntryPoint::AddReservations
            | TransactionEntryPoint::CancelReservations => false,
        }
    }

    fn serialized_field_lengths(&self) -> Vec<usize> {
        match self {
            TransactionEntryPoint::Custom(custom) => {
                vec![
                    crate::bytesrepr::U8_SERIALIZED_LENGTH,
                    custom.serialized_length(),
                ]
            }
            TransactionEntryPoint::Call
            | TransactionEntryPoint::Transfer
            | TransactionEntryPoint::AddBid
            | TransactionEntryPoint::WithdrawBid
            | TransactionEntryPoint::Delegate
            | TransactionEntryPoint::Undelegate
            | TransactionEntryPoint::Redelegate
            | TransactionEntryPoint::ActivateBid
            | TransactionEntryPoint::ChangeBidPublicKey
            | TransactionEntryPoint::AddReservations
            | TransactionEntryPoint::CancelReservations => {
                vec![crate::bytesrepr::U8_SERIALIZED_LENGTH]
            }
        }
    }
}

const TAG_FIELD_INDEX: u16 = 0;

const CALL_VARIANT_TAG: u8 = 0;

const CUSTOM_VARIANT_TAG: u8 = 1;
const CUSTOM_CUSTOM_INDEX: u16 = 1;

const TRANSFER_VARIANT_TAG: u8 = 2;
const ADD_BID_VARIANT_TAG: u8 = 3;
const WITHDRAW_BID_VARIANT_TAG: u8 = 4;
const DELEGATE_VARIANT_TAG: u8 = 5;
const UNDELEGATE_VARIANT_TAG: u8 = 6;
const REDELEGATE_VARIANT_TAG: u8 = 7;
const ACTIVATE_BID_VARIANT_TAG: u8 = 8;
const CHANGE_BID_PUBLIC_KEY_VARIANT_TAG: u8 = 9;
const ADD_RESERVATIONS_VARIANT_TAG: u8 = 10;
const CANCEL_RESERVATIONS_VARIANT_TAG: u8 = 11;

impl ToBytes for TransactionEntryPoint {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        match self {
            TransactionEntryPoint::Call => {
                CalltableSerializationEnvelopeBuilder::new(self.serialized_field_lengths())?
                    .add_field(TAG_FIELD_INDEX, &CALL_VARIANT_TAG)?
                    .binary_payload_bytes()
            }
            TransactionEntryPoint::Custom(custom) => {
                CalltableSerializationEnvelopeBuilder::new(self.serialized_field_lengths())?
                    .add_field(TAG_FIELD_INDEX, &CUSTOM_VARIANT_TAG)?
                    .add_field(CUSTOM_CUSTOM_INDEX, &custom)?
                    .binary_payload_bytes()
            }
            TransactionEntryPoint::Transfer => {
                CalltableSerializationEnvelopeBuilder::new(self.serialized_field_lengths())?
                    .add_field(TAG_FIELD_INDEX, &TRANSFER_VARIANT_TAG)?
                    .binary_payload_bytes()
            }
            TransactionEntryPoint::AddBid => {
                CalltableSerializationEnvelopeBuilder::new(self.serialized_field_lengths())?
                    .add_field(TAG_FIELD_INDEX, &ADD_BID_VARIANT_TAG)?
                    .binary_payload_bytes()
            }
            TransactionEntryPoint::WithdrawBid => {
                CalltableSerializationEnvelopeBuilder::new(self.serialized_field_lengths())?
                    .add_field(TAG_FIELD_INDEX, &WITHDRAW_BID_VARIANT_TAG)?
                    .binary_payload_bytes()
            }
            TransactionEntryPoint::Delegate => {
                CalltableSerializationEnvelopeBuilder::new(self.serialized_field_lengths())?
                    .add_field(TAG_FIELD_INDEX, &DELEGATE_VARIANT_TAG)?
                    .binary_payload_bytes()
            }
            TransactionEntryPoint::Undelegate => {
                CalltableSerializationEnvelopeBuilder::new(self.serialized_field_lengths())?
                    .add_field(TAG_FIELD_INDEX, &UNDELEGATE_VARIANT_TAG)?
                    .binary_payload_bytes()
            }
            TransactionEntryPoint::Redelegate => {
                CalltableSerializationEnvelopeBuilder::new(self.serialized_field_lengths())?
                    .add_field(TAG_FIELD_INDEX, &REDELEGATE_VARIANT_TAG)?
                    .binary_payload_bytes()
            }
            TransactionEntryPoint::ActivateBid => {
                CalltableSerializationEnvelopeBuilder::new(self.serialized_field_lengths())?
                    .add_field(TAG_FIELD_INDEX, &ACTIVATE_BID_VARIANT_TAG)?
                    .binary_payload_bytes()
            }
            TransactionEntryPoint::ChangeBidPublicKey => {
                CalltableSerializationEnvelopeBuilder::new(self.serialized_field_lengths())?
                    .add_field(TAG_FIELD_INDEX, &CHANGE_BID_PUBLIC_KEY_VARIANT_TAG)?
                    .binary_payload_bytes()
            }
            TransactionEntryPoint::AddReservations => {
                CalltableSerializationEnvelopeBuilder::new(self.serialized_field_lengths())?
                    .add_field(TAG_FIELD_INDEX, &ADD_RESERVATIONS_VARIANT_TAG)?
                    .binary_payload_bytes()
            }
            TransactionEntryPoint::CancelReservations => {
                CalltableSerializationEnvelopeBuilder::new(self.serialized_field_lengths())?
                    .add_field(TAG_FIELD_INDEX, &CANCEL_RESERVATIONS_VARIANT_TAG)?
                    .binary_payload_bytes()
            }
        }
    }
    fn serialized_length(&self) -> usize {
        CalltableSerializationEnvelope::estimate_size(self.serialized_field_lengths())
    }
}

impl FromBytes for TransactionEntryPoint {
    fn from_bytes(bytes: &[u8]) -> Result<(TransactionEntryPoint, &[u8]), Error> {
        let (binary_payload, remainder) = CalltableSerializationEnvelope::from_bytes(2u32, bytes)?;
        let window = binary_payload.start_consuming()?.ok_or(Formatting)?;
        window.verify_index(TAG_FIELD_INDEX)?;
        let (tag, window) = window.deserialize_and_maybe_next::<u8>()?;
        let to_ret = match tag {
            CALL_VARIANT_TAG => {
                if window.is_some() {
                    return Err(Formatting);
                }
                Ok(TransactionEntryPoint::Call)
            }
            CUSTOM_VARIANT_TAG => {
                let window = window.ok_or(Formatting)?;
                window.verify_index(CUSTOM_CUSTOM_INDEX)?;
                let (custom, window) = window.deserialize_and_maybe_next::<String>()?;
                if window.is_some() {
                    return Err(Formatting);
                }
                Ok(TransactionEntryPoint::Custom(custom))
            }
            TRANSFER_VARIANT_TAG => {
                if window.is_some() {
                    return Err(Formatting);
                }
                Ok(TransactionEntryPoint::Transfer)
            }
            ADD_BID_VARIANT_TAG => {
                if window.is_some() {
                    return Err(Formatting);
                }
                Ok(TransactionEntryPoint::AddBid)
            }
            WITHDRAW_BID_VARIANT_TAG => {
                if window.is_some() {
                    return Err(Formatting);
                }
                Ok(TransactionEntryPoint::WithdrawBid)
            }
            DELEGATE_VARIANT_TAG => {
                if window.is_some() {
                    return Err(Formatting);
                }
                Ok(TransactionEntryPoint::Delegate)
            }
            UNDELEGATE_VARIANT_TAG => {
                if window.is_some() {
                    return Err(Formatting);
                }
                Ok(TransactionEntryPoint::Undelegate)
            }
            REDELEGATE_VARIANT_TAG => {
                if window.is_some() {
                    return Err(Formatting);
                }
                Ok(TransactionEntryPoint::Redelegate)
            }
            ACTIVATE_BID_VARIANT_TAG => {
                if window.is_some() {
                    return Err(Formatting);
                }
                Ok(TransactionEntryPoint::ActivateBid)
            }
            CHANGE_BID_PUBLIC_KEY_VARIANT_TAG => {
                if window.is_some() {
                    return Err(Formatting);
                }
                Ok(TransactionEntryPoint::ChangeBidPublicKey)
            }
            ADD_RESERVATIONS_VARIANT_TAG => {
                if window.is_some() {
                    return Err(Formatting);
                }
                Ok(TransactionEntryPoint::AddReservations)
            }
            CANCEL_RESERVATIONS_VARIANT_TAG => {
                if window.is_some() {
                    return Err(Formatting);
                }
                Ok(TransactionEntryPoint::CancelReservations)
            }
            _ => Err(Formatting),
        };
        to_ret.map(|endpoint| (endpoint, remainder))
    }
}

impl Display for TransactionEntryPoint {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            TransactionEntryPoint::Call => write!(formatter, "call"),
            TransactionEntryPoint::Custom(entry_point) => {
                write!(formatter, "custom({entry_point})")
            }
            TransactionEntryPoint::Transfer => write!(formatter, "transfer"),
            TransactionEntryPoint::AddBid => write!(formatter, "add_bid"),
            TransactionEntryPoint::WithdrawBid => write!(formatter, "withdraw_bid"),
            TransactionEntryPoint::Delegate => write!(formatter, "delegate"),
            TransactionEntryPoint::Undelegate => write!(formatter, "undelegate"),
            TransactionEntryPoint::Redelegate => write!(formatter, "redelegate"),
            TransactionEntryPoint::ActivateBid => write!(formatter, "activate_bid"),
            TransactionEntryPoint::ChangeBidPublicKey => write!(formatter, "change_bid_public_key"),
            TransactionEntryPoint::AddReservations => write!(formatter, "add_reservations"),
            TransactionEntryPoint::CancelReservations => write!(formatter, "cancel_reservations"),
        }
    }
}

impl From<&str> for TransactionEntryPoint {
    fn from(value: &str) -> Self {
        if value.to_lowercase() == mint::METHOD_TRANSFER {
            return TransactionEntryPoint::Transfer;
        }
        if value.to_lowercase() == auction::METHOD_ACTIVATE_BID {
            return TransactionEntryPoint::ActivateBid;
        }
        if value.to_lowercase() == auction::METHOD_ADD_BID {
            return TransactionEntryPoint::AddBid;
        }
        if value.to_lowercase() == auction::METHOD_WITHDRAW_BID {
            return TransactionEntryPoint::WithdrawBid;
        }
        if value.to_lowercase() == auction::METHOD_DELEGATE {
            return TransactionEntryPoint::Delegate;
        }
        if value.to_lowercase() == auction::METHOD_UNDELEGATE {
            return TransactionEntryPoint::Undelegate;
        }
        if value.to_lowercase() == auction::METHOD_REDELEGATE {
            return TransactionEntryPoint::Redelegate;
        }
        if value.to_lowercase() == auction::METHOD_CHANGE_BID_PUBLIC_KEY {
            return TransactionEntryPoint::ChangeBidPublicKey;
        }
        TransactionEntryPoint::Custom(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bytesrepr::test_serialization_roundtrip, gens::transaction_entry_point_arb};
    use proptest::prelude::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        for _ in 0..10 {
            test_serialization_roundtrip(&TransactionEntryPoint::random(rng));
        }
    }

    proptest! {
        #[test]
        fn bytesrepr_roundtrip_from_arb(entry_point in transaction_entry_point_arb()) {
            test_serialization_roundtrip(&entry_point);
        }
    }
}
