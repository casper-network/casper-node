mod metering_middleware;

use std::{
    fmt::Debug,
    sync::{Arc, Weak},
};

use bytes::Bytes;
use wasmer::{
    AsStoreMut, AsStoreRef, CompilerConfig, Engine, Exports, Function, FunctionEnv, FunctionEnvMut,
    Imports, Instance, Memory, MemoryView, Module, RuntimeError, Store, StoreMut, Table,
    TypedFunction,
};
use wasmer_compiler_singlepass::Singlepass;
use wasmer_middlewares::{metering, Metering};
use wasmer_types::compilation::function;

use crate::{host, storage::Storage, Config, ExportError, MemoryError, TrapCode, VMResult};

use self::metering_middleware::make_wasmer_metering_middleware;

use super::{Caller, Context, Error as BackendError, GasUsage, MeteringPoints, WasmInstance};
use crate::VMError;

impl From<wasmer::MemoryAccessError> for MemoryError {
    fn from(error: wasmer::MemoryAccessError) -> Self {
        match error {
            wasmer::MemoryAccessError::HeapOutOfBounds => MemoryError::HeapOutOfBounds,
            wasmer::MemoryAccessError::Overflow => MemoryError::Overflow,
            wasmer::MemoryAccessError::NonUtf8String => MemoryError::NonUtf8String,
            _ => todo!(),
        }
    }
}

impl From<wasmer_types::TrapCode> for TrapCode {
    fn from(value: wasmer_types::TrapCode) -> Self {
        match value {
            wasmer_types::TrapCode::StackOverflow => TrapCode::StackOverflow,
            wasmer_types::TrapCode::HeapAccessOutOfBounds => TrapCode::HeapAccessOutOfBounds,
            wasmer_types::TrapCode::HeapMisaligned => TrapCode::HeapMisaligned,
            wasmer_types::TrapCode::TableAccessOutOfBounds => TrapCode::TableAccessOutOfBounds,
            wasmer_types::TrapCode::IndirectCallToNull => TrapCode::IndirectCallToNull,
            wasmer_types::TrapCode::BadSignature => TrapCode::BadSignature,
            wasmer_types::TrapCode::IntegerOverflow => TrapCode::IntegerOverflow,
            wasmer_types::TrapCode::IntegerDivisionByZero => TrapCode::IntegerDivisionByZero,
            wasmer_types::TrapCode::BadConversionToInteger => TrapCode::BadConversionToInteger,
            wasmer_types::TrapCode::UnreachableCodeReached => TrapCode::UnreachableCodeReached,
            wasmer_types::TrapCode::UnalignedAtomic => {
                todo!("Atomic memory extension is not supported")
            }
        }
    }
}

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
    fn with_store_and_instance<Ret>(
        &mut self,
        f: impl FnOnce(StoreMut, Arc<Instance>) -> Ret,
    ) -> Ret {
        let (data, mut store) = self.env.data_and_store_mut();
        let instance = data.instance.upgrade().expect("Valid instance");
        f(store, instance)
    }

    /// Returns the amount of gas used.
    fn get_remaining_points(&mut self) -> MeteringPoints {
        self.with_store_and_instance(|mut store, instance| {
            let mut metering_points = metering::get_remaining_points(&mut store, &instance);
            match metering_points {
                metering::MeteringPoints::Remaining(points) => MeteringPoints::Remaining(points),
                metering::MeteringPoints::Exhausted => MeteringPoints::Exhausted,
            }
        })
    }
    /// Set the amount of gas used.
    fn set_remaining_points(&mut self, new_value: u64) {
        self.with_store_and_instance(|mut store, instance| {
            metering::set_remaining_points(&mut store, &instance, new_value)
        })
    }
}

impl<'a, S: Storage + 'static> Caller<S> for WasmerCaller<'a, S> {
    fn memory_write(&self, offset: u32, data: &[u8]) -> Result<(), VMError> {
        Ok(self
            .with_memory(|mem| mem.write(offset.into(), data))
            .map_err(|memory_error| VMError::Memory(memory_error.into()))?)
    }

    fn context(&self) -> &Context<S> {
        &self.env.data().context
    }

    fn memory_read_into(&self, offset: u32, output: &mut [u8]) -> Result<(), VMError> {
        Ok(self
            .with_memory(|mem| mem.read(offset.into(), output))
            .map_err(|memory_error| VMError::Memory(memory_error.into()))?)
    }

    fn alloc(&mut self, idx: u32, size: usize, ctx: u32) -> VMResult<u32> {
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
        let alloc_callback: TypedFunction<(u32, u32), u32> = valid_funcref
            .typed(&store)
            .unwrap_or_else(|error| panic!("{error:?}"));
        let ptr =
            match alloc_callback.call(&mut store.as_store_mut(), size.try_into().unwrap(), ctx) {
                Ok(ptr) => ptr,
                Err(runtime_error) => todo!("{:?}", runtime_error),
            };
        Ok(ptr)
    }

    fn config(&self) -> &Config {
        &self.env.data().config
    }

    fn bytecode(&self) -> Bytes {
        self.env.data().bytecode.clone()
    }

    /// Returns the amount of gas used.
    fn gas_consumed(&mut self) -> MeteringPoints {
        self.get_remaining_points()
    }

    /// Set the amount of gas used.
    fn consume_gas(&mut self, new_value: u64) -> MeteringPoints {
        let gas_consumed = self.gas_consumed();
        match gas_consumed {
            MeteringPoints::Remaining(remaining_points) if remaining_points >= new_value => {
                let remaining_points = remaining_points - new_value;
                self.set_remaining_points(remaining_points);
                MeteringPoints::Remaining(remaining_points)
            }
            MeteringPoints::Remaining(_remaining_points) => MeteringPoints::Exhausted,
            MeteringPoints::Exhausted => MeteringPoints::Exhausted,
        }
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

fn handle_wasmer_result<T: Debug>(wasmer_result: Result<T, RuntimeError>) -> VMResult<T> {
    match wasmer_result {
        Ok(result) => Ok(result),
        Err(error) => match error.downcast::<VMError>() {
            Ok(vm_error) => Err(vm_error),
            Err(wasmer_runtime_error) => {
                // NOTE: Can this be other variant than VMError and trap? This may indicate a bug in
                // our code.
                let wasmer_trap_code = wasmer_runtime_error.to_trap().expect("Trap code");
                Err(VMError::Trap(wasmer_trap_code.into()))
            }
        },
    }
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
        let result = exported_call_func.call(&mut self.store.as_store_mut());
        handle_wasmer_result(result)
    }

    fn call_function(&mut self, function_index: u32) -> Result<(), VMError> {
        let exported_runtime = self
            .env
            .as_ref(&self.store)
            .exported_runtime
            .as_ref()
            .expect("Valid exported runtime");
        let table = exported_runtime
            .exported_table
            .as_ref()
            .expect("should have table")
            .clone();

        // NOTE: This should be safe to unwrap as we're passing a valid function index that comes
        // from the manifest, and it should be present in the table. The table should be validated
        // at creation time for all cases i.e. correct signature, the pointer is actually stored in
        // the table, etc.
        let function = table
            .get(&mut self.store, function_index)
            .expect("should have valid entry in the table");

        let function = match function.funcref() {
            Some(Some(funcref)) => funcref
                .typed::<(), ()>(&self.store)
                .expect("should have valid signature"),
            Some(None) => {
                todo!("the entry exists buut there's no object stored in the table?")
            }
            None => {
                todo!("not a funcref");
            }
        };

        let wasmer_result = function.call(&mut self.store.as_store_mut());
        handle_wasmer_result(wasmer_result)
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
                     cb_ctx: u32| {
                        let wasmer_caller = WasmerCaller { env };
                        host::casper_read(
                            wasmer_caller,
                            key_space,
                            key_ptr,
                            key_size,
                            info_ptr,
                            cb_alloc,
                            cb_ctx,
                        )
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
                "casper_return",
                Function::new_typed_with_env(
                    &mut store,
                    &function_env,
                    |env: FunctionEnvMut<WasmerEnv<S>>, flags, data_ptr, data_len| {
                        let wasmer_caller = WasmerCaller { env };
                        host::casper_return(wasmer_caller, flags, data_ptr, data_len)
                    },
                ),
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
                     -> VMResult<u32> {
                        let wasmer_caller = WasmerCaller { env };
                        host::casper_copy_input(wasmer_caller, cb_alloc, cb_ctx)
                    },
                ),
            );

            imports.define(
                "env",
                "casper_return",
                Function::new_typed_with_env(
                    &mut store,
                    &function_env,
                    |env: FunctionEnvMut<WasmerEnv<S>>,
                     flags: u32,
                     data_ptr: u32,
                     data_len: u32| {
                        let wasmer_caller = WasmerCaller { env };
                        host::casper_return(wasmer_caller, flags, data_ptr, data_len)
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
                     code_ptr: u32,
                     code_size: u32,
                     manifest_ptr: u32,
                     selector: u32,
                     input_ptr: u32,
                     input_len: u32,
                     result_ptr: u32| {
                        let wasmer_caller = WasmerCaller { env };
                        match host::casper_create_contract(
                            wasmer_caller,
                            code_ptr,
                            code_size,
                            manifest_ptr,
                            selector,
                            input_ptr,
                            input_len,
                            result_ptr,
                        ) {
                            Ok(Ok(())) => Ok(0),
                            Ok(Err(call_error)) => Ok(call_error as u32),
                            Err(error) => Err(error),
                        }
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
                     selector: u32,
                     input_ptr: u32,
                     input_len: u32,
                     cb_alloc: u32,
                     cb_ctx: u32| {
                        let wasmer_caller = WasmerCaller { env };
                        match host::casper_call(
                            wasmer_caller,
                            address_ptr,
                            address_len,
                            value,
                            selector,
                            input_ptr,
                            input_len,
                            cb_alloc,
                            cb_ctx,
                        ) {
                            Ok(Ok(())) => Ok(0),
                            Ok(Err(call_error)) => Ok(call_error as u32),
                            Err(error) => Err(error),
                        }
                    },
                ),
            );

            imports.define(
                "env",
                "casper_env_read",
                Function::new_typed_with_env(
                    &mut store,
                    &function_env,
                    |env: FunctionEnvMut<WasmerEnv<S>>,
                     env_path,
                     env_path_size,
                     cb_alloc: u32,
                     cb_ctx: u32| {
                        let wasmer_caller = WasmerCaller { env };
                        host::casper_env_read(
                            wasmer_caller,
                            env_path,
                            env_path_size,
                            cb_alloc,
                            cb_ctx,
                        )
                    },
                ),
            );

            imports.define(
                "env",
                "casper_env_caller",
                Function::new_typed_with_env(
                    &mut store,
                    &function_env,
                    |env: FunctionEnvMut<WasmerEnv<S>>, dest_ptr, dest_len| {
                        let wasmer_caller = WasmerCaller { env };
                        host::casper_env_caller(wasmer_caller, dest_ptr, dest_len)
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

impl<S> WasmInstance<S> for WasmerInstance<S>
where
    S: Storage + 'static,
{
    fn call_export(&mut self, name: &str) -> (Result<(), VMError>, GasUsage) {
        let vm_result = self.call_export(name);
        let remaining_points = metering::get_remaining_points(&mut self.store, &self.instance);
        match remaining_points {
            metering::MeteringPoints::Remaining(remaining_points) => {
                let gas_usage = GasUsage {
                    gas_limit: self.config.gas_limit,
                    remaining_points,
                };
                (vm_result, gas_usage)
            }
            metering::MeteringPoints::Exhausted => {
                let gas_usage = GasUsage {
                    gas_limit: self.config.gas_limit,
                    remaining_points: 0,
                };
                (Err(VMError::OutOfGas), gas_usage)
            }
        }
    }

    fn call_function(&mut self, function_index: u32) -> (Result<(), VMError>, GasUsage) {
        let vm_result = self.call_function(function_index);
        let remaining_points = metering::get_remaining_points(&mut self.store, &self.instance);
        match remaining_points {
            metering::MeteringPoints::Remaining(remaining_points) => {
                let gas_usage = GasUsage {
                    gas_limit: self.config.gas_limit,
                    remaining_points,
                };
                (vm_result, gas_usage)
            }
            metering::MeteringPoints::Exhausted => {
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

        let Context { storage, address } = context;

        Context {
            storage: storage.clone(),
            address: *address,
        }
    }
}
