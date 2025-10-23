pub mod executor;
pub mod sandboxed_execution;

use bytes::Bytes;
use executor::ExecuteError;
use serde::Serialize;
use thiserror::Error;

use casper_executor_wasm_common::{
    error::{CallError, TrapCode, CALLEE_SUCCEEDED},
    flags::ReturnFlags,
};
use casper_types::bytesrepr::Error as BytesreprError;

#[cfg(test)]
pub use sandboxed_execution::SandboxedExecutionRequestBuilder;
pub use sandboxed_execution::{
    SandboxedExecutionError, SandboxedExecutionRequest, SandboxedExecutionResult,
};

/// Interface version for the Wasm host functions.
///
/// This defines behavior of the Wasm execution environment i.e. the host behavior, serialization,
/// etc.
///
/// Only the highest `interface_version_X` is taken from the imports table which means Wasm has to
/// support X-1, X-2 versions as well.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct InterfaceVersion(u32);

impl From<u32> for InterfaceVersion {
    fn from(value: u32) -> Self {
        InterfaceVersion(value)
    }
}

pub type HostResult = Result<(), CallError>;

/// Converts a host result into the corresponding u32 value.
#[must_use]
pub fn u32_from_host_result(result: HostResult) -> u32 {
    match result {
        Ok(()) => CALLEE_SUCCEEDED,
        Err(host_error) => host_error.into_u32(),
    }
}

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
    #[error("incompatible type")]
    IncompatibleType,
    /// This error arises when an export is missing
    #[error("missing export {0}")]
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

/// Represents a catastrophic internal host error.
#[derive(Error, Debug, Clone, Serialize)]
pub enum FatalHostError {
    #[error("type conversion failure")]
    TypeConversion,
    #[error("contract already exists")]
    ContractAlreadyExists,
    #[error("tracking copy error")]
    TrackingCopy,
    #[error("failed building execution request: {0}")]
    ExecuteRequestBuildFailure(&'static str),
    #[error("unexpected entity kind")]
    UnexpectedEntityKind,
    #[error("failed reading total balance")]
    TotalBalanceReadFailure,
    #[error("total balance exceeded u64::MAX")]
    TotalBalanceOverflow,
    #[error("remaining gas exceeded the gas limit")]
    RemainingGasExceedsGasLimit,
    #[error("message did not have a checksum")]
    MessageChecksumMissing,
    #[error("missing system contract")]
    MissingSystemContract,
    #[error("dispatching system contract failed")]
    DispatchSystemContract,
    #[error("incompatible type: expected {expected}, found {found}")]
    UnexpectedStoredValueVariant { expected: String, found: String },
    #[error("Error on bytesrepr serialization/deserialization. Details: {0}")]
    Bytesrepr(BytesreprError),
    #[error(
        "Successfull execution of VM1 contract returned an output which is undefined behavior"
    )]
    UnexpectedOutput,
    #[error("Executor in a state that made in unable to proceed. Details: {0}")]
    CorruptExecutionState(String),
    #[error("Error when creating config: {0}")]
    ConfigBuilderError(String),
    #[error("invalid public key")]
    InvalidPublicKey,
    #[error("invalid entity address")]
    InvalidEntityAddr,
    #[error("serialization failure")]
    Serialization,
    #[error("Unable to determine the cost of ffi call")]
    UnableToValueFFICall,
}

/// The outcome of a call.
/// We can fold all errors into this type and return it from the host functions and remove Outcome
/// type.
#[derive(Debug, Error)]
pub enum VMError {
    /// NOTE: This will kill the node.
    #[error("Fatal host error: {0}")]
    Fatal(#[from] FatalHostError),

    #[error("Return 0x{flags:?} {data:?}")]
    Return {
        flags: ReturnFlags,
        data: Option<Bytes>,
    },
    #[error("export: {0}")]
    Export(ExportError),
    #[error("missing table entry: {0}")]
    AllocError(String),
    #[error("Out of gas")]
    OutOfGas,
    /// Error while executing Wasm: traps, memory access errors, etc.
    ///
    /// NOTE: for supporting multiple different backends we may want to abstract this a bit and
    /// extract memory access errors, trap codes, and unify error reporting.
    #[error("Trap: {0}")]
    Trap(TrapCode),
    #[error("Execute error: {0}")]
    Execute(#[from] ExecuteError),
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
    gas_limit: u64,
    memory_limit: u32,
}

impl Config {
    #[must_use]
    pub fn gas_limit(&self) -> u64 {
        self.gas_limit
    }

    #[must_use]
    pub fn memory_limit(&self) -> u32 {
        self.memory_limit
    }
}

/// Configuration for the Wasm engine.
#[derive(Clone, Debug, Default)]
pub struct ConfigBuilder {
    gas_limit: Option<u64>,
    /// Memory limit in pages.
    memory_limit: Option<u32>,
}

impl ConfigBuilder {
    /// Create a new configuration builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Gas limit in units.
    #[must_use]
    pub fn with_gas_limit(mut self, gas_limit: u64) -> Self {
        self.gas_limit = Some(gas_limit);
        self
    }

    /// Memory limit denominated in pages.
    #[must_use]
    pub fn with_memory_limit(mut self, memory_limit: u32) -> Self {
        self.memory_limit = Some(memory_limit);
        self
    }

    /// Build the configuration.
    pub fn build(self) -> Result<Config, ConfigBuilderError> {
        let gas_limit = self
            .gas_limit
            .ok_or(ConfigBuilderError::MissingField("gas_limit".to_owned()))?;
        let memory_limit = self
            .memory_limit
            .ok_or(ConfigBuilderError::MissingField("memory_limit".to_owned()))?;
        Ok(Config {
            gas_limit,
            memory_limit,
        })
    }
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ConfigBuilderError {
    #[error("Could not build config. Missing field: {0}")]
    MissingField(String),
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MeteringPoints {
    Remaining(u64),
    Exhausted,
}

impl MeteringPoints {
    pub fn try_into_remaining(self) -> Result<u64, Self> {
        if let Self::Remaining(v) = self {
            Ok(v)
        } else {
            Err(self)
        }
    }
}

/// An abstraction over the 'caller' object of a host function that works for any Wasm VM.
///
/// This allows access for important instances such as the context object that was passed to the
/// instance, wasm linear memory access, etc.
pub trait Caller {
    type Context;
    type Executor: crate::executor::Executor;

    fn context(&self) -> &Self::Context;
    fn context_mut(&mut self) -> &mut Self::Context;
    /// Returns currently running *unmodified* bytecode.
    fn bytecode(&self) -> Bytes;

    /// Check if an export is present in the module.
    fn has_export(&self, name: &str) -> VMResult<bool>;

    fn memory_read(&self, offset: u32, size: usize) -> VMResult<Vec<u8>>;
    fn memory_read_into(&self, offset: u32, output: &mut [u8]) -> VMResult<()>;
    fn memory_write(&self, offset: u32, data: &[u8]) -> VMResult<()>;
    /// Allocates memory inside the Wasm VM by calling an export.
    ///
    /// Error is a type-erased error coming from the VM itself.
    fn alloc(&mut self, idx: u32, size: usize, ctx: u32) -> VMResult<u32>;
    /// Returns the amount of gas remaining.
    fn get_remaining_points(&mut self) -> VMResult<MeteringPoints>;
    /// Check for gas exhaustion, then reduce remaining by amount if able.
    fn consume_gas(&mut self, value: u64) -> VMResult<()>;
    /// Returns a reference to the executor used by the current instance.
    fn executor(&self) -> &Self::Executor;
}

#[derive(Debug, Error)]
pub enum WasmPreparationError {
    #[error("Missing export {0}")]
    MissingExport(String),
    #[error("Compile error: {0}")]
    Compile(String),
    #[error("Memory instantiation error: {0}")]
    Memory(String),
    #[error("Instantiation error: {0}")]
    Instantiation(String),
    #[error("Internal host error {0}")]
    Internal(#[from] FatalHostError),
}

#[derive(Debug)]
pub struct GasUsage {
    /// The amount of gas used by the execution.
    gas_limit: u64,
    /// The amount of gas remaining after the execution.
    remaining_points: u64,
}

impl GasUsage {
    #[must_use]
    pub fn new(gas_limit: u64, remaining_points: u64) -> Self {
        GasUsage {
            gas_limit,
            remaining_points,
        }
    }

    #[must_use]
    pub fn gas_spent(&self) -> u64 {
        debug_assert!(self.remaining_points <= self.gas_limit);
        self.gas_limit - self.remaining_points
    }

    #[must_use]
    pub fn gas_limit(&self) -> u64 {
        self.gas_limit
    }

    #[must_use]
    pub fn remaining_points(&self) -> u64 {
        self.remaining_points
    }

    /// Spend a given amount of gas. If the amount exceeds the remaining gas, it will be set to 0.
    pub fn spend(&mut self, amount: u64) {
        self.remaining_points = self.remaining_points.saturating_sub(amount);
    }
}

/// A trait that represents a Wasm instance.
pub trait WasmInstance {
    type Context;

    fn call_export(&mut self, name: &str) -> (Result<(), VMError>, GasUsage);
    fn teardown(self) -> Self::Context;
}
