use crate::{WasmerCaller, WasmerEnv};
use casper_executor_wasm_interface::{executor::Executor, VMResult};
use casper_storage::global_state::GlobalStateReader;
use tracing::warn;
use wasmer::{FunctionEnv, FunctionEnvMut, Imports, Store};

const DEFAULT_ENV_NAME: &str = "env";

/// This function will populate imports object with all host functions that are defined.
pub(crate) fn generate_casper_imports<S: GlobalStateReader + 'static, E: Executor + 'static>(
    store: &mut Store,
    env: &FunctionEnv<WasmerEnv<S, E>>,
) -> Imports {
    let mut imports = Imports::new();
    imports.define(
        DEFAULT_ENV_NAME,
        "casper_ffi",
        wasmer::Function::new_typed_with_env(
            store,
            env,
            |env: FunctionEnvMut<WasmerEnv<S, E>>,
             ffi_opt: u32,
             input_ptr: u32,
             input_size: u32,
             alloc: u32,
             alloc_ctx: u32|
             -> VMResult<u32> {
                let wasmer_caller = WasmerCaller { env };

                // Dispatch to the actual host function. This also ensures that the return type of
                // host function impl has expected type.
                let result: VMResult<u32> = casper_executor_wasm_host::host::casper_ffi(
                    wasmer_caller,
                    ffi_opt,
                    input_ptr,
                    input_size,
                    alloc,
                    alloc_ctx,
                );

                match result {
                    Ok(ret) => Ok(ret),
                    Err(error) => {
                        warn!(
                            "Host function {} failed with error: {error:?}",
                            stringify!($name),
                        );

                        Err(error)
                    }
                }
            },
        ),
    );

    imports
}
