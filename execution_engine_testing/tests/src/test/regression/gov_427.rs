use std::convert::TryInto;

use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR, DEFAULT_WASM_CONFIG,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::{engine_state::Error, execution};
use casper_types::{addressable_entity::DEFAULT_ENTRY_POINT_NAME, RuntimeArgs};
use walrus::{ir::Value, FunctionBuilder, Module, ModuleConfig, ValType};

/// Creates a wasm with a function that contains local section with types in `repeated_pattern`
/// repeated `repeat_count` times with additional `extra_types` appended at the end of local group.
fn make_arbitrary_local_count(
    repeat_count: usize,
    repeat_pattern: &[ValType],
    extra_types: &[ValType],
) -> Vec<u8> {
    let mut module = Module::with_config(ModuleConfig::new());

    let _memory_id = module.memories.add_local(false, 11, None);

    let mut func_with_locals = FunctionBuilder::new(&mut module.types, &[], &[]);

    let mut locals = Vec::new();
    for _ in 0..repeat_count {
        for val_type in repeat_pattern {
            let local = module.locals.add(*val_type);
            locals.push((local, *val_type));
        }
    }

    for extra_type in extra_types {
        let local = module.locals.add(*extra_type);
        locals.push((local, *extra_type));
    }

    for (i, (local, val_type)) in locals.into_iter().enumerate() {
        let value = match val_type {
            ValType::I32 => Value::I32(i.try_into().unwrap()),
            ValType::I64 => Value::I64(i.try_into().unwrap()),
            ValType::F32 => Value::F32(i as f32),
            ValType::F64 => Value::F64(i as f64),
            ValType::V128 => Value::V128(i.try_into().unwrap()),
            ValType::Externref | ValType::Funcref => todo!("{:?}", val_type),
        };
        func_with_locals.func_body().const_(value).local_set(local);
    }

    let func_with_locals = func_with_locals.finish(vec![], &mut module.funcs);

    let mut call_func = FunctionBuilder::new(&mut module.types, &[], &[]);

    call_func.func_body().call(func_with_locals);

    let call = call_func.finish(Vec::new(), &mut module.funcs);

    module.exports.add(DEFAULT_ENTRY_POINT_NAME, call);

    module.emit_wasm()
}

#[ignore]
#[test]
fn too_many_locals_should_exceed_stack_height() {
    const CALL_COST: usize = 1;
    let extra_types = [ValType::I32];
    let repeat_pattern = [ValType::I64];
    let max_stack_height = DEFAULT_WASM_CONFIG.max_stack_height as usize;

    let success_wasm_bytes: Vec<u8> = make_arbitrary_local_count(
        max_stack_height - extra_types.len() - CALL_COST - 1,
        &repeat_pattern,
        &extra_types,
    );

    let failing_wasm_bytes: Vec<u8> = make_arbitrary_local_count(
        max_stack_height - extra_types.len() - CALL_COST,
        &repeat_pattern,
        &extra_types,
    );

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let success_request = ExecuteRequestBuilder::module_bytes(
        *DEFAULT_ACCOUNT_ADDR,
        success_wasm_bytes,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(success_request).expect_success().commit();

    let failing_request = ExecuteRequestBuilder::module_bytes(
        *DEFAULT_ACCOUNT_ADDR,
        failing_wasm_bytes,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(failing_request).expect_failure().commit();

    let error = builder.get_error().expect("should have error");

    // Here we pass the preprocess stage, but we fail at stack height limiter as we do have very
    // restrictive default stack height.
    assert!(
        matches!(
            &error,
            Error::Exec(execution::Error::Interpreter(s)) if s.contains("Unreachable")
        ),
        "{:?}",
        error
    );
}
