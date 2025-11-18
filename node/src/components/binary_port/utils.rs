use casper_executor_wasm_interface::SandboxedExecutionRequest as InnerSandboxedExecutionRequest;
use casper_types::{
    BlockHeader, BlockTime, Gas, InitiatorAddr, InvalidTransaction, PricingHandling, Transaction,
    TransactionConfig,
};

use crate::types::MetaTransaction;

pub(super) fn transaction_to_sandbox_request(
    transaction: Transaction,
    block_header: BlockHeader,
    gas_limit: u64,
) -> Result<InnerSandboxedExecutionRequest, InvalidTransaction> {
    // Filling it any data for pricing handling and transaction config since
    // these parts of the transaction play no role in sandbox execution
    let pricing_handling = PricingHandling::default();
    let transaction_config = TransactionConfig::default();
    let meta_transaction =
        MetaTransaction::from_transaction(&transaction, pricing_handling, &transaction_config)?;
    let account_hash = match &meta_transaction.initiator_addr() {
        InitiatorAddr::PublicKey(public_key) => public_key.to_account_hash(),
        InitiatorAddr::AccountHash(account_hash) => *account_hash,
    };
    Ok(InnerSandboxedExecutionRequest {
        state_hash: *block_header.state_root_hash(),
        block_height: block_header.height(),
        block_time: BlockTime::new(block_header.timestamp().millis()),
        parent_block_hash: *block_header.parent_hash(),
        protocol_version: block_header.protocol_version(),
        target: meta_transaction.target_retrofit(),
        entry_point: meta_transaction.entry_point(),
        initiator: account_hash,
        args: meta_transaction.transaction_args_retrofit(),
        authorization_keys: meta_transaction.authorization_keys(),
        gas_limit: Gas::new(gas_limit),
    })
}
