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
use wasmer_middlewares::metering;

use crate::{
    host, storage::GlobalStateReader, u32_from_host_result, Config, Executor, TrapCode, VMResult,
    WasmEngine,
};

use self::metering_middleware::make_wasmer_metering_middleware;

use super::{Caller, Context, GasUsage, MeteringPoints, PreparationError, WasmInstance};
use crate::VMError;

impl From<wasmer::MemoryAccessError> for TrapCode {
    fn from(error: wasmer::MemoryAccessError) -> Self {
        match error {
            wasmer::MemoryAccessError::HeapOutOfBounds | wasmer::MemoryAccessError::Overflow => {
                // As according to Wasm spec section `Memory Instructions` any access to memory that is out of bounds of the memory's current size is a trap.
                // Reference: https://webassembly.github.io/spec/core/syntax/instructions.html#memory-instructions
                TrapCode::MemoryOutOfBounds
            }
            wasmer::MemoryAccessError::NonUtf8String => {
                // This can happen only when using wasmer's utf8 reading routines which we don't need.
                unreachable!("NonUtf8String")
            }
            _ => {
                // All errors are handled and converted to a trap code, but we have to add this as wasmer's errors are #[non_exhaustive]
                unreachable!("Unexpected error: {:?}", error)
            }
        }
    }
}

impl From<wasmer_types::TrapCode> for TrapCode {
    fn from(value: wasmer_types::TrapCode) -> Self {
        match value {
            wasmer_types::TrapCode::StackOverflow => TrapCode::StackOverflow,
            wasmer_types::TrapCode::HeapAccessOutOfBounds => TrapCode::MemoryOutOfBounds,
            wasmer_types::TrapCode::HeapMisaligned => {
                unreachable!("Atomic operations are not supported")
            }
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

pub(crate) struct WasmerEngine(());

struct WasmerEnv<S: GlobalStateReader, E: Executor> {
    config: Config,
    context: Context<S, E>,
    instance: Weak<Instance>,
    bytecode: Bytes,
    exported_runtime: Option<ExportedRuntime>,
}

pub(crate) struct WasmerCaller<'a, S: GlobalStateReader, E: Executor> {
    env: FunctionEnvMut<'a, WasmerEnv<S, E>>,
}

impl<'a, S: GlobalStateReader + 'static, E: Executor + 'static> WasmerCaller<'a, S, E> {
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

impl<'a, S: GlobalStateReader + 'static, E: Executor + 'static> Caller<S, E>
    for WasmerCaller<'a, S, E>
{
    fn memory_write(&self, offset: u32, data: &[u8]) -> Result<(), VMError> {
        Ok(self
            .with_memory(|mem| mem.write(offset.into(), data))
            .map_err(TrapCode::from)
            .map_err(VMError::Trap)?)
    }

    fn context(&self) -> &Context<S, E> {
        &self.env.data().context
    }
    fn context_mut(&mut self) -> &mut Context<S, E> {
        &mut self.env.data_mut().context
    }

    fn memory_read_into(&self, offset: u32, output: &mut [u8]) -> Result<(), VMError> {
        Ok(self
            .with_memory(|mem| mem.read(offset.into(), output))
            .map_err(TrapCode::from)
            .map_err(VMError::Trap)?)
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
        let ptr = alloc_callback
            .call(&mut store.as_store_mut(), size.try_into().unwrap(), ctx)
            .map_err(handle_wasmer_runtime_error)?;
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

impl<S: GlobalStateReader, E: Executor> WasmerEnv<S, E> {}

impl<S: GlobalStateReader, E: Executor> WasmerEnv<S, E> {
    fn new(config: Config, context: Context<S, E>, code: Bytes) -> Self {
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

pub(crate) struct WasmerInstance<S: GlobalStateReader, E: Executor + 'static> {
    instance: Arc<Instance>,
    env: FunctionEnv<WasmerEnv<S, E>>,
    store: Store,
    config: Config,
}

fn handle_wasmer_runtime_error(error: RuntimeError) -> VMError {
    match error.downcast::<VMError>() {
        Ok(vm_error) => vm_error,
        Err(wasmer_runtime_error) => {
            // NOTE: Can this be other variant than VMError and trap? This may indicate a bug in
            // our code.
            let wasmer_trap_code = wasmer_runtime_error.to_trap().expect("Trap code");
            VMError::Trap(wasmer_trap_code.into())
        }
    }
}

impl<S, E> WasmerInstance<S, E>
where
    S: GlobalStateReader + 'static,
    E: Executor + 'static,
{
    pub(crate) fn call_export(&mut self, name: &str) -> Result<(), VMError> {
        let exported_call_func: TypedFunction<(), ()> = self
            .instance
            .exports
            .get_typed_function(&self.store, name)
            .expect("Validated module has expected export");
        let () = exported_call_func
            .call(&mut self.store.as_store_mut())
            .map_err(handle_wasmer_runtime_error)?;
        Ok(())
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

        let () = function
            .call(&mut self.store.as_store_mut())
            .map_err(handle_wasmer_runtime_error)?;
        Ok(())
    }

    pub(crate) fn from_wasm_bytes<C: Into<Bytes>>(
        wasm_bytes: C,
        context: Context<S, E>,
        config: Config,
    ) -> Result<Self, PreparationError> {
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
            .map_err(|error| PreparationError::Compile(error.to_string()))?;

        let mut store = Store::new(engine);

        let wasmer_env = WasmerEnv::new(config.clone(), context, wasm_bytes);
        let function_env = FunctionEnv::new(&mut store, wasmer_env);

        let memory = Memory::new(
            &mut store,
            wasmer_types::MemoryType {
                minimum: wasmer_types::Pages(17),
                maximum: None, //Some(wasmer_types::Pages(17 * 4)),
                shared: false,
            },
        )
        .map_err(|error| PreparationError::Memory(error.to_string()))?;

        let imports = {
            let mut imports = Imports::new();
            imports.define("env", "memory", memory.clone());

            imports.define(
                "env",
                "casper_write",
                Function::new_typed_with_env(
                    &mut store,
                    &function_env,
                    |env: FunctionEnvMut<WasmerEnv<S, E>>,
                     key_space: u64,
                     key_ptr: u32,
                     key_size: u32,
                     value_ptr: u32,
                     value_size: u32| {
                        let wasmer_caller = WasmerCaller { env };
                        host::casper_write(
                            wasmer_caller,
                            key_space,
                            key_ptr,
                            key_size,
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
                    |env: FunctionEnvMut<WasmerEnv<S, E>>,
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
                    |env: FunctionEnvMut<WasmerEnv<S, E>>, message_ptr: u32, message_size: u32| {
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
                    |env: FunctionEnvMut<WasmerEnv<S, E>>, flags, data_ptr, data_len| {
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
                    |env: FunctionEnvMut<WasmerEnv<S, E>>,
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
                    |env: FunctionEnvMut<WasmerEnv<S, E>>,
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
                "casper_create",
                Function::new_typed_with_env(
                    &mut store,
                    &function_env,
                    |env: FunctionEnvMut<WasmerEnv<S, E>>,
                     code_ptr: u32,
                     code_size: u32,
                     manifest_ptr: u32,
                     value: u64,
                     selector: u32,
                     input_ptr: u32,
                     input_len: u32,
                     result_ptr: u32| {
                        let wasmer_caller = WasmerCaller { env };
                        match host::casper_create(
                            wasmer_caller,
                            code_ptr,
                            code_size,
                            manifest_ptr,
                            value,
                            selector,
                            input_ptr,
                            input_len,
                            result_ptr,
                        ) {
                            Ok(Ok(())) => Ok(0),
                            Ok(Err(call_error)) => Ok(call_error.into_u32()),
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
                    |env: FunctionEnvMut<WasmerEnv<S, E>>,
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
                            Ok(host_result) => Ok(u32_from_host_result(host_result)),
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
                    |env: FunctionEnvMut<WasmerEnv<S, E>>,
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
                    |env: FunctionEnvMut<WasmerEnv<S, E>>, dest_ptr, dest_len| {
                        let wasmer_caller = WasmerCaller { env };
                        host::casper_env_caller(wasmer_caller, dest_ptr, dest_len)
                    },
                ),
            );

            imports.define(
                "env",
                "casper_env_value",
                Function::new_typed_with_env(
                    &mut store,
                    &function_env,
                    |env: FunctionEnvMut<WasmerEnv<S, E>>| {
                        let wasmer_caller = WasmerCaller { env };
                        host::casper_env_value(wasmer_caller)
                    },
                ),
            );

            imports.define(
                "env",
                "casper_env_balance",
                Function::new_typed_with_env(
                    &mut store,
                    &function_env,
                    |env: FunctionEnvMut<WasmerEnv<S, E>>,
                     entity_kind,
                     entity_addr,
                     entity_addr_len| {
                        let wasmer_caller = WasmerCaller { env };
                        host::casper_env_balance(
                            wasmer_caller,
                            entity_kind,
                            entity_addr,
                            entity_addr_len,
                        )
                    },
                ),
            );

            imports.define(
                "env",
                "casper_transfer",
                Function::new_typed_with_env(
                    &mut store,
                    &function_env,
                    |env: FunctionEnvMut<WasmerEnv<S, E>>, address_ptr, address_len, amount| {
                        let wasmer_caller = WasmerCaller { env };
                        host::casper_transfer(wasmer_caller, address_ptr, address_len, amount)
                    },
                ),
            );

            imports
        };

        // TODO: Deal with "start" section that executes actual Wasm - test, measure gas, etc. ->
        // Instance::new may fail with RuntimError

        let instance = {
            let instance = Instance::new(&mut store, &module, &imports)
                .map_err(|error| PreparationError::Instantiation(error.to_string()))?;

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
                return Err(PreparationError::MissingExport(error.to_string()))
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

impl<S, E> WasmInstance<S, E> for WasmerInstance<S, E>
where
    S: GlobalStateReader + 'static,
    E: Executor + 'static,
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
    fn teardown(self) -> Context<S, E> {
        let WasmerInstance { env, mut store, .. } = self;

        let mut env_mut = env.into_mut(&mut store);

        let data = env_mut.data_mut();

        // NOTE: There must be a better way than re-creating the object based on consumed fields.

        Context {
            initiator: data.context.initiator,
            caller: data.context.caller,
            callee: data.context.callee,
            value: data.context.value,
            tracking_copy: data.context.tracking_copy.fork2(),
            executor: data.context.executor.clone(),
            transaction_hash: data.context.transaction_hash,
            address_generator: Arc::clone(&data.context.address_generator),
        }
    }
}
