use casper_types::{
    EvmTransactionError, InvalidDeploy, InvalidTransaction, InvalidTransactionV1, Transaction,
};

/// Type representing results of the speculative execution.
#[derive(Debug)]
pub enum SpeculativeExecutionResult {
    InvalidTransaction(InvalidTransaction),
    WasmV1(Box<casper_binary_port::SpeculativeExecutionResult>),
    Evm(Box<casper_binary_port::EvmSpeculativeExecutionResult>),
}

impl SpeculativeExecutionResult {
    pub fn invalid_gas_limit(transaction: Transaction) -> Self {
        match transaction {
            Transaction::Deploy(_) => SpeculativeExecutionResult::InvalidTransaction(
                InvalidTransaction::Deploy(InvalidDeploy::UnableToCalculateGasLimit),
            ),
            Transaction::V1(_) => SpeculativeExecutionResult::InvalidTransaction(
                InvalidTransaction::V1(InvalidTransactionV1::UnableToCalculateGasLimit),
            ),
            Transaction::Evm(_) => SpeculativeExecutionResult::InvalidTransaction(
                InvalidTransaction::Evm(EvmTransactionError::Decode(
                    "EVM transactions are not routed through contract runtime".to_string(),
                )),
            ),
        }
    }

    pub fn invalid_transaction(error: InvalidTransaction) -> Self {
        SpeculativeExecutionResult::InvalidTransaction(error)
    }
}
