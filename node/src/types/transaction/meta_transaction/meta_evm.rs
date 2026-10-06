use std::fmt::{self, Display, Formatter};

use casper_types::{
    bytesrepr::ToBytes, Approval, Chainspec, Digest, EvmConfig, EvmTransaction,
    EvmTransactionError, EvmTransactionKind, Gas, InitiatorAddr, InvalidTransaction, TimeDiff,
    Timestamp, TransactionHash,
};
use serde::Serialize;

/// Metadata extracted from a Casper EVM transaction.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct MetaEvmTransaction {
    transaction: EvmTransaction,
    lane_id: u8,
    payload_hash: Digest,
}

impl MetaEvmTransaction {
    pub(crate) fn from_evm_transaction(
        transaction: &EvmTransaction,
        evm_config: &EvmConfig,
    ) -> Result<Self, InvalidTransaction> {
        let lane_id = evm_config
            .get_evm_lane_id(
                transaction.gas_limit(),
                transaction.serialized_length() as u64,
                transaction.input().len() as u64,
            )
            .ok_or(EvmTransactionError::MissingTransactionLane)?;
        let payload_hash = Digest::hash(transaction.signing_payload()?);
        Ok(MetaEvmTransaction {
            transaction: transaction.clone(),
            lane_id,
            payload_hash,
        })
    }

    pub(crate) fn transaction(&self) -> &EvmTransaction {
        &self.transaction
    }

    pub(crate) fn hash(&self) -> TransactionHash {
        TransactionHash::from(self.transaction.hash())
    }

    pub(crate) fn timestamp(&self) -> Timestamp {
        self.transaction.timestamp()
    }

    pub(crate) fn ttl(&self) -> TimeDiff {
        self.transaction.ttl()
    }

    pub(crate) fn approval(&self) -> Option<&Approval> {
        self.transaction.approval()
    }

    pub(crate) fn initiator_addr(&self) -> InitiatorAddr {
        self.transaction.initiator_addr()
    }

    pub(crate) fn lane_id(&self) -> u8 {
        self.lane_id
    }

    pub(crate) fn gas_limit(&self) -> Gas {
        Gas::new(self.transaction.gas_limit())
    }

    pub(crate) fn gas_price_tolerance(&self) -> u8 {
        u8::MAX
    }

    pub(crate) fn serialized_length(&self) -> usize {
        self.transaction.serialized_length()
    }

    pub(crate) fn payload_hash(&self) -> Digest {
        self.payload_hash
    }

    pub(crate) fn verify(&self) -> Result<(), EvmTransactionError> {
        self.transaction.verify()
    }

    pub(crate) fn is_config_compliant(
        &self,
        chainspec: &Chainspec,
    ) -> Result<(), EvmTransactionError> {
        let transaction = &self.transaction;
        let evm_config = &chainspec.evm_config;
        if !evm_config.enabled {
            return Err(EvmTransactionError::Disabled);
        }

        if !transaction.is_unsigned_call() {
            transaction.verify()?;
        }

        let expected = evm_config.chain_id;
        let actual = transaction
            .chain_id()
            .ok_or(EvmTransactionError::MissingChainId)?;
        if actual != expected {
            return Err(EvmTransactionError::ChainIdMismatch { expected, actual });
        }

        let gas_limit = transaction.gas_limit();
        evm_config.validate_transaction_gas_limit(gas_limit)?;

        // A non-empty access list increases intrinsic gas. Reject a shortfall
        // before packing because `revm` reports transaction-validation errors
        // as fatal block-execution errors.
        if !transaction.access_list().is_empty() {
            let intrinsic_gas = transaction.intrinsic_gas();
            if intrinsic_gas > gas_limit as u128 {
                return Err(EvmTransactionError::IntrinsicGasExceedsGasLimit {
                    intrinsic_gas,
                    gas_limit,
                });
            }
        }

        if !transaction.is_unsigned_call() && evm_config.value_motes(transaction.value()).is_none()
        {
            return Err(EvmTransactionError::ValueNotRepresentable {
                value: transaction.value(),
                wei_per_mote: evm_config.wei_per_mote,
            });
        }

        let base_fee = evm_config.base_fee_wei();
        match transaction.kind() {
            EvmTransactionKind::Legacy | EvmTransactionKind::Eip2930 => {
                if let Some(max_priority_fee_per_gas) = transaction.max_priority_fee_per_gas() {
                    return Err(EvmTransactionError::UnexpectedMaxPriorityFeePerGas {
                        max_priority_fee_per_gas,
                    });
                }
                let gas_price = transaction
                    .gas_price()
                    .ok_or(EvmTransactionError::MissingGasPrice)?;
                if gas_price < base_fee {
                    return Err(EvmTransactionError::GasPriceBelowBaseFee {
                        gas_price,
                        base_fee,
                    });
                }
                if !transaction.is_unsigned_call() && gas_price > base_fee {
                    return Err(EvmTransactionError::PositiveEffectivePriorityFeePerGas {
                        priority_fee_per_gas: gas_price - base_fee,
                    });
                }
            }
            EvmTransactionKind::Eip1559 | EvmTransactionKind::Eip7702 => {
                // `max_fee_per_gas` remains the user's total price cap and must
                // cover the configured base fee.
                let max_fee_per_gas = transaction.max_fee_per_gas();
                if max_fee_per_gas < base_fee {
                    return Err(EvmTransactionError::MaxFeePerGasBelowBaseFee {
                        max_fee_per_gas,
                        base_fee,
                    });
                }
                let max_priority_fee_per_gas = transaction
                    .max_priority_fee_per_gas()
                    .ok_or(EvmTransactionError::MissingMaxPriorityFeePerGas)?;
                if max_priority_fee_per_gas > max_fee_per_gas {
                    return Err(
                        EvmTransactionError::MaxPriorityFeePerGasExceedsMaxFeePerGas {
                            max_priority_fee_per_gas,
                            max_fee_per_gas,
                        },
                    );
                }
                let priority_fee_per_gas = transaction.effective_priority_fee_per_gas(base_fee);
                if priority_fee_per_gas != 0 {
                    // A non-zero cap is compatible with a tipless network when
                    // max_fee_per_gas equals the base fee. Reject only a
                    // positive effective tip, which would charge for priority
                    // that Casper does not honor.
                    return Err(EvmTransactionError::PositiveEffectivePriorityFeePerGas {
                        priority_fee_per_gas,
                    });
                }
            }
        }

        Ok(())
    }
}

impl Display for MetaEvmTransaction {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.transaction, formatter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TransactionFootprint;
    use alloy_consensus::{
        SignableTransaction, TxEip1559, TxEip2930, TxEip7702, TxEnvelope, TxLegacy,
    };
    use alloy_eips::{eip2718::Encodable2718, eip7702::Authorization};
    use alloy_primitives::{Address, Signature, TxKind, U256};
    use casper_types::{testing::TestRng, EvmConfig, Transaction, EVM_TRANSACTION_GAS_LIMIT};

    fn transaction(kind: EvmTransactionKind, gas_limit: u64) -> EvmTransaction {
        let to = TxKind::Call(Address::from([0x44; 20]));
        let signature = Signature::test_signature();
        let envelope: TxEnvelope = match kind {
            EvmTransactionKind::Legacy => TxLegacy {
                chain_id: Some(7),
                gas_limit,
                gas_price: 0,
                to,
                ..Default::default()
            }
            .into_signed(signature)
            .into(),
            EvmTransactionKind::Eip2930 => TxEip2930 {
                chain_id: 7,
                gas_limit,
                gas_price: 0,
                to,
                ..Default::default()
            }
            .into_signed(signature)
            .into(),
            EvmTransactionKind::Eip1559 => TxEip1559 {
                chain_id: 7,
                gas_limit,
                max_fee_per_gas: 0,
                max_priority_fee_per_gas: 0,
                to,
                ..Default::default()
            }
            .into_signed(signature)
            .into(),
            EvmTransactionKind::Eip7702 => TxEip7702 {
                chain_id: 7,
                gas_limit,
                max_fee_per_gas: 0,
                max_priority_fee_per_gas: 0,
                to: Address::from([0x44; 20]),
                authorization_list: vec![Authorization {
                    chain_id: U256::from(7),
                    address: Address::ZERO,
                    nonce: 0,
                }
                .into_signed(signature)],
                ..Default::default()
            }
            .into_signed(signature)
            .into(),
        };
        EvmTransaction::from_signed_rlp(
            envelope.encoded_2718(),
            Timestamp::zero(),
            TimeDiff::from_seconds(60),
        )
        .unwrap()
    }

    #[test]
    fn osaka_admission_and_received_block_footprints_validate_all_envelopes() {
        let mut chainspec = Chainspec::random(&mut TestRng::new());
        chainspec.evm_config = EvmConfig {
            enabled: true,
            chain_id: 7,
            ..Default::default()
        };
        // Received block validation and packing both construct this same footprint.
        for kind in [
            EvmTransactionKind::Legacy,
            EvmTransactionKind::Eip2930,
            EvmTransactionKind::Eip1559,
            EvmTransactionKind::Eip7702,
        ] {
            for gas_limit in [
                EVM_TRANSACTION_GAS_LIMIT - 1,
                EVM_TRANSACTION_GAS_LIMIT,
                EVM_TRANSACTION_GAS_LIMIT + 1,
            ] {
                let tx = transaction(kind, gas_limit);
                let meta =
                    MetaEvmTransaction::from_evm_transaction(&tx, &chainspec.transaction_config)
                        .unwrap();
                let admission = meta.is_config_compliant(&chainspec);
                let footprint =
                    TransactionFootprint::new(&chainspec, &Transaction::Evm(Box::new(tx)));
                if gas_limit <= EVM_TRANSACTION_GAS_LIMIT {
                    assert_eq!(admission, Ok(()), "kind {kind}");
                    assert!(footprint.is_ok(), "kind {kind}: {footprint:?}");
                } else {
                    assert!(matches!(
                        admission,
                        Err(EvmTransactionError::GasLimitExceedsTransactionGasLimit { .. })
                    ));
                    assert!(matches!(
                        footprint,
                        Err(InvalidTransaction::Evm(
                            EvmTransactionError::GasLimitExceedsTransactionGasLimit { .. }
                        ))
                    ));
                }
            }
            let tx = transaction(kind, 100_001);
            let lower = Chainspec {
                evm_config: EvmConfig {
                    block_gas_limit: 100_000,
                    ..chainspec.evm_config.clone()
                },
                ..chainspec.clone()
            };
            let meta =
                MetaEvmTransaction::from_evm_transaction(&tx, &lower.transaction_config).unwrap();
            assert!(matches!(
                meta.is_config_compliant(&lower),
                Err(EvmTransactionError::GasLimitExceedsBlockGasLimit { .. })
            ));
            assert!(matches!(
                TransactionFootprint::new(&lower, &Transaction::Evm(Box::new(tx))),
                Err(InvalidTransaction::Evm(
                    EvmTransactionError::GasLimitExceedsBlockGasLimit { .. }
                ))
            ));
        }
    }
}
