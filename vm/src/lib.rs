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
pub enum Error {
    #[error("Revert: {code}")]
    Revert { code: u32 },
    #[error("Error executing Wasm: {message}")]
    Runtime { message: String },
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
