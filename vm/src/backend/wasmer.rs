mod metering_middleware;

use std::{
    mem,
    ptr::NonNull,
    sync::{Arc, Mutex, Weak},
};

use bytes::Bytes;
use wasmer::{
    wasmparser::{Export, MemoryType},
    AsStoreMut, AsStoreRef, CompilerConfig, Engine, Exports, Function, FunctionEnv, FunctionEnvMut,
    Imports, Instance, Memory, MemoryView, Module, RuntimeError, Store, Table, TypedFunction,
    Value, ExportError,
};
use wasmer_compiler_singlepass::Singlepass;
use wasmer_middlewares::{
    metering::{self, MeteringPoints},
    Metering,
};
use wasmer_types::TrapCode;

use crate::{
    host::{self, Outcome},
    storage::Storage,
    Config, HostError, Resolver,
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

unsafe impl<S: Storage> Send for WasmerEnv<S> {}
unsafe impl<S: Storage> Sync for WasmerEnv<S> {}

// fn call_alloc<E: AsStoreMut>(
//     instance: &Instance,
//     mut env: E,
//     size: usize,
// ) -> Result<u32, VMError> {
//     // println!("call_alloc({})", size);
//     // let tref = self.env.data().instance.as_ref();
//     // let func = instance
//     //     .exports
//     //     .get_typed_function::<u32, u32>(&env, "alloc")

//     // func.call(&mut env, size.try_into().unwrap());
// }

// fn call_dealloc<E: AsStoreMut>(
//     instance: &Instance,
//     mut env: E,
//     ptr: u32,
//     size: usize,
// ) -> Result<(), RuntimeError> {
//     // println!("call_alloc({})", size);
//     // let tref = self.env.data().instance.as_ref();
//     let func = instance
//         .exports
//         .get_typed_function::<(u32, u32), ()>(&env, "dealloc")
//         .expect("should have function");
//     func.call(&mut env, ptr, size.try_into().unwrap())
// }

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
        Ok(host_error) => return VMError::Host(host_error),
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
            .expect("should have table exported") // TODO: if theres no table then no function pointer is stored in the wasm blob - probably safe
            .get(&mut store.as_store_mut(), idx)
            .expect("has entry in the table"); // TODO: better error handling - pass 0 as nullptr?
        let funcref = value.funcref().expect("is funcref");
        let valid_funcref = funcref.as_ref().expect("valid funcref");
        let alloc_callback: TypedFunction<(u32, u32), u32> = valid_funcref.typed(&mut store)?;
        let ptr = alloc_callback.call(&mut store.as_store_mut(), size.try_into().unwrap(), ctx)?;
        Ok(ptr)
    }

    fn config(&self) -> &Config {
        &self.env.data().config
    }

    fn bytecode(&self) -> Bytes {
        self.env.data().bytecode.clone()
        // let instance = self
        //     .env
        //     .data()
        //     .instance
        //     .upgrade()
        //     .expect("instance should be alive");
        // todo!()
        // let weak: Weak<dyn WasmInstance<S>> = Arc::downgrade(&concrete);
        // weak.upgrade().expect("fine")
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
    pub(crate) exported_call_func: TypedFunction<(), ()>,
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

    /// Calls into `alloc` function to inject a data structure containing all the arguments
    /// passed, together with a list of pointers pointing at each memory chunk to
    /// allow Wasm to easily retrieve the data.
    // pub(crate) fn inject_arguments(&mut self, arguments: &[u8]) -> Result<Vec<Value>, VMError> {
    //     // Allocate memory inside the VM and copy over each argument into the provided pointer.
    //     let mut data_size: usize = args.iter().map(|arg| arg.len()).sum();
    //     data_size += args.len() * mem::size_of::<Slice>();

    //     let arg_ptr = self
    //         .wasmer_env()
    //         .exported_runtime()
    //         .exported_alloc_func
    //         .clone()
    //         .call(&mut self.store, data_size.try_into().unwrap())
    //         .map_err(|error| handle_runtime_error(error, &mut self.store, &self.instance))?;
    //     let mut cur_ptr = arg_ptr;
    //     let mut slices = Vec::new();

    //     for arg in args {
    //         self.wasmer_env()
    //             .exported_runtime()
    //             .memory
    //             .view(&self.store)
    //             .write(cur_ptr as _, arg)
    //             .map_err(|error| VMError::Runtime {
    //                 message: error.to_string(),
    //             })?;
    //         slices.push(Slice {
    //             ptr: cur_ptr,
    //             size: arg.len() as _,
    //         });
    //         cur_ptr += arg.len() as u32;
    //     }
    //     let mut slices_ptr = cur_ptr;
    //     let mut ptr = slices_ptr;
    //     let mut slices_ptrs = Vec::new();
    //     for slice in slices {
    //         let slice_bytes: [u8; mem::size_of::<Slice>()] = unsafe { mem::transmute_copy(&slice)
    // };         // dbg!(&slice_bytes.len());
    //         self.wasmer_env()
    //             .exported_runtime()
    //             .memory
    //             .view(&self.store)
    //             .write(ptr as _, &slice_bytes)
    //             .map_err(|error| VMError::Runtime {
    //                 message: error.to_string(),
    //             })?;
    //         slices_ptrs.push(Value::I32(ptr as _));
    //         ptr += slice_bytes.len() as u32;
    //     }
    //     Ok(slices_ptrs)
    // }

    pub(crate) fn call_export(&mut self, name: &str) -> Result<(), VMError> {
        // TODO: Possible optimization is to use get_typed_function to optimize the call, rather
        // than using dynamic interface.
        // let slices_ptrs = self.inject_arguments(args)?;

        // let exported_function = self
        //     .instance
        //     .exports
        //     // .get_typed_function::<(u64, u32), ()>(&self.store, name)
        //     .get_function(name)
        //     .map_err(|_error| {
        //         VMError::Resolver(Resolver::Export {
        //             name: name.to_string(),
        //         })
        //     })?;

        let result = self
            .wasmer_env()
            .exported_runtime()
            .exported_call_func
            .clone()
            .call(&mut self.store.as_store_mut());

        let remaining_points_1 = metering::get_remaining_points(&mut self.store, &self.instance);
        dbg!(&remaining_points_1);

        match remaining_points_1 {
            metering::MeteringPoints::Remaining(_remaining_points) => {}
            metering::MeteringPoints::Exhausted => {
                // todo!("exhausted after calling export {name}")
                return Err(VMError::OutOfGas);
            }
        }

        // NOTE: We don't need to dealloc after calling an export, since the instance will likely.
        // not be reused when calling an export. We'll revisit this most likely
        //     .exported_runtime()
        //     .exported_dealloc_func
        //     .clone()
        //     .call(&mut self.store, arg_ptr, data_size.try_into().unwrap())
        //     .map_err(|error| handle_runtime_error(error, &mut self.store, &self.instance))?;

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
                     result_ptr: u32|
                     -> u32 {
                        let wasmer_caller = WasmerCaller { env };
                        host::casper_create_contract(
                            wasmer_caller,
                            code_ptr,
                            code_size,
                            manifest_ptr,
                            result_ptr,
                        )
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
        let table = match instance
            .exports
            .get_table("__indirect_function_table") {
                Ok(table) => Some(table.clone()),
                Err(error @ ExportError::IncompatibleType) => return Err(BackendError::Export(error.to_string())),
                Err(ExportError::Missing(_)) => None,
            };

        let exported_call_func = instance
            .exports
            .get_typed_function(&mut store, "call")
            .map_err(|error| BackendError::Export(error.to_string()))?;

        // let memory = instance
        //     .exports
        //     .get_memory("memory")
        //     .map_err(|error| BackendError::Export(error.to_string()))?;

        {
            let function_env_mut = function_env.as_mut(&mut store);
            function_env_mut.instance = Arc::downgrade(&instance);
            function_env_mut.exported_runtime = Some(ExportedRuntime {
                memory: memory.clone(),
                exported_table: table,
                exported_call_func,
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

type wasm32_ptr = u32;
type wasm32_usize = u32;

#[repr(C)]
struct Slice {
    ptr: wasm32_ptr,
    size: wasm32_usize,
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
                return (vm_result, gas_usage);
            }
            MeteringPoints::Exhausted => {
                let gas_usage = GasUsage {
                    gas_limit: self.config.gas_limit,
                    remaining_points: 0,
                };
                return (Err(VMError::OutOfGas), gas_usage);
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
