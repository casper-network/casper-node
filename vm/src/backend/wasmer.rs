use std::{
    ptr::NonNull,
    sync::{Arc, Mutex, Weak}, mem,
};

use wasmer::{
    AsStoreMut, AsStoreRef, Engine, Exports, Function, FunctionEnv, FunctionEnvMut, Imports,
    Instance, Memory, MemoryView, Module, RuntimeError, Store,
};
use wasmer_compiler_singlepass::Singlepass;

use crate::{host, storage::Storage};

use super::{Caller, Context, Error as BackendError, GasSummary, WasmInstance};
use crate::Error as VMError;

pub(crate) struct WasmerModule {
    module: Module,
}

pub(crate) struct WasmerEngine {}

struct WasmerEnv<S: Storage> {
    context: Context<S>,
    memory: Option<Memory>,
    instance: Weak<Instance>,
}

unsafe impl<S: Storage> Send for WasmerEnv<S> {}
unsafe impl<S: Storage> Sync for WasmerEnv<S> {}


fn call_alloc<E: AsStoreMut>(instance: &Instance, mut env: E, size: usize) -> u32 {
     // let tref = self.env.data().instance.as_ref();
     let func = instance
     .exports
     .get_typed_function::<u32, u32>(&env, "alloc")
     .expect("should have function");
 let ret = func.call(&mut env, size.try_into().unwrap()).unwrap();
 ret
}

pub(crate) struct WasmerCaller<'a, S: Storage> {
    env: FunctionEnvMut<'a, WasmerEnv<S>>,
}

impl<'a, S: Storage + 'static> WasmerCaller<'a, S> {
    fn with_memory<T>(&self, f: impl FnOnce(MemoryView<'_>) -> T) -> T {
        let mem = self.env.data().memory.as_ref().expect("should have memory");
        let binding = self.env.as_store_ref();
        let view = mem.view(&binding);
        f(view)
    }
}

impl<'a, S: Storage + 'static> Caller<S> for WasmerCaller<'a, S> {
    // fn memory_read(&self, offset: u32, size: usize) -> Result<Vec<u8>, BackendError> {
    //     Ok(v)
    // }

    fn memory_write(&self, offset: u32, data: &[u8]) -> Result<(), BackendError> {
        self.with_memory(|mem| mem.write(offset.into(), data))
            .expect("should write");
        Ok(())
    }

    fn context(&self) -> &Context<S> {
        &self.env.data().context
    }

    fn memory_read_into(&self, offset: u32, output: &mut [u8]) -> Result<(), BackendError> {
        self.with_memory(|mem| mem.read(offset.into(), output))
            .expect("should read");
        Ok(())
    }

    fn alloc(&mut self, size: usize) -> u32 {
        // let tref = self.env.data().instance.as_ref();
        let tref = self
            .env
            .data()
            .instance
            .upgrade()
            .expect("instance should be alive");
        call_alloc(&tref, &mut self.env, size)
    }
}

impl<S: Storage> WasmerEnv<S> {}

impl<S: Storage> WasmerEnv<S> {
    fn new(context: Context<S>) -> Self {
        Self {
            context,
            memory: None,
            instance: Weak::new(),
        }
    }
}

pub(crate) struct WasmerInstance<S: Storage> {
    instance: Arc<Instance>,
    env: FunctionEnv<WasmerEnv<S>>,
    store: Store,
}

impl<S> WasmerInstance<S>
where
    S: Storage + 'static,
{
    pub(crate) fn from_wasm_bytes(
        wasm_bytes: &[u8],
        context: Context<S>,
    ) -> Result<Self, BackendError> {
        // let mut store = Engine
        let engine = {
            let singlepass_compiler = Singlepass::new();
            // singlepass_compiler.push_middleware(middleware)
            singlepass_compiler
        };

        let engine = Engine::from(engine);

        let module = Module::new(&engine, wasm_bytes)
            .map_err(|error| BackendError::CompileError(error.to_string()))?;

        let mut store = Store::new(engine);

        let wasmer_env = WasmerEnv::new(context);
        let function_env = FunctionEnv::new(&mut store, wasmer_env);

        let imports = {
            let mut imports = Imports::new();
            imports.define(
                "env",
                "casper_write",
                Function::new_typed_with_env(
                    &mut store,
                    &function_env,
                    |env: FunctionEnvMut<WasmerEnv<S>>,
                     key_space: u64,
                     key_ptr: u32,
                     key_size: u32,
                     value_tag: u64,
                     value_ptr: u32,
                     value_size: u32|
                     -> i32 {
                        let wasmer_caller = WasmerCaller { env };
                        host::casper_write(
                            wasmer_caller,
                            key_space,
                            key_ptr,
                            key_size,
                            value_tag,
                            value_ptr,
                            value_size,
                        )
                    },
                ),
            );

            imports.define(
                "env",
                "casper_read",
                Function::new_typed_with_env(
                    &mut store,
                    &function_env,
                    |env: FunctionEnvMut<WasmerEnv<S>>,
                     key_space: u64,
                     key_ptr: u32,
                     key_size: u32,
                     info_ptr: u32|
                     -> i32 {
                        let wasmer_caller = WasmerCaller { env };
                        host::casper_read(wasmer_caller, key_space, key_ptr, key_size, info_ptr)
                    },
                ),
            );
            imports.define(
                "env",
                "casper_print",
                Function::new_typed_with_env(
                    &mut store,
                    &function_env,
                    |env: FunctionEnvMut<WasmerEnv<S>>,
                     message_ptr: u32,
                     message_size: u32|
                     -> i32 {
                        let wasmer_caller = WasmerCaller { env };
                        host::casper_print(wasmer_caller, message_ptr, message_size)
                    },
                ),
            );

            imports.define(
                "env",
                "casper_revert",
                // Function::new_typed_with_env(
                //     &mut store,
                //     &function_env,
                //     |_env: FunctionEnvMut<WasmerEnv<S>>,
                //         code: u32|
                //      -> i32 {
                //         host::casper_revert( code)
                //     },
                // ),
                Function::new_typed(&mut store, |code| -> Result<(), RuntimeError> {
                    eprintln!("casper_revert({code})");
                    Err(RuntimeError::user(Box::new(VMError::Revert { code })))
                    // host::casper_revert(code)
                }),
            );

            imports
        };

        let exports = Exports::new();

        let instance =
            Arc::new(Instance::new(&mut store, &module, &imports).expect("should instantiate"));

        let memory = instance
            .exports
            .get_memory("memory")
            .expect("should have memory");

        {
            let function_env_mut = function_env.as_mut(&mut store);
            function_env_mut.memory = Some(memory.clone());
            function_env_mut.instance = Arc::downgrade(&instance);
        }

        Ok(Self {
            instance,
            env: function_env,
            store,
        })
    }
}

impl<S> WasmInstance<S> for WasmerInstance<S>
where
    S: Storage + 'static,
{
    fn call_export(&mut self, name: &str, args: &[&[u8]]) -> (Result<(), VMError>, GasSummary) {
        // Inject arguments
        // alloc(sum(args.size) + len(args) * sizeof(u32))
        // [offset1, offset2, offset3]

        let mut data_size: usize = args.iter().map(|arg| arg.len()).sum();
        let mut offset_size: usize = args.len();

        // let mut arg_ptrs = Vec::new();
        let mut args_ptr_chunk = Vec::new();

        let args_ptr = call_alloc(&self.instance, &mut self.store, args.len());
        let memory = self.instance.exports.get_memory("memory").expect("should have memory").clone();
        // let view = memory.view(&self.store);

        #[repr(C)]
        struct Slice {
            ptr: u32,
            size: u32,
        }


        for arg in args {
            let arg_ptr = call_alloc(&self.instance, &mut self.store.as_store_mut(), arg.len());
            memory.view(&self.store).write(arg_ptr.into(), arg).unwrap();

            let slice = Slice {
                ptr: arg_ptr,
                size: arg.len() as _,
            };

            let slice_bytes: [u8; mem::size_of::<Slice>()] = unsafe { mem::transmute_copy(&slice) };

            args_ptr_chunk.extend_from_slice(&slice_bytes);
        }

        let offsets_ptr = call_alloc(&self.instance, &mut self.store, args_ptr_chunk.len());
        // let args_ptr = call_alloc(&self.instance, &mut self.store, args.len());
        memory.view(&self.store).write(offsets_ptr.into(), &args_ptr_chunk).unwrap();



        let typed_function = self
            .instance
            .exports
            .get_typed_function::<(u64, u32), ()>(&self.store, name)
            .expect("export error");
        let vm_result = match typed_function.call(&mut self.store, args.len().try_into().unwrap(), offsets_ptr) {
            Ok(()) => Ok(()),
            Err(error) => match error.downcast::<VMError>() {
                Ok(vm_error) => Err(vm_error),
                Err(vm_error) => todo!("handle {vm_error:?}"),
            },
        };
        let gas_summary = GasSummary {};
        (vm_result, gas_summary)
    }

    // pub fn run_export<S: Storage + 'static>(
    //     &mut self,
    //     mut instance: impl WasmInstance<S>,
    //     name: &str,
    // ) -> (Result<(), Error>, GasSummary) {
    //     let gas_summary = GasSummary {};

    //     let result = instance.call_export0(name);
    //     dbg!(&result);

    //     (result, gas_summary)
    // }

    /// Consume instance object and retrieve the [`Context`] object.
    fn teardown(self) -> Context<S> {
        let WasmerInstance { env, store, .. } = self;
        let a = env.as_ref(&store);
        Context {
            storage: a.context.storage.clone(),
        }
    }

}
