pub mod chain;
pub mod executor;
pub(crate) mod host;
pub mod storage;
pub(crate) mod wasm_backend;

use bytes::Bytes;

use executor::Executor;
use storage::GlobalStateReader;
use thiserror::Error;
use vm_common::flags::ReturnFlags;
use wasm_backend::{wasmer::WasmerInstance, Context, GasUsage, PreparationError, WasmInstance};

const CALLEE_SUCCEED: u32 = 0;
const CALLEE_REVERTED: u32 = 1;
const CALLEE_TRAPPED: u32 = 2;
const CALLEE_GAS_DEPLETED: u32 = 3;

/// Represents the result of a host function call.
///
/// 0 is used as a success.
#[derive(Debug)]
#[repr(u32)]
pub enum HostError {
    /// Callee contract reverted.
    CalleeReverted,
    /// Called contract trapped.
    CalleeTrapped(TrapCode),
    /// Called contract reached gas limit.
    CalleeGasDepleted,
}

// no revert: output
// revert: output
// trap: no output
// gas depleted: no output

type HostResult = Result<(), HostError>;

impl HostError {
    /// Converts the host error into a u32.
    pub(crate) fn into_u32(self) -> u32 {
        match self {
            HostError::CalleeReverted => CALLEE_REVERTED,
            HostError::CalleeTrapped(_) => CALLEE_TRAPPED,
            HostError::CalleeGasDepleted => CALLEE_GAS_DEPLETED,
        }
    }
}

/// Converts a host result into a u32.
pub(crate) fn u32_from_host_result(result: HostResult) -> u32 {
    match result {
        Ok(_) => CALLEE_SUCCEED,
        Err(host_error) => host_error.into_u32(),
    }
}

/// `WasmEngine` is a struct that represents a WebAssembly engine.
///
/// This struct is used to encapsulate the operations and state of a WebAssembly engine.
/// Currently, it does not hold any data (`()`), but it can be extended in the future to hold
/// different compiler instances, configuration options, cached artifacts, state information,
/// etc.
///
/// # Examples
///
/// Basic usage:
///
/// ```
/// let engine = WasmEngine::new();
/// ```
#[derive(Clone)]
pub struct WasmEngine(());

/// Errors that can occur when resolving imports.
#[derive(Debug, Error)]
pub enum Resolver {
    #[error("export {name} not found.")]
    Export { name: String },
    /// Trying to call a function pointer by index.
    #[error("function pointer {index} not found.")]
    Table { index: u32 },
}

#[derive(Error, Debug)]
pub enum ExportError {
    /// An error than occurs when the exported type and the expected type
    /// are incompatible.
    #[error("Incompatible Export Type")]
    IncompatibleType,
    /// This error arises when an export is missing
    #[error("Missing export {0}")]
    Missing(String),
}

#[derive(Error, Debug)]
#[non_exhaustive]
pub enum MemoryError {
    /// Memory access is outside heap bounds.
    #[error("memory access out of bounds")]
    HeapOutOfBounds,
    /// Address calculation overflow.
    #[error("address calculation overflow")]
    Overflow,
    /// String is not valid UTF-8.
    #[error("string is not valid utf-8")]
    NonUtf8String,
}

/// Wasm trap code.
#[derive(Debug, Error)]
pub enum TrapCode {
    /// Trap code for out of bounds memory access.
    #[error("call stack exhausted")]
    StackOverflow,
    /// Trap code for out of bounds memory access.
    #[error("out of bounds memory access")]
    MemoryOutOfBounds,
    /// Trap code for out of bounds table access.
    #[error("undefined element: out of bounds table access")]
    TableAccessOutOfBounds,
    /// Trap code for indirect call to null.
    #[error("uninitialized element")]
    IndirectCallToNull,
    /// Trap code for indirect call type mismatch.
    #[error("indirect call type mismatch")]
    BadSignature,
    /// Trap code for integer overflow.
    #[error("integer overflow")]
    IntegerOverflow,
    /// Trap code for division by zero.
    #[error("integer divide by zero")]
    IntegerDivisionByZero,
    /// Trap code for invalid conversion to integer.
    #[error("invalid conversion to integer")]
    BadConversionToInteger,
    /// Trap code for unreachable code reached triggered by unreachable instruction.
    #[error("unreachable")]
    UnreachableCodeReached,
}

/// The outcome of a call.
/// We can fold all errors into this type and return it from the host functions and remove Outcome
/// type.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum VMError {
    #[error("Return 0x{flags:?} {data:?}")]
    Return {
        flags: ReturnFlags,
        data: Option<Bytes>,
    },
    #[error("Out of gas")]
    OutOfGas,
    /// Error while executing Wasm: traps, memory access errors, etc.
    ///
    /// NOTE: for supporting multiple different backends we may want to abstract this a bit and
    /// extract memory access errors, trap codes, and unify error reporting.
    #[error("Trap: {0}")]
    Trap(TrapCode),
}

impl VMError {
    /// Returns the output data if the error is a `Return` error.
    pub fn into_output_data(self) -> Option<Bytes> {
        match self {
            VMError::Return { data, .. } => data,
            _ => None,
        }
    }
}

/// Result of a VM operation.
pub type VMResult<T> = Result<T, VMError>;

/// Configuration for the Wasm engine.
#[derive(Clone, Debug)]
pub struct Config {
    pub(crate) gas_limit: u64,
    pub(crate) memory_limit: u32,
    pub(crate) input: Bytes,
}

/// Configuration for the Wasm engine.
#[derive(Clone, Debug, Default)]
pub struct ConfigBuilder {
    gas_limit: Option<u64>,
    /// Memory limit in pages.
    memory_limit: Option<u32>,
    /// Input data.
    input: Option<Bytes>,
}

impl ConfigBuilder {
    /// Create a new configuration builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Gas limit in units.
    pub fn with_gas_limit(mut self, gas_limit: u64) -> Self {
        self.gas_limit = Some(gas_limit);
        self
    }

    /// Memory limit denominated in pages.
    pub fn with_memory_limit(mut self, memory_limit: u32) -> Self {
        self.memory_limit = Some(memory_limit);
        self
    }

    /// Pass input data.
    pub fn with_input(mut self, input: Bytes) -> Self {
        self.input = Some(input);
        self
    }

    /// Build the configuration.
    pub fn build(self) -> Config {
        let gas_limit = self.gas_limit.expect("Required field");
        let memory_limit = self.memory_limit.expect("Required field");
        let input = self.input.unwrap_or_default();
        Config {
            gas_limit,
            memory_limit,
            input,
        }
    }
}

impl WasmEngine {
    pub(crate) fn prepare<S: GlobalStateReader + 'static, E: Executor + 'static, C: Into<Bytes>>(
        &self,
        wasm_bytes: C,
        context: Context<S, E>,
        config: Config,
    ) -> Result<impl WasmInstance<S, E>, PreparationError> {
        let wasm_bytes: Bytes = wasm_bytes.into();
        // NOTE: We can add more engines here in the future, e.g. Lucet, Wasmtime, wasmi all configurable at runtime. For now, it's ok to return `impl WasmInstance`.
        let instance = WasmerInstance::from_wasm_bytes(wasm_bytes, context, config)?;
        Ok(instance)
    }

    #[inline]
    pub fn new() -> Self {
        WasmEngine(())
    }
}

impl Default for WasmEngine {
    fn default() -> Self {
        Self::new()
    }
}
