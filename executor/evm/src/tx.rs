//! Translation from Casper-owned EVM requests into revm transaction environments.

use alloy_eips::eip7702::{
    Authorization as RevmAuthorization, SignedAuthorization as RevmSignedAuthorization,
};
use casper_types::{evm, BlockHash, EvmConfig, EvmTransactionKind, U256 as CasperU256};
use revm::{
    context::TxEnv,
    primitives::{Address, Bytes, TxKind, B256, U256},
};

use crate::{Error, ExecuteKind};

pub(crate) fn build_tx_env(config: &EvmConfig, kind: &ExecuteKind) -> Result<TxEnv, Error> {
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
                EvmTransactionKind::Legacy | EvmTransactionKind::Eip2930 => builder.gas_price(
                    transaction
                        .gas_price()
                        .unwrap_or_else(|| transaction.max_fee_per_gas()),
                ),
                EvmTransactionKind::Eip1559 => {
                    let max_priority_fee_per_gas =
                        Some(transaction.max_priority_fee_per_gas().unwrap_or(0));
                    // Preserve the EIP-1559 fields when translating into
                    // revm. Node config compliance currently only admits
                    // zero-priority-fee EIP-1559 transactions because Casper
                    // does not prioritize transactions based on transaction
                    // gas parameters, but the executor remains a faithful
                    // typed-transaction adapter.
                    builder
                        .max_fee_per_gas(transaction.max_fee_per_gas())
                        .gas_priority_fee(max_priority_fee_per_gas)
                }
                EvmTransactionKind::Eip7702 => {
                    let max_priority_fee_per_gas =
                        Some(transaction.max_priority_fee_per_gas().unwrap_or(0));
                    builder
                        .max_fee_per_gas(transaction.max_fee_per_gas())
                        .gas_priority_fee(max_priority_fee_per_gas)
                        .tx_type(Some(evm::EIP7702_TRANSACTION_TYPE_ID))
                        .authorization_list_signed(
                            transaction
                                .authorization_list()
                                .iter()
                                .map(to_revm_authorization)
                                .collect(),
                        )
                }
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

fn to_revm_authorization(authorization: &evm::SetCodeAuthorization) -> RevmSignedAuthorization {
    RevmSignedAuthorization::new_unchecked(
        RevmAuthorization {
            chain_id: to_revm_u256(authorization.chain_id),
            address: to_revm_address(authorization.address),
            nonce: authorization.nonce,
        },
        authorization.y_parity,
        to_revm_u256(authorization.r),
        to_revm_u256(authorization.s),
    )
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

pub(crate) fn from_revm_topic(topic: B256) -> evm::Topic {
    evm::Topic::new(topic.0)
}

pub(crate) fn to_revm_block_hash(block_hash: BlockHash) -> B256 {
    let mut bytes = [0u8; evm::HASH_LENGTH];
    bytes.copy_from_slice(block_hash.as_ref());
    B256::from(bytes)
}

pub(crate) fn to_revm_u256(value: CasperU256) -> U256 {
    let mut bytes = [0u8; 32];
    value.to_big_endian(&mut bytes);
    U256::from_be_slice(&bytes)
}

pub(crate) fn to_revm_storage_word(value: CasperU256) -> U256 {
    to_revm_u256(value)
}

pub(crate) fn from_revm_storage_word(value: U256) -> CasperU256 {
    let bytes = value.to_be_bytes::<32>();
    CasperU256::from_big_endian(&bytes)
}
