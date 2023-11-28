pub mod backend;
pub mod chain;
pub(crate) mod host;
pub mod storage;

use bytes::Bytes;

use backend::{wasmer::WasmerInstance, Context, Error as BackendError, WasmInstance};
use storage::Storage;
use thiserror::Error;

struct Arguments {
    bytes: Bytes,
}

#[derive(Clone)]
pub struct VM;

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

#[derive(Debug, Error)]
pub enum TrapCode {
    #[error("call stack exhausted")]
    StackOverflow,
    #[error("out of bounds memory access")]
    HeapAccessOutOfBounds,
    #[error("misaligned heap")]
    HeapMisaligned,
    #[error("undefined element: out of bounds table access")]
    TableAccessOutOfBounds,
    #[error("uninitialized element")]
    IndirectCallToNull,
    #[error("indirect call type mismatch")]
    BadSignature,
    #[error("integer overflow")]
    IntegerOverflow,
    #[error("integer divide by zero")]
    IntegerDivisionByZero,
    #[error("invalid conversion to integer")]
    BadConversionToInteger,
    #[error("unreachable")]
    UnreachableCodeReached,
}

/// The outcome of a call.
/// We can fold all errors into this type and return it from the host functions and remove Outcome
/// type.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum VMError {
    #[error("Revert: {code}")]
    Revert { code: u32 },
    #[error("Out of gas")]
    OutOfGas,
    #[error(transparent)]
    Export(#[from] ExportError),
    /// Error while executing Wasm: traps, memory access errors, etc.
    ///
    /// NOTE: for supporting multiple different backends we may want to abstract this a bit and
    /// extract memory access errors, trap codes, and unify error reporting.
    #[error("Trap: {0}")]
    Trap(TrapCode),
    #[error("Error resolving a function: {0}")]
    Resolver(Resolver),
    #[error("Memory error: {0}")]
    Memory(#[from] MemoryError),
}

pub type VMResult<T> = Result<T, VMError>;

#[derive(Clone, Debug)]
pub struct Config {
    pub(crate) gas_limit: u64,
    pub(crate) memory_limit: u32,
    pub(crate) input: Bytes,
}

#[derive(Clone, Debug, Default)]
pub struct ConfigBuilder {
    gas_limit: Option<u64>,
    /// Memory limit in pages.
    memory_limit: Option<u32>,
    /// Input data.
    input: Option<Bytes>,
}

impl ConfigBuilder {
    pub fn new() -> Self {
        Self::default()
    }

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

impl VM {
    pub fn prepare<S: Storage + 'static, C: Into<Bytes>>(
        &mut self,
        wasm_bytes: C,
        context: Context<S>,
        config: Config,
    ) -> Result<impl WasmInstance<S>, BackendError> {
        let wasm_bytes: Bytes = wasm_bytes.into();
        let instance = WasmerInstance::from_wasm_bytes(wasm_bytes, context, config)?;

        Ok(instance)
    }

    pub fn new() -> Self {
        VM
    }
}

impl Default for VM {
    fn default() -> Self {
        Self::new()
    }
}
