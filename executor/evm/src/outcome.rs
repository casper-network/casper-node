//! Public execution outcome types.

use casper_types::{evm, U512};
use revm::context_interface::result::{
    ExecutionResult, HaltReason as RevmHaltReason, OutOfGasError as RevmOutOfGasError, Output,
};

use crate::tx;

/// Result returned by [`crate::EvmExecutor::execute`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionOutcome {
    /// High-level EVM execution status.
    pub status: ExecutionStatus,
    /// Gas consumed by execution.
    pub gas_used: u64,
    /// Return or revert bytes.
    pub output: Vec<u8>,
    /// Logs emitted by successful execution.
    pub logs: Vec<evm::Log>,
    /// Address created by a successful create transaction.
    pub created_contract_address: Option<evm::Address>,
    /// Whole motes discarded when final EVM balances are rounded down for persistence.
    pub dust_motes: U512,
}

impl ExecutionOutcome {
    pub(crate) fn from_revm_result(result: &ExecutionResult, dust_motes: U512) -> Self {
        match result {
            ExecutionResult::Success {
                gas, logs, output, ..
            } => {
                let (output_bytes, created_contract_address) = match output {
                    Output::Call(bytes) => (bytes.to_vec(), None),
                    Output::Create(bytes, address) => {
                        (bytes.to_vec(), address.map(tx::from_revm_address))
                    }
                };
                Self {
                    status: ExecutionStatus::Success,
                    gas_used: gas.tx_gas_used(),
                    output: output_bytes,
                    logs: logs.iter().map(from_revm_log).collect(),
                    created_contract_address,
                    dust_motes,
                }
            }
            ExecutionResult::Revert { gas, output, .. } => Self {
                status: ExecutionStatus::Revert,
                gas_used: gas.tx_gas_used(),
                output: output.to_vec(),
                logs: Vec::new(),
                created_contract_address: None,
                dust_motes,
            },
            ExecutionResult::Halt { gas, reason, .. } => Self {
                status: ExecutionStatus::Halt(from_revm_halt_reason(reason)),
                gas_used: gas.tx_gas_used(),
                output: Vec::new(),
                logs: Vec::new(),
                created_contract_address: None,
                dust_motes,
            },
        }
    }

    /// Converts this execution outcome into EVM receipt data.
    pub fn to_receipt(&self, effective_gas_price: u128) -> evm::Receipt {
        let status = match self.status {
            ExecutionStatus::Success => evm::ReceiptStatus::Success,
            ExecutionStatus::Revert => evm::ReceiptStatus::Revert,
            ExecutionStatus::Halt(reason) => evm::ReceiptStatus::Halt(reason),
        };
        evm::Receipt {
            status,
            gas_used: self.gas_used,
            effective_gas_price,
            contract_address: self.created_contract_address,
            logs: self.logs.clone(),
        }
    }
}

/// High-level EVM execution status.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionStatus {
    /// Execution completed successfully.
    Success,
    /// Execution reverted and returned revert bytes.
    Revert,
    /// Execution halted, usually consuming all supplied gas.
    Halt(evm::HaltReason),
}

fn from_revm_halt_reason(reason: &RevmHaltReason) -> evm::HaltReason {
    match reason {
        RevmHaltReason::OutOfGas(reason) => {
            evm::HaltReason::OutOfGas(from_revm_out_of_gas_error(*reason))
        }
        RevmHaltReason::OpcodeNotFound => evm::HaltReason::OpcodeNotFound,
        RevmHaltReason::InvalidFEOpcode => evm::HaltReason::InvalidFEOpcode,
        RevmHaltReason::InvalidJump => evm::HaltReason::InvalidJump,
        RevmHaltReason::NotActivated => evm::HaltReason::NotActivated,
        RevmHaltReason::StackUnderflow => evm::HaltReason::StackUnderflow,
        RevmHaltReason::StackOverflow => evm::HaltReason::StackOverflow,
        RevmHaltReason::OutOfOffset => evm::HaltReason::OutOfOffset,
        RevmHaltReason::CreateCollision => evm::HaltReason::CreateCollision,
        RevmHaltReason::PrecompileError | RevmHaltReason::PrecompileErrorWithContext(_) => {
            evm::HaltReason::PrecompileError
        }
        RevmHaltReason::NonceOverflow => evm::HaltReason::NonceOverflow,
        RevmHaltReason::CreateContractSizeLimit => evm::HaltReason::CreateContractSizeLimit,
        RevmHaltReason::CreateContractStartingWithEF => {
            evm::HaltReason::CreateContractStartingWithEF
        }
        RevmHaltReason::CreateInitCodeSizeLimit => evm::HaltReason::CreateInitCodeSizeLimit,
        RevmHaltReason::OverflowPayment => evm::HaltReason::OverflowPayment,
        RevmHaltReason::StateChangeDuringStaticCall => evm::HaltReason::StateChangeDuringStaticCall,
        RevmHaltReason::CallNotAllowedInsideStatic => evm::HaltReason::CallNotAllowedInsideStatic,
        RevmHaltReason::OutOfFunds => evm::HaltReason::OutOfFunds,
        RevmHaltReason::CallTooDeep => evm::HaltReason::CallTooDeep,
    }
}

fn from_revm_out_of_gas_error(error: RevmOutOfGasError) -> evm::OutOfGasError {
    match error {
        RevmOutOfGasError::Basic => evm::OutOfGasError::Basic,
        RevmOutOfGasError::MemoryLimit => evm::OutOfGasError::MemoryLimit,
        RevmOutOfGasError::Memory => evm::OutOfGasError::Memory,
        RevmOutOfGasError::Precompile => evm::OutOfGasError::Precompile,
        RevmOutOfGasError::InvalidOperand => evm::OutOfGasError::InvalidOperand,
        RevmOutOfGasError::ReentrancySentry => evm::OutOfGasError::ReentrancySentry,
    }
}

fn from_revm_log(log: &revm::primitives::Log) -> evm::Log {
    evm::Log {
        address: tx::from_revm_address(log.address),
        topics: log
            .data
            .topics()
            .iter()
            .copied()
            .map(tx::from_revm_topic)
            .collect(),
        data: log.data.data.to_vec().into(),
    }
}
