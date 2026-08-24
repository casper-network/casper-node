use crate::types::transaction::WasmV2TransactionInput;
use casper_execution_engine::engine_state::SessionInputData;
use casper_types::{EvmTransaction, TransactionArgs, TransactionEntryPoint};
use std::fmt::Formatter;

#[derive(Clone, Debug)]
pub(crate) enum ProcessRequest {
    Unknown,
    NativeMint {
        session_args: TransactionArgs,
        entry_point: TransactionEntryPoint,
    },
    NativeAuction {
        session_args: TransactionArgs,
        entry_point: TransactionEntryPoint,
    },
    WasmV1 {
        session_input_data: SessionInputData,
    },
    WasmV2 {
        transaction_input: WasmV2TransactionInput,
    },
    EvmV1 {
        evm_txn: EvmTransaction,
        base_fee_wei: u128,
        effective_gas_price: u128,
        block_gas_limit: u64,
    },
    NoExecEvm {
        effective_gas_price: u128,
    },
    NoExec,
}

impl ProcessRequest {
    pub(crate) fn requires_processing_hold(&self) -> bool {
        match self {
            ProcessRequest::NativeMint { .. }
            | ProcessRequest::NativeAuction { .. }
            | ProcessRequest::WasmV1 { .. }
            | ProcessRequest::WasmV2 { .. }
            | ProcessRequest::EvmV1 { .. } => true,
            ProcessRequest::NoExecEvm { .. } | ProcessRequest::NoExec | ProcessRequest::Unknown => {
                false
            }
        }
    }
}

impl std::fmt::Display for ProcessRequest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ProcessRequest::Unknown => {
                write!(formatter, "unknown process request")
            }
            ProcessRequest::NativeMint { .. } => write!(formatter, "native mint process request"),
            ProcessRequest::NativeAuction { .. } => {
                write!(formatter, "native auction process request")
            }
            ProcessRequest::WasmV1 { .. } => write!(formatter, "wasm_v1 process request"),
            ProcessRequest::WasmV2 { .. } => write!(formatter, "wasm_v2 process request"),
            ProcessRequest::EvmV1 { .. } => write!(formatter, "evm_v1 process request"),
            ProcessRequest::NoExecEvm { .. } => write!(formatter, "no_exec_evm process request"),
            ProcessRequest::NoExec => write!(formatter, "no_exec process request"),
        }
    }
}
