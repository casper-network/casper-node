use std::{
    collections::BTreeSet,
    fmt::{self, Display, Formatter},
};

use casper_types::{
    bytesrepr::ToBytes, evm, Approval, Chainspec, Digest, Gas, InitiatorAddr, InvalidTransaction,
    TimeDiff, Timestamp, TransactionConfig, TransactionHash,
};
use serde::Serialize;

/// Metadata extracted from a Casper EVM transaction.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct MetaEvmTransaction {
    transaction: evm::Transaction,
    initiator_addr: InitiatorAddr,
    lane_id: u8,
    payload_hash: Digest,
}

impl MetaEvmTransaction {
    pub(crate) fn from_evm_transaction(
        transaction: &evm::Transaction,
        transaction_config: &TransactionConfig,
    ) -> Result<Self, InvalidTransaction> {
        let lane_id = transaction_config
            .transaction_v1_config
            .wasm_lanes()
            .iter()
            .last()
            .map(|lane| lane.id())
            .ok_or(evm::TransactionError::MissingTransactionLane)?;
        let payload_hash = Digest::hash(transaction.signing_payload()?);
        Ok(MetaEvmTransaction {
            transaction: transaction.clone(),
            initiator_addr: InitiatorAddr::EvmAddress(transaction.from()),
            lane_id,
            payload_hash,
        })
    }

    pub(crate) fn transaction(&self) -> &evm::Transaction {
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

    pub(crate) fn approvals(&self) -> &BTreeSet<Approval> {
        self.transaction.approvals()
    }

    pub(crate) fn initiator_addr(&self) -> &InitiatorAddr {
        &self.initiator_addr
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

    pub(crate) fn verify(&self) -> Result<(), evm::TransactionError> {
        self.transaction.verify()
    }

    pub(crate) fn is_config_compliant(
        &self,
        chainspec: &Chainspec,
    ) -> Result<(), evm::TransactionError> {
        let transaction = &self.transaction;
        let evm_config = &chainspec.evm_config;
        if !evm_config.enabled {
            return Err(evm::TransactionError::Disabled);
        }

        if !transaction.is_unsigned_call() {
            transaction.verify()?;
        }

        let expected = evm_config.chain_id;
        let actual = transaction
            .chain_id()
            .ok_or(evm::TransactionError::MissingChainId)?;
        if actual != expected {
            return Err(evm::TransactionError::ChainIdMismatch { expected, actual });
        }

        let gas_limit = transaction.gas_limit();
        let block_gas_limit = evm_config.block_gas_limit;
        if gas_limit > block_gas_limit {
            return Err(evm::TransactionError::GasLimitExceedsBlockGasLimit {
                gas_limit,
                block_gas_limit,
            });
        }

        let base_fee = u128::from(evm_config.base_fee);
        match transaction.kind() {
            evm::TransactionKind::Legacy | evm::TransactionKind::Eip2930 => {
                let gas_price = transaction
                    .gas_price()
                    .ok_or(evm::TransactionError::MissingGasPrice)?;
                if gas_price < base_fee {
                    return Err(evm::TransactionError::GasPriceBelowBaseFee {
                        gas_price,
                        base_fee,
                    });
                }
            }
            evm::TransactionKind::Eip1559 | evm::TransactionKind::Eip7702 => {
                // `max_fee_per_gas` is still meaningful on Casper as the user's
                // dynamic-fee total price cap. It must at least cover the
                // configured EVM base fee; with the priority fee forced to zero
                // below, this cap is what lets Ethereum tooling submit typed
                // dynamic-fee transactions without implying transaction
                // priority based on gas parameters.
                let max_fee_per_gas = transaction.max_fee_per_gas();
                if max_fee_per_gas < base_fee {
                    return Err(evm::TransactionError::MaxFeePerGasBelowBaseFee {
                        max_fee_per_gas,
                        base_fee,
                    });
                }
                let max_priority_fee_per_gas = transaction.max_priority_fee_per_gas().unwrap_or(0);
                if max_priority_fee_per_gas != 0 {
                    // Casper does not currently prioritize transactions based
                    // on transaction gas parameters. Accepting a non-zero
                    // EIP-1559 priority fee would charge users for a priority
                    // signal that the node does not honor, so this prototype
                    // only accepts EIP-1559 as a max-fee compatibility
                    // envelope with zero priority fee.
                    return Err(evm::TransactionError::NonZeroMaxPriorityFeePerGas {
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
