use wasmer::{
    CompilerConfig, Engine, Exports, Function, FunctionEnv, FunctionEnvMut, Imports, Instance,
    Module, Store,
};
use wasmer_compiler_singlepass::Singlepass;

use crate::storage::Storage;

use super::{Error as BackendError, Environment};

pub(crate) struct WasmerModule {
    module: Module,
}

pub(crate) struct WasmerEngine {}

struct WasmerEnv<S:Storage>(Environment<S>);

impl<S:Storage> WasmerEnv<S> {
    fn new(env: Environment<S>) -> Self {
        Self(env)
    }
}

pub(crate) struct WasmerInstance<S> {
    instance: Box<Instance>,
    env: FunctionEnv<WasmerEnv<S>>,
    store: Store,
}

impl<S:Storage> WasmerInstance<S> {
    pub fn from_wasm_bytes(wasm_bytes: &[u8], environment) -> Result<Self, BackendError> {
        // let mut store = Engine
        let engine = {
            let singlepass_compiler = Singlepass::new();
            // singlepass_compiler.push_middleware(middleware)
            singlepass_compiler
        };

        let engine = Engine::from(engine);

        let module = {
            let module = Module::new(&engine, wasm_bytes).map_err(|error| BackendError::CompileError(error.to_string()))?;
            module
        };

        let mut store = Store::new(engine);

        // let instance = Instance::

        let env = FunctionEnv::new(&mut store, WasmerEnv::new());

        let imports = {
            // let mut exports = Exports::new();
            // let store = Store::new(engine);

            //
            //pub fn casper_read(key_space: u64, key_ptr: *const u8, key_size: usize, size: *mut
            // usize, tag: *mut u64) -> *mut u8; pub fn casper_write(key_space: u64,
            // key_ptr: *const u8, key_size: usize, value_tag: u64, value_ptr: *const u8,
            // value_size: usize) -> i32;
            //

            // exports.insert(
            // let imports = Imports::new();
            let mut imports = Imports::new();
            imports.define("env", "casper_write",
                Function::new_typed_with_env(
                    &mut store,
                    &env,
                    |env: FunctionEnvMut<WasmerEnv>,
                     key_space: u64,
                     key_ptr: u32,
                     key_size: u32,
                     value_tag: u64,
                     value_ptr: u32,
                     value_size: u32,
                     | -> i32 {
                        // env

                    },
                ));


            imports
        };
        // imports.insert()

        let exports = Exports::new();

        let instance = Instance::new(&mut store, &module, &imports).expect("should instantiate");

        Ok(Self {
            instance: Box::new(instance),
            env,
            store,
        })
    }

    pub(crate) fn call_export0(&mut self, name: &str) -> Result<(), BackendError> {
        let typed_function = self.instance.exports.get_typed_function::<(), ()>(&mut self.store, name).expect("export error");
        typed_function.call(&mut self.store).expect("call error");
        Ok(())
    }

}
