use std::fmt::{self, Display, Formatter};

use casper_types::{
    bytesrepr::ToBytes, Approval, Chainspec, Digest, EvmTransaction, EvmTransactionError,
    EvmTransactionKind, Gas, InitiatorAddr, InvalidTransaction, TimeDiff, Timestamp,
    TransactionConfig, TransactionHash,
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
        transaction_config: &TransactionConfig,
    ) -> Result<Self, InvalidTransaction> {
        let lane_id = transaction_config
            .transaction_v1_config
            .wasm_lanes()
            .iter()
            .last()
            .map(|lane| lane.id())
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
        let block_gas_limit = evm_config.block_gas_limit;
        if gas_limit > block_gas_limit {
            return Err(EvmTransactionError::GasLimitExceedsBlockGasLimit {
                gas_limit,
                block_gas_limit,
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
            }
            EvmTransactionKind::Eip1559 | EvmTransactionKind::Eip7702 => {
                // `max_fee_per_gas` is still meaningful on Casper as the user's
                // dynamic-fee total price cap. It must at least cover the
                // configured EVM base fee; with the priority fee forced to zero
                // below, this cap is what lets Ethereum tooling submit typed
                // dynamic-fee transactions without implying transaction
                // priority based on gas parameters.
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
                if max_priority_fee_per_gas != 0 {
                    // Casper does not currently prioritize transactions based
                    // on transaction gas parameters. Accepting a non-zero
                    // EIP-1559 priority fee would charge users for a priority
                    // signal that the node does not honor, so this prototype
                    // only accepts EIP-1559 as a max-fee compatibility
                    // envelope with zero priority fee.
                    return Err(EvmTransactionError::NonZeroMaxPriorityFeePerGas {
                        max_priority_fee_per_gas,
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
