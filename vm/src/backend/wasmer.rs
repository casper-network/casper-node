mod metering_middleware;

use std::sync::{Arc, Weak};

use bytes::Bytes;
use wasmer::{
    AsStoreMut, AsStoreRef, CompilerConfig, Engine, Exports, Function, FunctionEnv, FunctionEnvMut,
    Imports, Instance, Memory, MemoryView, Module, RuntimeError, Store, Table, TypedFunction,
};
use wasmer_compiler_singlepass::Singlepass;
use wasmer_middlewares::metering::{self, MeteringPoints};
use wasmer_types::TrapCode;

use crate::{
    host::{self, Outcome},
    storage::Storage,
    Config, ExportError, HostError,
};

use self::metering_middleware::make_wasmer_metering_middleware;

use super::{Caller, Context, Error as BackendError, GasUsage, WasmInstance};
use crate::Error as VMError;

// pub(crate) struct WasmerModule {
//     module: Module,
// }

pub(crate) struct WasmerEngine {}

struct WasmerEnv<S: Storage> {
    config: Config,
    context: Context<S>,
    instance: Weak<Instance>,
    bytecode: Bytes,
    exported_runtime: Option<ExportedRuntime>,
}

pub(crate) struct WasmerCaller<'a, S: Storage> {
    env: FunctionEnvMut<'a, WasmerEnv<S>>,
}

impl<'a, S: Storage + 'static> WasmerCaller<'a, S> {
    fn with_memory<T>(&self, f: impl FnOnce(MemoryView<'_>) -> T) -> T {
        let mem = &self.env.data().exported_runtime().memory;
        let binding = self.env.as_store_ref();
        let view = mem.view(&binding);
        f(view)
    }
}

/// Handles translation of [`RuntimeError`] into [`VMError`].
///
/// This does not handle gas metering points.
fn handle_runtime_error(
    error: RuntimeError,
    mut store: impl AsStoreMut,
    instance: &Instance,
) -> VMError {
    match error.downcast::<HostError>() {
        Ok(host_error) => VMError::Host(host_error),
        Err(runtime_error) => {
            if runtime_error.clone().to_trap() == Some(TrapCode::UnreachableCodeReached) {
                match dbg!(metering::get_remaining_points(&mut store, instance)) {
                    MeteringPoints::Remaining(_) => {}
                    MeteringPoints::Exhausted => return VMError::OutOfGas,
                }
            }
            VMError::Runtime {
                message: runtime_error.message(),
            }
        }
    }
}

impl<'a, S: Storage + 'static> Caller<S> for WasmerCaller<'a, S> {
    fn memory_write(&self, offset: u32, data: &[u8]) -> Result<(), VMError> {
        self.with_memory(|mem| mem.write(offset.into(), data))
            .map_err(|memory_access_error| VMError::Runtime {
                message: memory_access_error.to_string(),
            })
    }

    fn context(&self) -> &Context<S> {
        &self.env.data().context
    }

    fn memory_read_into(&self, offset: u32, output: &mut [u8]) -> Result<(), VMError> {
        self.with_memory(|mem| mem.read(offset.into(), output))
            .map_err(|memory_access_error| VMError::Runtime {
                message: memory_access_error.to_string(),
            })
    }

    fn alloc(
        &mut self,
        idx: u32,
        size: usize,
        ctx: u32,
    ) -> Result<u32, Box<dyn std::error::Error>> {
        let (data, mut store) = self.env.data_and_store_mut();
        let value = data
            .exported_runtime()
            .exported_table
            .as_ref()
            .expect("should have table exported") // TODO: if theres no table then no function pointer is stored in the wasm blob -
            // probably safe
            .get(&mut store.as_store_mut(), idx)
            .expect("has entry in the table"); // TODO: better error handling - pass 0 as nullptr?
        let funcref = value.funcref().expect("is funcref");
        let valid_funcref = funcref.as_ref().expect("valid funcref");
        let alloc_callback: TypedFunction<(u32, u32), u32> = valid_funcref.typed(&store)?;
        let ptr = alloc_callback.call(&mut store.as_store_mut(), size.try_into().unwrap(), ctx)?;
        Ok(ptr)
    }

    fn config(&self) -> &Config {
        &self.env.data().config
    }

    fn bytecode(&self) -> Bytes {
        self.env.data().bytecode.clone()
    }

    fn retrieve_manifest() {
        todo!()
    }
}

impl<S: Storage> WasmerEnv<S> {}

impl<S: Storage> WasmerEnv<S> {
    fn new(config: Config, context: Context<S>, code: Bytes) -> Self {
        Self {
            config,
            context,
            // memory: None,
            instance: Weak::new(),
            exported_runtime: None,
            bytecode: code,
        }
    }
    pub(crate) fn exported_runtime(&self) -> &ExportedRuntime {
        self.exported_runtime
            .as_ref()
            .expect("Valid instance of exported runtime")
    }
}

/// Container for Wasm-provided exports such as alloc, dealloc, etc.
///
/// Let's call it a "minimal runtime" that is expected to exist inside a Wasm.
#[derive(Clone)]
pub(crate) struct ExportedRuntime {
    pub(crate) memory: Memory,
    pub(crate) exported_table: Option<Table>,
    pub(crate) exports: Exports,
}

pub(crate) struct WasmerInstance<S: Storage> {
    instance: Arc<Instance>,
    env: FunctionEnv<WasmerEnv<S>>,
    store: Store,
    config: Config,
}

impl<S> WasmerInstance<S>
where
    S: Storage + 'static,
{
    fn wasmer_env(&self) -> &WasmerEnv<S> {
        self.env.as_ref(&self.store)
    }

    pub(crate) fn call_export(&mut self, name: &str) -> Result<(), VMError> {
        let exported_call_func: TypedFunction<(), ()> = self
            .instance
            .exports
            .get_typed_function(&self.store, name)
            .map_err(|export_error| match export_error {
                wasmer::ExportError::IncompatibleType => ExportError::IncompatibleType,
                wasmer::ExportError::Missing(name) => ExportError::Missing(name),
            })?;
        // .map_err(|error| BackendError::Export(error.to_string()))?;

        let result = exported_call_func.call(&mut self.store.as_store_mut());

        let remaining_points_1 = metering::get_remaining_points(&mut self.store, &self.instance);
        dbg!(&remaining_points_1);

        match remaining_points_1 {
            metering::MeteringPoints::Remaining(_remaining_points) => {}
            metering::MeteringPoints::Exhausted => {
                return Err(VMError::OutOfGas);
            }
        }

        match result {
            Ok(result) => Ok(()),
            Err(error) => match error.downcast::<HostError>() {
                Ok(host_error) => Err(VMError::Host(host_error)),
                Err(vm_error) => Err(VMError::Runtime {
                    message: vm_error.to_string(),
                }),
            },
        }
    }

    pub(crate) fn from_wasm_bytes<C: Into<Bytes>>(
        wasm_bytes: C,
        context: Context<S>,
        config: Config,
    ) -> Result<Self, BackendError> {
        // let mut store = Engine
        let engine = {
            let mut singlepass_compiler = Singlepass::new();
            let metering = make_wasmer_metering_middleware(config.gas_limit);
            singlepass_compiler.push_middleware(metering);
            // singlepass_compiler.push_middleware(middleware)
            singlepass_compiler
        };

        let engine = Engine::from(engine);

        let wasm_bytes: Bytes = wasm_bytes.into();

        let module = Module::new(&engine, &wasm_bytes)
            .map_err(|error| BackendError::Compile(error.to_string()))?;

        let mut store = Store::new(engine);

        let wasmer_env = WasmerEnv::new(config.clone(), context, wasm_bytes);
        let function_env = FunctionEnv::new(&mut store, wasmer_env);

        let memory = Memory::new(
            &mut store,
            wasmer_types::MemoryType {
                minimum: wasmer_types::Pages(17),
                maximum: Some(wasmer_types::Pages(17 * 4)),
                shared: false,
            },
        )
        .map_err(|error| BackendError::Memory(error.to_string()))?;

        let imports = {
            let mut imports = Imports::new();
            imports.define("env", "memory", memory.clone());

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
                     info_ptr: u32,
                     cb_alloc: u32,
                     cb_ctx: u32|
                     -> Result<i32, RuntimeError> {
                        let wasmer_caller = WasmerCaller { env };
                        match host::casper_read(
                            wasmer_caller,
                            key_space,
                            key_ptr,
                            key_size,
                            info_ptr,
                            cb_alloc,
                            cb_ctx,
                        ) {
                            Ok(result) => Ok(result),
                            Err(Outcome::VM(type_erased_runtime_error)) => {
                                dbg!(&type_erased_runtime_error);
                                let boxed_runtime_error: Box<RuntimeError> =
                                    type_erased_runtime_error
                                        .downcast()
                                        .expect("Valid RuntimeError instance");
                                Err(*boxed_runtime_error)
                            }
                            Err(Outcome::Host(host_error)) => {
                                Err(RuntimeError::user(Box::new(host_error)))
                            }
                        }
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
                Function::new_typed(&mut store, |code| -> Result<(), RuntimeError> {
                    Err(RuntimeError::user(Box::new(HostError::Revert { code })))
                }),
            );

            imports.define(
                "env",
                "casper_copy_input",
                Function::new_typed_with_env(
                    &mut store,
                    &function_env,
                    |env: FunctionEnvMut<WasmerEnv<S>>,
                     cb_alloc: u32,
                     cb_ctx: u32|
                     -> Result<u32, RuntimeError> {
                        let wasmer_caller = WasmerCaller { env };
                        let ret = host::casper_copy_input(wasmer_caller, cb_alloc, cb_ctx);
                        handle_host_function_result(ret)
                    },
                ),
            );

            imports.define(
                "env",
                "casper_create_contract",
                Function::new_typed_with_env(
                    &mut store,
                    &function_env,
                    |env: FunctionEnvMut<WasmerEnv<S>>,
                     // casper_create_contract(code_ptr: *const u8, code_size: usize,
                     // manifest_ptr: *mut Manifest, result_ptr: *mut CreateResult) -> i32;
                     code_ptr: u32,
                     code_size: u32,
                     manifest_ptr: u32,
                     result_ptr: u32| {
                        let wasmer_caller = WasmerCaller { env };
                        let ret = host::casper_create_contract(
                            wasmer_caller,
                            code_ptr,
                            code_size,
                            manifest_ptr,
                            result_ptr,
                        );
                        handle_host_function_result(ret)
                    },
                ),
            );

            imports.define(
                "env",
                "casper_call",
                Function::new_typed_with_env(
                    &mut store,
                    &function_env,
                    |env: FunctionEnvMut<WasmerEnv<S>>,
                     address_ptr: u32,
                     address_len: u32,
                     value: u64,
                     entry_name: u32,
                     entry_len: u32,
                     input_ptr: u32,
                     input_len: u32,
                     cb_alloc: u32,
                     cb_ctx: u32|
                     -> Result<u32, RuntimeError> {
                        let wasmer_caller = WasmerCaller { env };
                        let ret = host::casper_call(
                            wasmer_caller,
                            address_ptr,
                            address_len,
                            value,
                            entry_name,
                            entry_len,
                            input_ptr,
                            input_len,
                            cb_alloc,
                            cb_ctx,
                        );
                        match ret {
                            Ok(result) => Ok(result),
                            Err(Outcome::VM(type_erased_runtime_error)) => {
                                let boxed_runtime_error: Box<RuntimeError> =
                                    type_erased_runtime_error
                                        .downcast()
                                        .expect("Valid RuntimeError instance");
                                Err(*boxed_runtime_error)
                            }
                            Err(Outcome::Host(host_error)) => {
                                Err(RuntimeError::user(Box::new(host_error)))
                            }
                        }
                    },
                ),
            );

            imports
        };

        // TODO: Deal with "start" section that executes actual Wasm - test, measure gas, etc. ->
        // Instance::new may fail with RuntimError

        let instance = {
            let instance = Instance::new(&mut store, &module, &imports)
                .map_err(|error| BackendError::Instantiation(error.to_string()))?;

            // We don't necessarily need atomic counter. Arc's purpose is to be able to retrieve a
            // Weak reference to the instance to be able to invoke recursive calls to the wasm
            // itself from within a host function implementation.

            // instance.exports.get_table(name)
            Arc::new(instance)
        };

        // TODO: get first export of type table as some compilers generate different names (i.e.
        // rust __indirect_function_table, assemblyscript `table` etc). There's only one table
        // allowed in a valid module.
        let table = match instance.exports.get_table("__indirect_function_table") {
            Ok(table) => Some(table.clone()),
            Err(error @ wasmer::ExportError::IncompatibleType) => {
                return Err(BackendError::MissingExport(error.to_string()))
            }
            Err(wasmer::ExportError::Missing(_)) => None,
        };

        let exports = instance.exports.clone();
        // let memory = instance
        //     .exports
        //     .get_memory("memory")
        //     .map_err(|error| BackendError::Export(error.to_string()))?;

        {
            let function_env_mut = function_env.as_mut(&mut store);
            function_env_mut.instance = Arc::downgrade(&instance);
            function_env_mut.exported_runtime = Some(ExportedRuntime {
                memory,
                exported_table: table,
                exports,
            });
        }

        Ok(Self {
            instance,
            env: function_env,
            store,
            config,
        })
    }
}

fn handle_host_function_result<T>(ret: Result<T, Outcome>) -> Result<T, RuntimeError> {
    match ret {
        Ok(result) => Ok(result),
        Err(Outcome::VM(type_erased_runtime_error)) => {
            let boxed_runtime_error: Box<RuntimeError> = type_erased_runtime_error
                .downcast()
                .expect("Valid RuntimeError instance");
            Err(*boxed_runtime_error)
        }
        Err(Outcome::Host(host_error)) => Err(RuntimeError::user(Box::new(host_error))),
    }
}

impl<S> WasmInstance<S> for WasmerInstance<S>
where
    S: Storage + 'static,
{
    fn call_export(&mut self, name: &str) -> (Result<(), VMError>, GasUsage) {
        let vm_result = self.call_export(name);
        let remaining_points = metering::get_remaining_points(&mut self.store, &self.instance);
        match remaining_points {
            MeteringPoints::Remaining(remaining_points) => {
                let gas_usage = GasUsage {
                    gas_limit: self.config.gas_limit,
                    remaining_points,
                };
                (vm_result, gas_usage)
            }
            MeteringPoints::Exhausted => {
                let gas_usage = GasUsage {
                    gas_limit: self.config.gas_limit,
                    remaining_points: 0,
                };
                (Err(VMError::OutOfGas), gas_usage)
            }
        }
    }

    /// Consume instance object and retrieve the [`Context`] object.
    fn teardown(self) -> Context<S> {
        let WasmerInstance { env, store, .. } = self;
        let WasmerEnv { context, .. } = env.as_ref(&store);

        let Context { storage } = context;

        Context {
            storage: storage.clone(),
        }
    }
}
