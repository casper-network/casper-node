pub mod backend;
pub(crate) mod host;
pub mod storage;

use bytes::Bytes;

use backend::{wasmer::WasmerInstance, Context, Error as BackendError, GasUsage, WasmInstance};
use storage::Storage;
use thiserror::Error;

struct Arguments {
    bytes: Bytes,
}



#[derive(Clone)]
pub struct VM;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum HostError {
    #[error("revert {code}")]
    Revert { code: u32 },
}

#[derive(Debug, Error)]
pub enum Resolver {
    #[error("export {name} not found.")]
    Export { name: String },
    /// Trying to call a function pointer by index.
    #[error("function pointer {index} not found.")]
    Table { index: u32 },
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("Host error: {0}")]
    Host(#[source] HostError),
    #[error("Out of gas")]
    OutOfGas,
    /// Error while executing Wasm: traps, memory access errors, etc.
    ///
    /// NOTE: for supporting multiple different backends we may want to abstract this a bit and
    /// extract memory access errors, trap codes, and unify error reporting.
    #[error("Error executing Wasm: {message}")]
    Runtime { message: String },
    #[error("Error resolving a function: {0}")]
    Resolver(Resolver),
}

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
    pub fn prepare<S: Storage + 'static>(
        &mut self,
        wasm_bytes: &[u8],
        context: Context<S>,
        config: Config,
    ) -> Result<impl WasmInstance<S>, BackendError> {
        let instance = WasmerInstance::from_wasm_bytes(wasm_bytes, context, config)?;
        Ok(instance)
    }

    pub fn new() -> Self {
        VM
    }
}
