pub mod executor;

use bytes::Bytes;
use thiserror::Error;

use casper_executor_wasm_common::flags::ReturnFlags;

/// Interface version for the Wasm host functions.
///
/// This defines behavior of the Wasm execution environment i.e. the host behavior, serialiation,
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

const CALLEE_SUCCEED: u32 = 0;
const CALLEE_REVERTED: u32 = 1;
const CALLEE_TRAPPED: u32 = 2;
const CALLEE_GAS_DEPLETED: u32 = 3;
const CALLEE_NOT_CALLABLE: u32 = 4;

/// Represents the result of a host function call.
///
/// 0 is used as a success.
#[derive(Debug, Error)]
pub enum HostError {
    /// Callee contract reverted.
    #[error("callee reverted")]
    CalleeReverted,
    /// Called contract trapped.
    #[error("callee trapped: {0}")]
    CalleeTrapped(TrapCode),
    /// Called contract reached gas limit.
    #[error("callee gas depleted")]
    CalleeGasDepleted,
    /// Called contract is not callable.
    #[error("not callable")]
    NotCallable,
}

pub type HostResult = Result<(), HostError>;

impl HostError {
    /// Converts the host error into a u32.
    fn into_u32(self) -> u32 {
        match self {
            HostError::CalleeReverted => CALLEE_REVERTED,
            HostError::CalleeTrapped(_) => CALLEE_TRAPPED,
            HostError::CalleeGasDepleted => CALLEE_GAS_DEPLETED,
            HostError::NotCallable => CALLEE_NOT_CALLABLE,
        }
    }
}

/// Converts a host result into a u32.
pub fn u32_from_host_result(result: HostResult) -> u32 {
    match result {
        Ok(_) => CALLEE_SUCCEED,
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
pub enum VMError {
    #[error("Return 0x{flags:?} {data:?}")]
    Return {
        flags: ReturnFlags,
        data: Option<Bytes>,
    },
    #[error("export: {0}")]
    Export(ExportError),
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
    gas_limit: u64,
    memory_limit: u32,
}

impl Config {
    pub fn gas_limit(&self) -> u64 {
        self.gas_limit
    }

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

    /// Build the configuration.
    pub fn build(self) -> Config {
        let gas_limit = self.gas_limit.expect("Required field");
        let memory_limit = self.memory_limit.expect("Required field");
        Config {
            gas_limit,
            memory_limit,
        }
    }
}

#[derive(Debug, Copy, Clone)]
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

    fn config(&self) -> &Config;
    fn context(&self) -> &Self::Context;
    fn context_mut(&mut self) -> &mut Self::Context;
    /// Returns currently running *unmodified* bytecode.
    fn bytecode(&self) -> Bytes;

    /// Check if an export is present in the module.
    fn has_export(&self, name: &str) -> bool;

    fn memory_read(&self, offset: u32, size: usize) -> VMResult<Vec<u8>> {
        let mut vec = vec![0; size];
        self.memory_read_into(offset, &mut vec)?;
        Ok(vec)
    }
    fn memory_read_into(&self, offset: u32, output: &mut [u8]) -> VMResult<()>;
    fn memory_write(&self, offset: u32, data: &[u8]) -> VMResult<()>;
    /// Allocates memory inside the Wasm VM by calling an export.
    ///
    /// Error is a type-erased error coming from the VM itself.
    fn alloc(&mut self, idx: u32, size: usize, ctx: u32) -> VMResult<u32>;
    /// Returns the amount of gas used.
    fn gas_consumed(&mut self) -> MeteringPoints;
    /// Set the amount of gas used.
    fn consume_gas(&mut self, value: u64) -> MeteringPoints;
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
}

#[derive(Debug)]
pub struct GasUsage {
    /// The amount of gas used by the execution.
    gas_limit: u64,
    /// The amount of gas remaining after the execution.
    remaining_points: u64,
}

impl GasUsage {
    pub fn new(gas_limit: u64, remaining_points: u64) -> Self {
        GasUsage {
            gas_limit,
            remaining_points,
        }
    }

    pub fn gas_spent(&self) -> u64 {
        debug_assert!(self.remaining_points <= self.gas_limit);
        self.gas_limit - self.remaining_points
    }

    pub fn gas_limit(&self) -> u64 {
        self.gas_limit
    }

    pub fn remaining_points(&self) -> u64 {
        self.remaining_points
    }
}

/// A trait that represents a Wasm instance.
pub trait WasmInstance {
    type Context;

    fn call_export(&mut self, name: &str) -> (Result<(), VMError>, GasUsage);
    fn teardown(self) -> Self::Context;
}
