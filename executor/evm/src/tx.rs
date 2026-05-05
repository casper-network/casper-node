//! Translation from Casper-owned EVM requests into revm transaction environments.

use casper_types::evm;
use revm::{
    context::TxEnv,
    primitives::{Address, Bytes, TxKind, B256, U256},
};

use crate::{Error, ExecuteKind};

pub(crate) fn build_tx_env(config: &evm::EvmConfig, kind: &ExecuteKind) -> Result<TxEnv, Error> {
    let tx_env = match kind {
        ExecuteKind::Transaction(transaction) => {
            let mut builder = TxEnv::builder()
                .caller(to_revm_address(transaction.from()))
                .gas_limit(transaction.gas_limit())
                .value(to_revm_u256(transaction.value()))
                .data(Bytes::from(transaction.input().to_vec()))
                .nonce(transaction.nonce())
                .chain_id(transaction.chain_id().or(Some(config.chain_id)));

            builder = match transaction.kind() {
                evm::TransactionKind::Legacy | evm::TransactionKind::Eip2930 => builder.gas_price(
                    transaction
                        .gas_price()
                        .unwrap_or_else(|| transaction.max_fee_per_gas()),
                ),
                evm::TransactionKind::Eip1559 => builder
                    .max_fee_per_gas(transaction.max_fee_per_gas())
                    .gas_priority_fee(transaction.max_priority_fee_per_gas()),
            };

            builder = match transaction.to() {
                Some(address) => builder.kind(TxKind::Call(to_revm_address(address))),
                None => builder.kind(TxKind::Create),
            };

            builder
                .build()
                .map_err(|error| Error::Transaction(format!("{error:?}")))?
        }
        ExecuteKind::Call(call) => TxEnv::builder()
            .caller(to_revm_address(call.from))
            .gas_limit(call.gas_limit)
            .gas_price(call.gas_price)
            .kind(match call.to {
                Some(address) => TxKind::Call(to_revm_address(address)),
                None => TxKind::Create,
            })
            .value(to_revm_u256(call.value))
            .data(Bytes::from(call.input.clone()))
            .nonce(call.nonce)
            .chain_id(Some(config.chain_id))
            .build()
            .map_err(|error| Error::Transaction(format!("{error:?}")))?,
    };
    Ok(tx_env)
}

pub(crate) fn to_revm_address(address: evm::Address) -> Address {
    Address::from(address.value())
}

pub(crate) fn from_revm_address(address: Address) -> evm::Address {
    evm::Address::new(address.into_array())
}

pub(crate) fn to_revm_hash(hash: evm::Hash) -> B256 {
    B256::from(hash.value())
}

pub(crate) fn from_revm_hash(hash: B256) -> evm::Hash {
    evm::Hash::new(hash.0)
}

pub(crate) fn to_revm_u256(value: evm::Hash) -> U256 {
    U256::from_be_slice(value.as_bytes())
}

pub(crate) fn from_revm_u256(value: U256) -> evm::Hash {
    evm::Hash::new(value.to_be_bytes())
}
