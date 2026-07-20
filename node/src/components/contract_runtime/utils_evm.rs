use casper_executor_evm::BlockContext as EvmBlockContext;
use casper_types::{
    evm::{
        Address as EvmAddress, HaltReason as EvmHaltReason, Receipt as EvmReceipt,
        ReceiptStatus as EvmReceiptStatus,
    },
    BlockTime, Chainspec, PublicKey,
};

pub(super) fn block_context(
    chainspec: &Chainspec,
    block_height: u64,
    block_time: BlockTime,
    proposer: &PublicKey,
) -> EvmBlockContext {
    EvmBlockContext {
        number: block_height,
        timestamp: block_time.value() / 1000,
        beneficiary: EvmAddress::from_block_proposer_public_key(proposer),
        gas_limit: Some(chainspec.evm_config.block_gas_limit),
        base_fee: Some(chainspec.evm_config.base_fee_wei()),
    }
}

pub(super) fn precondition_receipt(effective_gas_price: u128) -> EvmReceipt {
    EvmReceipt {
        status: EvmReceiptStatus::Halt(EvmHaltReason::Unknown),
        gas_used: 0,
        effective_gas_price,
        contract_address: None,
        logs: Vec::new(),
    }
}
