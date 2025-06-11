use casper_executor_wasm_interface::{executor::Executor, VMError, VMResult};
use casper_storage::global_state::GlobalStateReader;
use tracing::warn;
use wasmer::{FunctionEnv, FunctionEnvMut, Imports, Store};

use casper_contract_sdk_sys::for_each_host_function;

use crate::WasmerEnv;

/// A trait for converting a C ABI type declaration to a type that is understandable by wasm32
/// target (and wasmer, by a consequence).
#[allow(dead_code)]
pub(crate) trait WasmerConvert: Sized {
    type Output;
}

impl WasmerConvert for i32 {
    type Output = i32;
}

impl WasmerConvert for u32 {
    type Output = u32;
}
impl WasmerConvert for u64 {
    type Output = u64;
}

impl WasmerConvert for usize {
    type Output = u32;
}

impl<T> WasmerConvert for *const T {
    type Output = u32; // Pointers are 32-bit addressable
}

impl<T> WasmerConvert for *mut T {
    type Output = u32; // Pointers are 32-bit addressable
}

impl<Arg1: WasmerConvert, Arg2: WasmerConvert, Ret: WasmerConvert> WasmerConvert
    for extern "C" fn(Arg1, Arg2) -> Ret
{
    type Output = u32; // Function pointers are 32-bit addressable
}

const DEFAULT_ENV_NAME: &str = "env";

/// This function will populate imports object with all host functions that are defined.
pub(crate) fn generate_casper_imports<S: GlobalStateReader + 'static, E: Executor + 'static>(
    store: &mut Store,
    env: &FunctionEnv<WasmerEnv<S, E>>,
) -> Imports {
    let mut imports = Imports::new();

    macro_rules! visit_host_function {
        (@convert_ret $ret:ty) => {
            <$ret as $crate::imports::WasmerConvert>::Output
        };
        (@convert_ret) => { () };
        ( $( $(#[$cfg:meta])? $vis:vis fn $name:ident $(( $($arg:ident: $argty:ty,)* ))? $(-> $ret:ty)?;)+) => {
            $(
                imports.define($crate::imports::DEFAULT_ENV_NAME, stringify!($name), wasmer::Function::new_typed_with_env(
                    store,
                    env,
                    |
                        env: FunctionEnvMut<WasmerEnv<S, E>>,
                        // List all types and statically mapped C types into wasm types
                        $($($arg: <$argty as $crate::imports::WasmerConvert>::Output,)*)?
                    | -> VMResult<visit_host_function!(@convert_ret $($ret)?)> {
                        let wasmer_caller = $crate::WasmerCaller { env };

                        // Dispatch to the actual host function. This also ensures that the return type of host function impl has expected type.
                        let result: VMResult< visit_host_function!(@convert_ret $($ret)?) > = casper_executor_wasm_host::host::$name(wasmer_caller, $($($arg,)*)?);

                        match result {
                            Ok(ret) => Ok(ret),
                            Err(error) => {
                                warn!(
                                    "Host function {} failed with error: {error:?}",
                                    stringify!($name),
                                );

                                if let VMError::Internal(internal) = error {
                                    panic!("InternalHostError {internal:?}; aborting");
                                }

                                Err(error)
                            }
                        }
                    }
                ));
            )*
        }
    }
    for_each_host_function!(visit_host_function);

    imports
}
