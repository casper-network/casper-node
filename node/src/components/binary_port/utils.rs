use casper_binary_port::{
    SandboxedExecutionError, SandboxedExecutionRequest, SandboxedExecutionResult,
};
use casper_executor_wasm_interface::{
    SandboxedExecutionError as InnerSandboxedExecutionError,
    SandboxedExecutionRequest as InnerSandboxedExecutionRequest,
    SandboxedExecutionResult as InnerSandboxedExecutionResult,
};

/// Transforms binary port request into corresponding inner sandboxed execution request.
pub(super) fn map_sandbox_request(
    req: SandboxedExecutionRequest,
) -> InnerSandboxedExecutionRequest {
    InnerSandboxedExecutionRequest {
        initiator: req.initiator,
        contract_address: req.contract_address,
        entry_point: req.entry_point,
        input: req.input,
        gas_limit: req.gas_limit,
        block_time: req.block_time,
        state_hash: req.state_hash,
        parent_block_hash: req.parent_block_hash,
        block_height: req.block_height,
        chain_name: req.chain_name,
    }
}

/// Transforms inner sandboxed execution error into corresponding binary port error.
pub(super) fn map_sandbox_error(
    maybe_error: Option<InnerSandboxedExecutionError>,
) -> Option<SandboxedExecutionError> {
    match maybe_error {
        Some(error) => {
            let ret = match error {
                InnerSandboxedExecutionError::CalleeRolledBack => {
                    SandboxedExecutionError::CalleeRolledBack
                }
                InnerSandboxedExecutionError::CalleeTrapped => {
                    SandboxedExecutionError::CalleeTrapped
                }
                InnerSandboxedExecutionError::CalleeGasDepleted => {
                    SandboxedExecutionError::CalleeGasDepleted
                }
                InnerSandboxedExecutionError::NotCallable => SandboxedExecutionError::NotCallable,
                InnerSandboxedExecutionError::CodeNotFound => SandboxedExecutionError::CodeNotFound,
                InnerSandboxedExecutionError::InternalHostError => {
                    SandboxedExecutionError::InternalHostError
                }
                InnerSandboxedExecutionError::NoActiveContract => {
                    SandboxedExecutionError::NoActiveContract
                }
                InnerSandboxedExecutionError::EntityNotFound => {
                    SandboxedExecutionError::EntityNotFound
                }
                InnerSandboxedExecutionError::LockedPackage => {
                    SandboxedExecutionError::LockedPackage
                }
                InnerSandboxedExecutionError::Api(api_error) => {
                    SandboxedExecutionError::Api(api_error)
                }
                InnerSandboxedExecutionError::InputInvalid => SandboxedExecutionError::InputInvalid,
            };
            Some(ret)
        }
        None => None,
    }
}

/// Transforms inner sandboxed execution result into corresponding binary port result.
pub(super) fn map_sandbox_result(
    result: InnerSandboxedExecutionResult,
) -> SandboxedExecutionResult {
    let error = map_sandbox_error(result.error);
    let output = result.output;
    let gas_usage = result.gas_usage;
    SandboxedExecutionResult {
        error,
        output,
        gas_usage,
    }
}
