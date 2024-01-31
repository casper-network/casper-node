#[cfg(any(feature = "std", test))]
pub(super) mod arg_handling;

use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::{Rng, RngCore};
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
#[cfg(any(feature = "std", test))]
use serde::{Deserialize, Serialize};
#[cfg(any(feature = "std", test))]
use tracing::debug;

use super::super::{RuntimeArgs, TransactionEntryPoint, TransactionScheduling, TransactionTarget};
#[cfg(doc)]
use super::TransactionV1;
#[cfg(any(feature = "std", test))]
use super::{TransactionConfig, TransactionV1ConfigFailure};
use crate::bytesrepr::{self, FromBytes, ToBytes};
#[cfg(any(feature = "testing", test))]
use crate::{
    bytesrepr::Bytes, testing::TestRng, PublicKey, TransactionInvocationTarget, TransactionRuntime,
    TransactionSessionKind,
};

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
    pub(super) args: RuntimeArgs,
    pub(super) target: TransactionTarget,
    pub(super) entry_point: TransactionEntryPoint,
    pub(super) scheduling: TransactionScheduling,
}

impl TransactionV1Body {
    /// Returns a new `TransactionV1Body`.
    pub fn new(
        args: RuntimeArgs,
        target: TransactionTarget,
        entry_point: TransactionEntryPoint,
        scheduling: TransactionScheduling,
    ) -> Self {
        TransactionV1Body {
            args,
            target,
            entry_point,
            scheduling,
        }
    }

    /// Returns the runtime args of the transaction.
    pub fn args(&self) -> &RuntimeArgs {
        &self.args
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

    #[cfg(any(feature = "std", test))]
    pub(super) fn is_valid(
        &self,
        config: &TransactionConfig,
    ) -> Result<(), TransactionV1ConfigFailure> {
        let args_length = self.args.serialized_length();
        if args_length > config.transaction_v1_config.max_args_length as usize {
            debug!(
                args_length,
                max_args_length = config.transaction_v1_config.max_args_length,
                "transaction runtime args excessive size"
            );
            return Err(TransactionV1ConfigFailure::ExcessiveArgsLength {
                max_length: config.transaction_v1_config.max_args_length as usize,
                got: args_length,
            });
        }

        match &self.target {
            TransactionTarget::Native => match self.entry_point {
                TransactionEntryPoint::Custom(_) => {
                    debug!(
                        entry_point = %self.entry_point,
                        "native transaction cannot have custom entry point"
                    );
                    Err(TransactionV1ConfigFailure::EntryPointCannotBeCustom {
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
            },
            TransactionTarget::Stored { .. } => match &self.entry_point {
                TransactionEntryPoint::Custom(_) => Ok(()),
                TransactionEntryPoint::Transfer
                | TransactionEntryPoint::AddBid
                | TransactionEntryPoint::WithdrawBid
                | TransactionEntryPoint::Delegate
                | TransactionEntryPoint::Undelegate
                | TransactionEntryPoint::Redelegate => {
                    debug!(
                        entry_point = %self.entry_point,
                        "transaction targeting stored entity/package must have custom entry point"
                    );
                    Err(TransactionV1ConfigFailure::EntryPointMustBeCustom {
                        entry_point: self.entry_point.clone(),
                    })
                }
            },
            TransactionTarget::Session { module_bytes, .. } => match &self.entry_point {
                TransactionEntryPoint::Custom(_) => {
                    if module_bytes.is_empty() {
                        debug!("transaction with session code must not have empty module bytes");
                        return Err(TransactionV1ConfigFailure::EmptyModuleBytes);
                    }
                    Ok(())
                }
                TransactionEntryPoint::Transfer
                | TransactionEntryPoint::AddBid
                | TransactionEntryPoint::WithdrawBid
                | TransactionEntryPoint::Delegate
                | TransactionEntryPoint::Undelegate
                | TransactionEntryPoint::Redelegate => {
                    debug!(
                        entry_point = %self.entry_point,
                        "transaction with session code must have custom entry point"
                    );
                    Err(TransactionV1ConfigFailure::EntryPointMustBeCustom {
                        entry_point: self.entry_point.clone(),
                    })
                }
            },
        }
    }

    /// Returns a random `TransactionV1Body`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..8) {
            0 => {
                let source = rng.gen();
                let target = rng.gen();
                let amount = rng.gen_range(
                    TransactionConfig::default().native_transfer_minimum_motes..=u64::MAX,
                );
                let maybe_to = rng.gen::<bool>().then(|| rng.gen());
                let maybe_id = rng.gen::<bool>().then(|| rng.gen());
                let args =
                    arg_handling::new_transfer_args(source, target, amount, maybe_to, maybe_id)
                        .unwrap();
                TransactionV1Body::new(
                    args,
                    TransactionTarget::Native,
                    TransactionEntryPoint::Transfer,
                    TransactionScheduling::random(rng),
                )
            }
            1 => {
                let public_key = PublicKey::random(rng);
                let delegation_rate = rng.gen();
                let amount = rng.gen::<u64>();
                let args =
                    arg_handling::new_add_bid_args(public_key, delegation_rate, amount).unwrap();
                TransactionV1Body::new(
                    args,
                    TransactionTarget::Native,
                    TransactionEntryPoint::AddBid,
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
                    TransactionScheduling::random(rng),
                )
            }
            6 => {
                let target = TransactionTarget::Stored {
                    id: TransactionInvocationTarget::random(rng),
                    runtime: TransactionRuntime::VmCasperV1,
                };
                TransactionV1Body::new(
                    RuntimeArgs::random(rng),
                    target,
                    TransactionEntryPoint::Custom(rng.random_string(1..11)),
                    TransactionScheduling::random(rng),
                )
            }
            7 => {
                let mut buffer = vec![0u8; rng.gen_range(0..100)];
                rng.fill_bytes(buffer.as_mut());
                let target = TransactionTarget::Session {
                    kind: TransactionSessionKind::random(rng),
                    module_bytes: Bytes::from(buffer),
                    runtime: TransactionRuntime::VmCasperV1,
                };
                TransactionV1Body::new(
                    RuntimeArgs::random(rng),
                    target,
                    TransactionEntryPoint::Custom(rng.random_string(1..11)),
                    TransactionScheduling::random(rng),
                )
            }
            _ => unreachable!(),
        }
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

impl ToBytes for TransactionV1Body {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.args.write_bytes(writer)?;
        self.target.write_bytes(writer)?;
        self.entry_point.write_bytes(writer)?;
        self.scheduling.write_bytes(writer)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.args.serialized_length()
            + self.target.serialized_length()
            + self.entry_point.serialized_length()
            + self.scheduling.serialized_length()
    }
}

impl FromBytes for TransactionV1Body {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (args, remainder) = RuntimeArgs::from_bytes(bytes)?;
        let (target, remainder) = TransactionTarget::from_bytes(remainder)?;
        let (entry_point, remainder) = TransactionEntryPoint::from_bytes(remainder)?;
        let (scheduling, remainder) = TransactionScheduling::from_bytes(remainder)?;
        let body = TransactionV1Body::new(args, target, entry_point, scheduling);
        Ok((body, remainder))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_args;

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
        config.transaction_v1_config.max_args_length = 10;
        let mut body = TransactionV1Body::random(rng);
        body.args = runtime_args! {"a" => 1_u8};

        let expected_error = TransactionV1ConfigFailure::ExcessiveArgsLength {
            max_length: 10,
            got: 15,
        };

        assert_eq!(body.is_valid(&config,), Err(expected_error));
    }

    #[test]
    fn not_acceptable_due_to_custom_entry_point_in_native() {
        let rng = &mut TestRng::new();
        let public_key = PublicKey::random(rng);
        let amount = rng.gen::<u64>();
        let args = arg_handling::new_withdraw_bid_args(public_key, amount).unwrap();
        let entry_point = TransactionEntryPoint::Custom("call".to_string());
        let body = TransactionV1Body::new(
            args,
            TransactionTarget::Native,
            entry_point.clone(),
            TransactionScheduling::random(rng),
        );

        let expected_error = TransactionV1ConfigFailure::EntryPointCannotBeCustom { entry_point };

        let config = TransactionConfig::default();
        assert_eq!(body.is_valid(&config,), Err(expected_error));
    }

    #[test]
    fn not_acceptable_due_to_non_custom_entry_point_in_stored_or_session() {
        let rng = &mut TestRng::new();
        let config = TransactionConfig::default();

        let mut check = |entry_point: TransactionEntryPoint| {
            let stored_target = TransactionTarget::new_stored(
                TransactionInvocationTarget::InvocableEntity([0; 32]),
                TransactionRuntime::VmCasperV1,
            );
            let session_target = TransactionTarget::new_session(
                TransactionSessionKind::Standard,
                Bytes::from(vec![1]),
                TransactionRuntime::VmCasperV1,
            );

            let stored_body = TransactionV1Body::new(
                RuntimeArgs::new(),
                stored_target,
                entry_point.clone(),
                TransactionScheduling::random(rng),
            );
            let session_body = TransactionV1Body::new(
                RuntimeArgs::new(),
                session_target,
                entry_point.clone(),
                TransactionScheduling::random(rng),
            );

            let expected_error = TransactionV1ConfigFailure::EntryPointMustBeCustom { entry_point };

            assert_eq!(stored_body.is_valid(&config,), Err(expected_error.clone()));
            assert_eq!(session_body.is_valid(&config,), Err(expected_error));
        };

        check(TransactionEntryPoint::Transfer);
        check(TransactionEntryPoint::AddBid);
        check(TransactionEntryPoint::WithdrawBid);
        check(TransactionEntryPoint::Delegate);
        check(TransactionEntryPoint::Undelegate);
        check(TransactionEntryPoint::Redelegate);
    }
}
