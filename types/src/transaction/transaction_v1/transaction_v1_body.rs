#[cfg(any(feature = "std", test))]
pub(super) mod arg_handling;

use alloc::{string::String, vec::Vec};
use core::fmt::{self, Display, Formatter};

use super::super::{RuntimeArgs, TransactionEntryPoint, TransactionScheduling, TransactionTarget};
#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(all(feature = "std", feature = "testing"), test))]
use rand::{Rng, RngCore};
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
#[cfg(any(feature = "std", test))]
use serde::{Deserialize, Serialize};
#[cfg(any(all(feature = "std", feature = "testing"), test))]
use tracing::debug;

#[cfg(any(all(feature = "std", feature = "testing"), test))]
use super::TransactionConfig;
use super::TransactionLane;
#[cfg(doc)]
use super::TransactionV1;
#[cfg(any(feature = "std", test))]
use crate::InvalidTransactionV1;
use crate::TransactionRuntime;

use crate::{
    bytesrepr::{self, Bytes, Error, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    transaction::serialization::{
        CalltableSerializationEnvelope, CalltableSerializationEnvelopeBuilder,
    },
    CLTyped, CLValueError,
};
#[cfg(any(all(feature = "std", feature = "testing"), test))]
use crate::{testing::TestRng, PublicKey};
/// The body of a [`TransactionV1`].
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug)]
#[cfg_attr(
    any(feature = "std", test),
    derive(Serialize, Deserialize),
    serde(deny_unknown_fields)
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "Body of a `TransactionV1`.")
)]
pub struct TransactionV1Body {
    pub(crate) args: TransactionArgs,
    pub(crate) target: TransactionTarget,
    pub(crate) entry_point: TransactionEntryPoint,
    pub(crate) transaction_lane: u8,
    pub(crate) scheduling: TransactionScheduling,
    pub(super) transferred_value: u128,
}

/// The arguments of a transaction, which can be either a named set of runtime arguments or a
/// chunked bytes.
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug)]
#[cfg_attr(
    any(feature = "std", test),
    derive(Serialize, Deserialize),
    serde(deny_unknown_fields)
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "Body of a `TransactionArgs`.")
)]
pub enum TransactionArgs {
    /// Named runtime arguments.
    Named(RuntimeArgs),
    /// Bytesrepr bytes.
    Bytesrepr(Bytes),
}

impl TransactionArgs {
    /// Returns `RuntimeArgs` if the transaction arguments are named.
    pub fn as_named(&self) -> Option<&RuntimeArgs> {
        match self {
            TransactionArgs::Named(args) => Some(args),
            TransactionArgs::Bytesrepr(_) => None,
        }
    }

    /// Returns `RuntimeArgs` if the transaction arguments are mnamed.
    pub fn into_named(self) -> Option<RuntimeArgs> {
        match self {
            TransactionArgs::Named(args) => Some(args),
            TransactionArgs::Bytesrepr(_) => None,
        }
    }

    /// Returns `Bytes` if the transaction arguments are chunked.
    pub fn into_bytesrepr(self) -> Option<Bytes> {
        match self {
            TransactionArgs::Named(_) => None,
            TransactionArgs::Bytesrepr(bytes) => Some(bytes),
        }
    }

    /// Inserts a key-value pair into the named runtime arguments.
    pub fn insert<K, V>(&mut self, key: K, value: V) -> Result<(), CLValueError>
    where
        K: Into<String>,
        V: CLTyped + ToBytes,
    {
        match self {
            TransactionArgs::Named(args) => {
                args.insert(key, value)?;
                Ok(())
            }
            TransactionArgs::Bytesrepr(_) => {
                Err(CLValueError::Serialization(bytesrepr::Error::Formatting))
            }
        }
    }
}

impl FromBytes for TransactionArgs {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            0 => {
                let (args, remainder) = RuntimeArgs::from_bytes(remainder)?;
                Ok((TransactionArgs::Named(args), remainder))
            }
            1 => {
                let (bytes, remainder) = Bytes::from_bytes(remainder)?;
                Ok((TransactionArgs::Bytesrepr(bytes), remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

impl ToBytes for TransactionArgs {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        match self {
            TransactionArgs::Named(args) => args.serialized_length() + U8_SERIALIZED_LENGTH,
            TransactionArgs::Bytesrepr(bytes) => bytes.serialized_length() + U8_SERIALIZED_LENGTH,
        }
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            TransactionArgs::Named(args) => {
                writer.push(0);
                args.write_bytes(writer)
            }
            TransactionArgs::Bytesrepr(bytes) => {
                writer.push(1);
                bytes.write_bytes(writer)
            }
        }
    }
}

impl TransactionV1Body {
    /// Returns a new `TransactionV1Body`.
    pub fn new(
        args: RuntimeArgs,
        target: TransactionTarget,
        entry_point: TransactionEntryPoint,
        transaction_lane: u8,
        scheduling: TransactionScheduling,
    ) -> Self {
        TransactionV1Body {
            args: TransactionArgs::Named(args),
            target,
            entry_point,
            transaction_lane,
            scheduling,
            transferred_value: 0,
        }
    }

    /// Returns a new `TransactionV1Body`.
    pub fn new_v2(
        args: TransactionArgs,
        target: TransactionTarget,
        entry_point: TransactionEntryPoint,
        transaction_lane: u8,
        scheduling: TransactionScheduling,
        transferred_value: u128,
    ) -> Self {
        TransactionV1Body {
            args,
            target,
            entry_point,
            transaction_lane,
            scheduling,
            transferred_value,
        }
    }

    /// Returns the runtime args of the transaction.
    pub fn args(&self) -> &TransactionArgs {
        &self.args
    }

    /// Consumes `self`, returning the runtime args of the transaction.
    pub fn take_args(self) -> TransactionArgs {
        self.args
    }

    /// Returns the target of the transaction.
    pub fn target(&self) -> &TransactionTarget {
        &self.target
    }

    /// Returns the entry point of the transaction.
    pub fn entry_point(&self) -> &TransactionEntryPoint {
        &self.entry_point
    }

    /// Returns the scheduling kind of the transaction.
    pub fn scheduling(&self) -> &TransactionScheduling {
        &self.scheduling
    }

    /// Returns true if this transaction is a native mint interaction.
    pub fn is_native_mint(&self) -> bool {
        self.transaction_lane == TransactionLane::Mint as u8
    }

    /// Returns true if this transaction is a native auction interaction.
    pub fn is_native_auction(&self) -> bool {
        self.transaction_lane == TransactionLane::Auction as u8
    }

    /// Returns true if this transaction is a smart contract installer or upgrader.
    pub fn is_install_or_upgrade(&self) -> bool {
        self.transaction_lane == TransactionLane::InstallUpgrade as u8
    }

    /// Returns the transaction category.
    pub fn transaction_lane(&self) -> u8 {
        self.transaction_lane
    }

    /// Returns the transaction runtime of the transaction.
    pub fn transaction_runtime(&self) -> Option<TransactionRuntime> {
        match self.target {
            TransactionTarget::Native => None,
            TransactionTarget::Stored { runtime, .. } => Some(runtime),
            TransactionTarget::Session { runtime, .. } => Some(runtime),
        }
    }

    /// Consumes `self`, returning its constituent parts.
    pub fn destructure(
        self,
    ) -> (
        TransactionArgs,
        TransactionTarget,
        TransactionEntryPoint,
        TransactionScheduling,
    ) {
        (self.args, self.target, self.entry_point, self.scheduling)
    }

    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub(super) fn is_valid(&self, config: &TransactionConfig) -> Result<(), InvalidTransactionV1> {
        use super::TransactionV1ExcessiveSizeError;

        let kind = self.transaction_lane;
        if !config.transaction_v1_config.is_supported(kind) {
            return Err(InvalidTransactionV1::InvalidTransactionKind(
                self.transaction_lane,
            ));
        }

        let max_serialized_length = config.transaction_v1_config.get_max_serialized_length(kind);
        let actual_length = self.serialized_length();
        if actual_length > max_serialized_length as usize {
            return Err(InvalidTransactionV1::ExcessiveSize(
                TransactionV1ExcessiveSizeError {
                    max_transaction_size: max_serialized_length as u32,
                    actual_transaction_size: actual_length,
                },
            ));
        }

        let max_args_length = config.transaction_v1_config.get_max_args_length(kind);

        let args_length = self.args.serialized_length();
        if args_length > max_args_length as usize {
            debug!(
                args_length,
                max_args_length = max_args_length,
                "transaction runtime args excessive size"
            );
            return Err(InvalidTransactionV1::ExcessiveArgsLength {
                max_length: max_args_length as usize,
                got: args_length,
            });
        }

        match &self.target {
            TransactionTarget::Native => match self.entry_point {
                TransactionEntryPoint::Call => {
                    debug!(
                        entry_point = %self.entry_point,
                        "native transaction cannot have call entry point"
                    );
                    Err(InvalidTransactionV1::EntryPointCannotBeCall)
                }
                TransactionEntryPoint::Custom(_) => {
                    debug!(
                        entry_point = %self.entry_point,
                        "native transaction cannot have custom entry point"
                    );
                    Err(InvalidTransactionV1::EntryPointCannotBeCustom {
                        entry_point: self.entry_point.clone(),
                    })
                }
                TransactionEntryPoint::Transfer => arg_handling::has_valid_transfer_args(
                    &self.args,
                    config.native_transfer_minimum_motes,
                ),
                TransactionEntryPoint::AddBid => arg_handling::has_valid_add_bid_args(&self.args),
                TransactionEntryPoint::WithdrawBid => {
                    arg_handling::has_valid_withdraw_bid_args(&self.args)
                }
                TransactionEntryPoint::Delegate => {
                    arg_handling::has_valid_delegate_args(&self.args)
                }
                TransactionEntryPoint::Undelegate => {
                    arg_handling::has_valid_undelegate_args(&self.args)
                }
                TransactionEntryPoint::Redelegate => {
                    arg_handling::has_valid_redelegate_args(&self.args)
                }
                TransactionEntryPoint::ActivateBid => {
                    arg_handling::has_valid_activate_bid_args(&self.args)
                }
                TransactionEntryPoint::ChangeBidPublicKey => {
                    arg_handling::has_valid_change_bid_public_key_args(&self.args)
                }
                TransactionEntryPoint::AddReservations => {
                    todo!()
                }
                TransactionEntryPoint::CancelReservations => {
                    todo!()
                }
            },
            TransactionTarget::Stored { .. } => match &self.entry_point {
                TransactionEntryPoint::Custom(_) => Ok(()),
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
                    debug!(
                        entry_point = %self.entry_point,
                        "transaction targeting stored entity/package must have custom entry point"
                    );
                    Err(InvalidTransactionV1::EntryPointMustBeCustom {
                        entry_point: self.entry_point.clone(),
                    })
                }
            },
            TransactionTarget::Session { module_bytes, .. } => match &self.entry_point {
                TransactionEntryPoint::Call | TransactionEntryPoint::Custom(_) => {
                    if module_bytes.is_empty() {
                        debug!("transaction with session code must not have empty module bytes");
                        return Err(InvalidTransactionV1::EmptyModuleBytes);
                    }
                    Ok(())
                }
                TransactionEntryPoint::Transfer
                | TransactionEntryPoint::AddBid
                | TransactionEntryPoint::WithdrawBid
                | TransactionEntryPoint::Delegate
                | TransactionEntryPoint::Undelegate
                | TransactionEntryPoint::Redelegate
                | TransactionEntryPoint::ActivateBid
                | TransactionEntryPoint::ChangeBidPublicKey
                | TransactionEntryPoint::AddReservations
                | TransactionEntryPoint::CancelReservations => {
                    debug!(
                        entry_point = %self.entry_point,
                        "transaction with session code must use custom or default 'call' entry point"
                    );
                    Err(InvalidTransactionV1::EntryPointMustBeCustom {
                        entry_point: self.entry_point.clone(),
                    })
                }
            },
        }
    }

    fn serialized_field_lengths(&self) -> Vec<usize> {
        vec![
            self.args.serialized_length(),
            self.target.serialized_length(),
            self.entry_point.serialized_length(),
            self.transaction_lane.serialized_length(),
            self.scheduling.serialized_length(),
        ]
    }

    /// Returns a random `TransactionV1Body`.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random_of_lane(rng: &mut TestRng, lane: u8) -> Self {
        match lane {
            0 => Self::random_transfer(rng),
            1 => Self::random_staking(rng),
            2 => Self::random_install_upgrade(rng),
            _ => Self::random_standard(rng),
        }
    }

    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    fn random_transfer(rng: &mut TestRng) -> Self {
        use crate::transaction::TransferTarget;

        let amount =
            rng.gen_range(TransactionConfig::default().native_transfer_minimum_motes..=u64::MAX);
        let maybe_source = if rng.gen() { Some(rng.gen()) } else { None };
        let target = TransferTarget::random(rng);
        let maybe_id = rng.gen::<bool>().then(|| rng.gen());
        let args = arg_handling::new_transfer_args(amount, maybe_source, target, maybe_id).unwrap();
        TransactionV1Body::new(
            args,
            TransactionTarget::Native,
            TransactionEntryPoint::Transfer,
            TransactionLane::Mint as u8,
            TransactionScheduling::random(rng),
        )
    }

    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    fn random_standard(rng: &mut TestRng) -> Self {
        use crate::transaction::TransactionInvocationTarget;

        let target = TransactionTarget::Stored {
            id: TransactionInvocationTarget::random(rng),
            runtime: TransactionRuntime::VmCasperV1,
        };
        TransactionV1Body::new(
            RuntimeArgs::random(rng),
            target,
            TransactionEntryPoint::Custom(rng.random_string(1..11)),
            TransactionLane::Large as u8,
            TransactionScheduling::random(rng),
        )
    }

    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    fn random_install_upgrade(rng: &mut TestRng) -> Self {
        let target = TransactionTarget::Session {
            module_bytes: Bytes::from(rng.random_vec(0..100)),
            runtime: TransactionRuntime::VmCasperV1,
        };
        TransactionV1Body::new(
            RuntimeArgs::random(rng),
            target,
            TransactionEntryPoint::Custom(rng.random_string(1..11)),
            TransactionLane::InstallUpgrade as u8,
            TransactionScheduling::random(rng),
        )
    }

    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    fn random_staking(rng: &mut TestRng) -> Self {
        let public_key = PublicKey::random(rng);
        let delegation_rate = rng.gen();
        let amount = rng.gen::<u64>();
        let minimum_delegation_amount = rng.gen::<u32>() as u64;
        let maximum_delegation_amount = minimum_delegation_amount + rng.gen::<u32>() as u64;
        let args = arg_handling::new_add_bid_args(
            public_key,
            delegation_rate,
            amount,
            minimum_delegation_amount,
            maximum_delegation_amount,
        )
        .unwrap();
        TransactionV1Body::new(
            args,
            TransactionTarget::Native,
            TransactionEntryPoint::AddBid,
            TransactionLane::Auction as u8,
            TransactionScheduling::random(rng),
        )
    }

    /// Returns a random `TransactionV1Body`.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random(rng: &mut TestRng) -> Self {
        use crate::transaction::TransferTarget;

        match rng.gen_range(0..8) {
            0 => {
                let amount = rng.gen_range(
                    TransactionConfig::default().native_transfer_minimum_motes..=u64::MAX,
                );
                let maybe_source = if rng.gen() { Some(rng.gen()) } else { None };
                let target = TransferTarget::random(rng);
                let maybe_id = rng.gen::<bool>().then(|| rng.gen());
                let args = arg_handling::new_transfer_args(amount, maybe_source, target, maybe_id)
                    .unwrap();
                TransactionV1Body::new(
                    args,
                    TransactionTarget::Native,
                    TransactionEntryPoint::Transfer,
                    TransactionLane::Mint as u8,
                    TransactionScheduling::random(rng),
                )
            }
            1 => {
                let public_key = PublicKey::random(rng);
                let delegation_rate = rng.gen();
                let amount = rng.gen::<u64>();
                let minimum_delegation_amount = rng.gen::<u32>() as u64;
                let maximum_delegation_amount = minimum_delegation_amount + rng.gen::<u32>() as u64;
                let args = arg_handling::new_add_bid_args(
                    public_key,
                    delegation_rate,
                    amount,
                    minimum_delegation_amount,
                    maximum_delegation_amount,
                )
                .unwrap();
                TransactionV1Body::new(
                    args,
                    TransactionTarget::Native,
                    TransactionEntryPoint::AddBid,
                    TransactionLane::Auction as u8,
                    TransactionScheduling::random(rng),
                )
            }
            2 => {
                let public_key = PublicKey::random(rng);
                let amount = rng.gen::<u64>();
                let args = arg_handling::new_withdraw_bid_args(public_key, amount).unwrap();
                TransactionV1Body::new(
                    args,
                    TransactionTarget::Native,
                    TransactionEntryPoint::WithdrawBid,
                    TransactionLane::Auction as u8,
                    TransactionScheduling::random(rng),
                )
            }
            3 => {
                let delegator = PublicKey::random(rng);
                let validator = PublicKey::random(rng);
                let amount = rng.gen::<u64>();
                let args = arg_handling::new_delegate_args(delegator, validator, amount).unwrap();
                TransactionV1Body::new(
                    args,
                    TransactionTarget::Native,
                    TransactionEntryPoint::Delegate,
                    TransactionLane::Auction as u8,
                    TransactionScheduling::random(rng),
                )
            }
            4 => {
                let delegator = PublicKey::random(rng);
                let validator = PublicKey::random(rng);
                let amount = rng.gen::<u64>();
                let args = arg_handling::new_undelegate_args(delegator, validator, amount).unwrap();
                TransactionV1Body::new(
                    args,
                    TransactionTarget::Native,
                    TransactionEntryPoint::Undelegate,
                    TransactionLane::Auction as u8,
                    TransactionScheduling::random(rng),
                )
            }
            5 => {
                let delegator = PublicKey::random(rng);
                let validator = PublicKey::random(rng);
                let amount = rng.gen::<u64>();
                let new_validator = PublicKey::random(rng);
                let args =
                    arg_handling::new_redelegate_args(delegator, validator, amount, new_validator)
                        .unwrap();
                TransactionV1Body::new(
                    args,
                    TransactionTarget::Native,
                    TransactionEntryPoint::Redelegate,
                    TransactionLane::Auction as u8,
                    TransactionScheduling::random(rng),
                )
            }
            6 => Self::random_standard(rng),
            7 => {
                let mut buffer = vec![0u8; rng.gen_range(1..100)];
                rng.fill_bytes(buffer.as_mut());
                let target = TransactionTarget::Session {
                    module_bytes: Bytes::from(buffer),
                    runtime: TransactionRuntime::VmCasperV1,
                };
                TransactionV1Body::new(
                    RuntimeArgs::random(rng),
                    target,
                    TransactionEntryPoint::Custom(rng.random_string(1..11)),
                    TransactionLane::Large as u8,
                    TransactionScheduling::random(rng),
                )
            }
            _ => unreachable!(),
        }
    }

    /// Returns a token value attached to the transaction.
    pub fn value(&self) -> u128 {
        self.transferred_value
    }
}

const ARGS_INDEX: u16 = 0;
const TARGET_INDEX: u16 = 1;
const ENTRY_POINT_INDEX: u16 = 2;
const TRANSACTION_LANE_INDEX: u16 = 3;
const SCHEDULING_INDEX: u16 = 4;
const TRANSFERRED_VALUE_INDEX: u16 = 5;

impl FromBytes for TransactionV1Body {
    fn from_bytes(bytes: &[u8]) -> Result<(TransactionV1Body, &[u8]), Error> {
        let (binary_payload, remainder) =
            crate::transaction::serialization::CalltableSerializationEnvelope::from_bytes(
                5, bytes,
            )?;
        let window = binary_payload.start_consuming()?;
        let window = window.ok_or(Error::Formatting)?;
        window.verify_index(ARGS_INDEX)?;
        let (args, window) = window.deserialize_and_maybe_next::<TransactionArgs>()?;
        let window = window.ok_or(Error::Formatting)?;
        window.verify_index(TARGET_INDEX)?;
        let (target, window) = window.deserialize_and_maybe_next::<TransactionTarget>()?;
        let window = window.ok_or(Error::Formatting)?;
        window.verify_index(ENTRY_POINT_INDEX)?;
        let (entry_point, window) = window.deserialize_and_maybe_next::<TransactionEntryPoint>()?;
        let window = window.ok_or(Error::Formatting)?;
        window.verify_index(TRANSACTION_LANE_INDEX)?;
        let (transaction_lane, window) = window.deserialize_and_maybe_next::<u8>()?;
        let window = window.ok_or(Error::Formatting)?;
        window.verify_index(SCHEDULING_INDEX)?;

        let (scheduling, window) = window.deserialize_and_maybe_next::<TransactionScheduling>()?;
        let window = window.ok_or(Error::Formatting)?;
        window.verify_index(TRANSFERRED_VALUE_INDEX)?;
        let (transferred_value, window) = window.deserialize_and_maybe_next::<u128>()?;
        if window.is_some() {
            return Err(Error::Formatting);
        }
        let from_bytes = TransactionV1Body {
            args,
            target,
            entry_point,
            transaction_lane,
            scheduling,
            transferred_value,
        };
        Ok((from_bytes, remainder))
    }
}

impl ToBytes for TransactionV1Body {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        CalltableSerializationEnvelopeBuilder::new(self.serialized_field_lengths())?
            .add_field(ARGS_INDEX, &self.args)?
            .add_field(TARGET_INDEX, &self.target)?
            .add_field(ENTRY_POINT_INDEX, &self.entry_point)?
            .add_field(TRANSACTION_LANE_INDEX, &self.transaction_lane)?
            .add_field(SCHEDULING_INDEX, &self.scheduling)?
            .binary_payload_bytes()
    }
    fn serialized_length(&self) -> usize {
        CalltableSerializationEnvelope::estimate_size(self.serialized_field_lengths())
    }
}

impl Display for TransactionV1Body {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "v1-body({} {} {})",
            self.target, self.entry_point, self.scheduling
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bytesrepr, runtime_args};

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        let body = TransactionV1Body::random(rng);
        bytesrepr::test_serialization_roundtrip(&body);
    }

    #[test]
    fn not_acceptable_due_to_excessive_args_length() {
        let rng = &mut TestRng::new();
        let mut config = TransactionConfig::default();
        let mut body = TransactionV1Body::random_standard(rng);
        config.transaction_v1_config.wasm_lanes =
            vec![vec![body.transaction_lane as u64, 1_048_576, 10, 0]];
        body.args = TransactionArgs::Named(runtime_args! {"a" => 1_u8});

        let expected_error = InvalidTransactionV1::ExcessiveArgsLength {
            max_length: 10,
            got: 16,
        };

        assert_eq!(body.is_valid(&config), Err(expected_error));
    }

    #[test]
    fn not_acceptable_due_to_custom_entry_point_in_native() {
        let rng = &mut TestRng::new();
        let public_key = PublicKey::random(rng);
        let amount = rng.gen::<u64>();
        let args = arg_handling::new_withdraw_bid_args(public_key, amount).unwrap();
        let entry_point = TransactionEntryPoint::Custom("custom".to_string());
        let body = TransactionV1Body::new(
            args,
            TransactionTarget::Native,
            entry_point.clone(),
            TransactionLane::Mint as u8,
            TransactionScheduling::random(rng),
        );

        let expected_error = InvalidTransactionV1::EntryPointCannotBeCustom { entry_point };

        let config = TransactionConfig::default();
        assert_eq!(body.is_valid(&config), Err(expected_error));
    }

    #[test]
    fn not_acceptable_due_to_call_entry_point_in_native() {
        let rng = &mut TestRng::new();
        let public_key = PublicKey::random(rng);
        let amount = rng.gen::<u64>();
        let args = arg_handling::new_withdraw_bid_args(public_key, amount).unwrap();
        let entry_point = TransactionEntryPoint::Call;
        let body = TransactionV1Body::new(
            args,
            TransactionTarget::Native,
            entry_point,
            TransactionLane::Mint as u8,
            TransactionScheduling::random(rng),
        );

        let expected_error = InvalidTransactionV1::EntryPointCannotBeCall;

        let config = TransactionConfig::default();
        assert_eq!(body.is_valid(&config,), Err(expected_error));
    }

    #[test]
    fn not_acceptable_due_to_non_custom_entry_point_in_stored_or_session() {
        let rng = &mut TestRng::new();
        let config = TransactionConfig::default();

        let mut check = |entry_point: TransactionEntryPoint| {
            let stored_target = TransactionTarget::new_stored(
                TransactionInvocationTarget::ByHash([0; 32]),
                TransactionRuntime::VmCasperV1,
            );
            let session_target = TransactionTarget::new_session(
                Bytes::from(vec![1]),
                TransactionRuntime::VmCasperV1,
            );

            let stored_body = TransactionV1Body::new(
                RuntimeArgs::new(),
                stored_target,
                entry_point.clone(),
                TransactionLane::Large as u8,
                TransactionScheduling::random(rng),
            );
            let session_body = TransactionV1Body::new(
                RuntimeArgs::new(),
                session_target,
                entry_point.clone(),
                TransactionLane::Large as u8,
                TransactionScheduling::random(rng),
            );

            let expected_error = InvalidTransactionV1::EntryPointMustBeCustom { entry_point };

            assert_eq!(stored_body.is_valid(&config), Err(expected_error.clone()));
            assert_eq!(session_body.is_valid(&config), Err(expected_error));
        };

        check(TransactionEntryPoint::Transfer);
        check(TransactionEntryPoint::AddBid);
        check(TransactionEntryPoint::WithdrawBid);
        check(TransactionEntryPoint::Delegate);
        check(TransactionEntryPoint::Undelegate);
        check(TransactionEntryPoint::Redelegate);
    }
}
