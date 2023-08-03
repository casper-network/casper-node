pub mod backend;
pub(crate) mod host;
pub mod storage;

use bytes::Bytes;

use backend::{wasmer::WasmerInstance, Context, Error as BackendError, WasmInstance};
use storage::Storage;

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

enum VMError {
    Revert,
}

impl VM {
    pub fn prepare<S: Storage + 'static>(
        &mut self,
        execute_request: ExecuteRequest,
        context: Context<S>,
    ) -> Result<impl WasmInstance<S>, BackendError> {
        // ) -> (Result<WasmerInstance, BackendError>, GasSummary) {
        let ExecuteRequest { wasm_bytes } = execute_request;
        let instance = WasmerInstance::from_wasm_bytes(&wasm_bytes, context)?;
        Ok(instance)
    }

    pub fn new() -> Self {
        VM
    }
}
