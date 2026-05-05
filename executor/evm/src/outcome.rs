//! Public execution outcome types.

use casper_types::evm;
use revm::context_interface::result::{ExecutionResult, Output};

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
    pub logs: Vec<Log>,
    /// Address created by a successful create transaction.
    pub created_contract_address: Option<evm::Address>,
}

impl ExecutionOutcome {
    pub(crate) fn from_revm_result(result: &ExecutionResult) -> Self {
        match result {
            ExecutionResult::Success {
                gas_used,
                logs,
                output,
                ..
            } => {
                let (output_bytes, created_contract_address) = match output {
                    Output::Call(bytes) => (bytes.to_vec(), None),
                    Output::Create(bytes, address) => {
                        (bytes.to_vec(), address.map(tx::from_revm_address))
                    }
                };
                Self {
                    status: ExecutionStatus::Success,
                    gas_used: *gas_used,
                    output: output_bytes,
                    logs: logs.iter().map(Log::from_revm_log).collect(),
                    created_contract_address,
                }
            }
            ExecutionResult::Revert { gas_used, output } => Self {
                status: ExecutionStatus::Revert,
                gas_used: *gas_used,
                output: output.to_vec(),
                logs: Vec::new(),
                created_contract_address: None,
            },
            ExecutionResult::Halt { gas_used, .. } => Self {
                status: ExecutionStatus::Halt,
                gas_used: *gas_used,
                output: Vec::new(),
                logs: Vec::new(),
                created_contract_address: None,
            },
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
    Halt,
}

/// EVM log entry emitted by a successful transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Log {
    /// Contract address that emitted the log.
    pub address: evm::Address,
    /// Indexed log topics.
    pub topics: Vec<evm::Hash>,
    /// Unindexed log data.
    pub data: Vec<u8>,
}

impl Log {
    fn from_revm_log(log: &revm::primitives::Log) -> Self {
        Self {
            address: tx::from_revm_address(log.address),
            topics: log
                .data
                .topics()
                .iter()
                .copied()
                .map(tx::from_revm_hash)
                .collect(),
            data: log.data.data.to_vec(),
        }
    }
}
