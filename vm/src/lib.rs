mod backend;
pub mod storage;

use bytes::Bytes;
use casper_types::account::AccountHash;

use backend::{Error as BackendError, GasSummary, wasmer::WasmerInstance};

struct Arguments {
    bytes: Bytes,
}

/// VM execute request specifies execution context, the wasm bytes, and other necessary information
/// to execute.
pub struct ExecuteRequest {
    /// Wasm module.
    wasm_bytes: Bytes,
}

pub trait Storage {}

pub struct VM;

enum VMError {
    Revert,
}

impl VM {
    pub(crate) fn prepare(
        &mut self,
        execute_request: ExecuteRequest,
    ) -> Result<WasmerInstance, BackendError> {
    // ) -> (Result<WasmerInstance, BackendError>, GasSummary) {
        let ExecuteRequest { wasm_bytes } = execute_request;
        let instance = WasmerInstance::from_wasm_bytes(&wasm_bytes)?;
        Ok(instance)
    }

    fn new() -> Self {
        VM
    }
}

#[cfg(test)]
mod tests {
    use crate::backend::wasmer::WasmerInstance;

    use super::*;
    const TEST_CONTRACT_WASM: &[u8] = include_bytes!("../test-contract.wasm");

    #[test]
    fn smoke() {
        let mut vm = VM::new();
        let execute_request = ExecuteRequest {
            wasm_bytes: Bytes::from_static(TEST_CONTRACT_WASM),
        };

        // let wasmer_instance = WasmerInstance::new
        let mut instance = vm.prepare(execute_request).expect("should prepare");
        instance.call_export0("call").expect("should call");
        // dbg!(&res);
    }
}
