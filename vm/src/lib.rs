pub mod backend;
pub(crate) mod host;
pub mod storage;

use bytes::Bytes;

use backend::{wasmer::WasmerInstance, Context, Error as BackendError, GasSummary, WasmInstance};
use storage::Storage;
use thiserror::Error;

struct Arguments {
    bytes: Bytes,
}

/// VM execute request specifies execution context, the wasm bytes, and other necessary information
/// to execute.
pub struct ExecuteRequest {
    /// Wasm module.
    pub wasm_bytes: Bytes,
}

pub struct VM;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum HostError {
    #[error("revert {code}")]
    Revert { code: u32 },
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
    #[error("{message}")]
    Export { message: String },
}

impl VM {
    pub fn prepare<S: Storage + 'static>(
        &mut self,
        execute_request: ExecuteRequest,
        context: Context<S>,
    ) -> Result<impl WasmInstance<S>, BackendError> {
        let ExecuteRequest { wasm_bytes } = execute_request;
        let instance = WasmerInstance::from_wasm_bytes(&wasm_bytes, context)?;
        Ok(instance)
    }

    pub fn new() -> Self {
        VM
    }
}
