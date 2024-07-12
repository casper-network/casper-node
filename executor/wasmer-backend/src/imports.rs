use casper_executor_wasm_interface::executor::Executor;
use casper_storage::global_state::GlobalStateReader;
use wasmer::{FunctionEnv, FunctionEnvMut, Imports, Store};

use casper_sdk_sys::for_each_host_function;

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

/// This function will populate imports object with all host functions that are defined.
#[allow(dead_code)]
pub(crate) fn populate_imports<S: GlobalStateReader + 'static, E: Executor + 'static>(
    imports: &mut Imports,
    env_name: &str,
    store: &mut Store,
    function_env: &FunctionEnv<WasmerEnv<S, E>>,
) {
    macro_rules! visit_host_function {
        ( $( $(#[$cfg:meta])? $vis:vis fn $name:ident $(( $($arg:ident: $argty:ty,)* ))? $(-> $ret:ty)?;)+) => {
            $(
                imports.define(env_name, stringify!($name), wasmer::Function::new_typed_with_env(
                    store,
                    &function_env,
                    |
                        env: FunctionEnvMut<WasmerEnv<S, E>>,
                        // List all types and statically mapped C types into wasm types
                        $($($arg: <$argty as $crate::imports::WasmerConvert>::Output,)*)?
                    | $(-> casper_executor_wasm_interface::VMResult<<$ret as $crate::imports::WasmerConvert>::Output>)? {
                        let wasmer_caller = $crate::WasmerCaller { env };
                        // Dispatch to the actual host function
                        let _res = casper_executor_wasm_host::host::$name(wasmer_caller, $($($arg,)*)?);

                        // TODO: Unify results in the `host` module and create a VMResult out of it
                        todo!()
                    }
                ));
            )*
        }
    }
    for_each_host_function!(visit_host_function);
}

#[cfg(test)]
mod tests {
    #[test]
    fn smoke_test() {}
}
