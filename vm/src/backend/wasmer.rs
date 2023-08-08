mod metering_middleware;

use std::{
    mem,
    ptr::NonNull,
    sync::{Arc, Mutex, Weak},
};

use wasmer::{
    AsStoreMut, AsStoreRef, CompilerConfig, Engine, Exports, Function, FunctionEnv, FunctionEnvMut,
    Imports, Instance, Memory, MemoryView, Module, RuntimeError, Store, Value, TypedFunction,
};
use wasmer_compiler_singlepass::Singlepass;
use wasmer_middlewares::{
    metering::{self, MeteringPoints},
    Metering,
};
use wasmer_types::TrapCode;

use crate::{host, storage::Storage, HostError, MemoryAccessError, TrapCode};

use self::metering_middleware::make_wasmer_metering_middleware;

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
    exported_runtime: ExportedRuntime,
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
        let mem = self.env.data().memory.as_ref().expect("should have memory");
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
            if runtime_error.to_trap() == Some(TrapCode::UnreachableCodeReached) {
                match metering::get_remaining_points(&mut store, instance) {
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

    fn alloc(&mut self, size: usize) -> Result<u32, VMError> {
        // let tref = self.env.data().instance.as_ref();
        let tref = self
            .env
            .data()
            .instance
            .upgrade()
            .expect("instance should be alive");

        call_alloc(&tref, &mut self.env, size)
            .map_err(|error| handle_runtime_error(error, &mut self.env, &tref))
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

/// Container for Wasm-provided exports such as alloc, dealloc, etc.
///
/// Let's call it a "minimal runtime" that is expected to exist inside a Wasm.
#[derive(Clone)]
pub(crate) struct ExportedRuntime {
    pub(crate) memory: Memory,
    pub(crate) exported_alloc_func: TypedFunction<u32, u32>,
}

pub(crate) struct WasmerInstance<S: Storage> {
    instance: Arc<Instance>,
    env: FunctionEnv<WasmerEnv<S>>,
    store: Store,
    exported_runtime: ExportedRuntime,
}

impl<S> WasmerInstance<S>
where
    S: Storage + 'static,
{
    pub(crate) fn call_export(&mut self, name: &str, args: &[&[u8]]) -> Result<(), VMError> {
        // Allocate memory inside the VM and copy over each argument into the provided pointer.
        let mut data_size: usize = args.iter().map(|arg| arg.len()).sum();
        data_size += args.len() * mem::size_of::<Slice>();

        let arg_ptr = self.exported_runtime.exported_alloc_func.call(&mut self.store, data_size.try_into().unwrap()).map_err(|error| handle_runtime_error(error, &mut self.store, &self.instance))?;
        let mut cur_ptr = arg_ptr;
        let mut slices = Vec::new();

        for arg in args {
            self.exported_runtime.memory.view(&self.store).write(cur_ptr as _, arg)?;
            slices.push(Slice {
                ptr: cur_ptr,
                size: arg.len() as _,
            });
            cur_ptr += arg.len() as u32;
        }
        let mut slices_ptr = cur_ptr;
        let mut ptr = slices_ptr;
        let mut slices_ptrs = Vec::new();
        for slice in slices {
            let slice_bytes: [u8; mem::size_of::<Slice>()] = unsafe { mem::transmute_copy(&slice) };
            // dbg!(&slice_bytes.len());
            memory
                .view(&self.store)
                .write(ptr as _, &slice_bytes)
                .unwrap();
            slices_ptrs.push(Value::I32(ptr as _));
            ptr += slice_bytes.len() as u32;
        }

        let exported_function = self
            .instance
            .exports
            // .get_typed_function::<(u64, u32), ()>(&self.store, name)
            .get_function(name)
            .expect("export error");

        // TODO: Possible optimization is to use get_typed_function to optimize the call, rather
        // than using dynamic interface.
        let result = exported_function.call(&mut self.store, &slices_ptrs);

        let remaining_points_1 = metering::get_remaining_points(&mut self.store, &self.instance);
        dbg!(&remaining_points_1);

        // let result = if args.len() == 2 {
        //     let typed = exported_function
        //         .typed::<(u32, u32), ()>(&mut self.store)
        //         .unwrap();
        //     typed.call(&mut self.store, slices_ptrs[0], slices_ptrs[1])
        // } else {
        //     todo!()
        // };
        match remaining_points_1 {
            metering::MeteringPoints::Remaining(_remaining_points) => {}
            metering::MeteringPoints::Exhausted => {
                todo!("exhausted after calling export {name}")
            }
        }

        let vm_result = match result {
            Ok(result) => Ok(()),
            Err(error) => match error.downcast::<VMError>() {
                Ok(vm_error) => Err(vm_error),
                Err(vm_error) => todo!("handle {vm_error:?}"),
            },
        };

        // let params = [
        //     Value::I64(args.len().try_into().unwrap()),
        //     Value::I32(slices_ptr as _)
        // ];

        call_dealloc(
            &self.instance,
            &mut self.store.as_store_mut(),
            arg_ptr,
            data_size,
        );

        let remaining_points_2 = metering::get_remaining_points(&mut self.store, &self.instance);
        dbg!(&remaining_points_2);

        match remaining_points_2 {
            metering::MeteringPoints::Remaining(_remaining_points) => {}
            metering::MeteringPoints::Exhausted => todo!("exhausted after calling final dealloc"),
        }
    }

    pub(crate) fn from_wasm_bytes(
        wasm_bytes: &[u8],
        context: Context<S>,
    ) -> Result<Self, BackendError> {
        // let mut store = Engine
        let engine = {
            let mut singlepass_compiler = Singlepass::new();
            let metering = make_wasmer_metering_middleware(context.initial_gas_limit);
            singlepass_compiler.push_middleware(metering);
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
                    Err(RuntimeError::user(Box::new(HostError::Revert { code })))
                    // host::casper_revert(code)
                }),
            );

            imports
        };

        // let exports = {
        //     let mut exports = Exports::new();
        //     exports.insert("alloc", Function)
        // }

        let instance =
            Arc::new(Instance::new(&mut store, &module, &imports).expect("should instantiate"));

            let exported_alloc_func = instance.exports.get_typed_function(&mut store, "alloc").map_err(|error| BackendError::Export(error.to_string()))?;

        let memory = instance
            .exports
            .get_memory("memory")
            .map_err(|error| BackendError::Export(error.to_string()))?;


        {
            let function_env_mut = function_env.as_mut(&mut store);
            function_env_mut.memory = Some(memory.clone());
            function_env_mut.instance = Arc::downgrade(&instance);
        }

        Ok(Self {
            instance,
            env: function_env,
            store,
            exported_alloc_func,
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
    fn call_export(&mut self, name: &str, args: &[&[u8]]) -> (Result<(), VMError>, GasSummary) {
        let vm_result = self.call_export(name, args);
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
        let WasmerEnv { context, .. } = env.as_ref(&store);

        let Context {
            initial_gas_limit: gas_limit,
            storage,
        } = context;

        Context {
            initial_gas_limit: *gas_limit,
            storage: storage.clone(),
        }
    }
}
