use std::fmt::Write;

use casper_wasm::{
    builder,
    elements::{Instruction, Instructions},
};

use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::{
    engine_state, execution,
    runtime::{
        PreprocessingError, WasmValidationError, DEFAULT_BR_TABLE_MAX_SIZE, DEFAULT_MAX_GLOBALS,
        DEFAULT_MAX_PARAMETER_COUNT, DEFAULT_MAX_TABLE_SIZE,
    },
};
use casper_types::{addressable_entity::DEFAULT_ENTRY_POINT_NAME, RuntimeArgs};

use crate::wasm_utils;

const OOM_INIT: (u32, Option<u32>) = (2805655325, None);
const FAILURE_ONE_ABOVE_LIMIT: (u32, Option<u32>) = (DEFAULT_MAX_TABLE_SIZE + 1, None);
const FAILURE_MAX_ABOVE_LIMIT: (u32, Option<u32>) = (DEFAULT_MAX_TABLE_SIZE, Some(u32::MAX));
const FAILURE_INIT_ABOVE_LIMIT: (u32, Option<u32>) =
    (DEFAULT_MAX_TABLE_SIZE, Some(DEFAULT_MAX_TABLE_SIZE + 1));
const ALLOWED_NO_MAX: (u32, Option<u32>) = (DEFAULT_MAX_TABLE_SIZE, None);
const ALLOWED_LIMITS: (u32, Option<u32>) = (DEFAULT_MAX_TABLE_SIZE, Some(DEFAULT_MAX_TABLE_SIZE));
// Anything larger than that fails wasmi interpreter with a runtime stack overflow.
const FAILING_BR_TABLE_SIZE: usize = DEFAULT_BR_TABLE_MAX_SIZE as usize + 1;
const FAILING_GLOBALS_SIZE: usize = DEFAULT_MAX_PARAMETER_COUNT as usize + 1;
const FAILING_PARAMS_COUNT: usize = DEFAULT_MAX_PARAMETER_COUNT as usize + 1;

fn make_oom_payload(initial: u32, maximum: Option<u32>) -> Vec<u8> {
    let mut bounds = initial.to_string();
    if let Some(max) = maximum {
        bounds += " ";
        bounds += &max.to_string();
    }

    let wat = format!(
        r#"(module
            (table (;0;) {} funcref)
            (memory (;0;) 0)
            (export "call" (func $call))
            (func $call))
            "#,
        bounds
    );
    wabt::wat2wasm(wat).expect("should parse wat")
}

#[ignore]
#[test]
fn should_not_oom() {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let initial_size_exceeded = vec![OOM_INIT, FAILURE_ONE_ABOVE_LIMIT];

    let max_size_exceeded = vec![FAILURE_MAX_ABOVE_LIMIT, FAILURE_INIT_ABOVE_LIMIT];

    for (initial, maximum) in initial_size_exceeded {
        let module_bytes = make_oom_payload(initial, maximum);
        let exec_request = ExecuteRequestBuilder::module_bytes(
            *DEFAULT_ACCOUNT_ADDR,
            module_bytes,
            RuntimeArgs::default(),
        )
        .build();

        builder.exec(exec_request).expect_failure().commit();

        let error = builder.get_error().unwrap();

        assert!(
            matches!(
                error,
                engine_state::Error::WasmPreprocessing(PreprocessingError::WasmValidation(WasmValidationError::InitialTableSizeExceeded { max, actual }))
                if max == DEFAULT_MAX_TABLE_SIZE && actual == initial
            ),
            "{:?}",
            error
        );
    }

    for (initial, maximum) in max_size_exceeded {
        let module_bytes = make_oom_payload(initial, maximum);
        let exec_request = ExecuteRequestBuilder::module_bytes(
            *DEFAULT_ACCOUNT_ADDR,
            module_bytes,
            RuntimeArgs::default(),
        )
        .build();

        builder.exec(exec_request).expect_failure().commit();

        let error = builder.get_error().unwrap();

        assert!(
            matches!(
                error,
                engine_state::Error::WasmPreprocessing(PreprocessingError::WasmValidation(WasmValidationError::MaxTableSizeExceeded { max, actual }))
                if max == DEFAULT_MAX_TABLE_SIZE && Some(actual) == maximum
            ),
            "{initial} {maximum:?} {:?}",
            error
        );
    }
}

#[ignore]
#[test]
fn should_pass_table_validation() {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let passing_test_cases = vec![ALLOWED_NO_MAX, ALLOWED_LIMITS];

    for (initial, maximum) in passing_test_cases {
        let module_bytes = make_oom_payload(initial, maximum);
        let exec_request = ExecuteRequestBuilder::module_bytes(
            *DEFAULT_ACCOUNT_ADDR,
            module_bytes,
            RuntimeArgs::default(),
        )
        .build();

        builder.exec(exec_request).expect_success().commit();
    }
}

#[ignore]
#[test]
fn should_pass_elem_section() {
    // more functions than elements - wasmi doesn't allocate
    let elem_does_not_fit_err = test_element_section(0, None, DEFAULT_MAX_TABLE_SIZE);
    assert!(
        matches!(
            elem_does_not_fit_err,
            Some(engine_state::Error::Exec(execution::Error::Interpreter(ref msg)))
            if msg == "elements segment does not fit"
        ),
        "{:?}",
        elem_does_not_fit_err
    );

    // wasmi assumes table size and function pointers are equal
    assert!(matches!(
        test_element_section(
            DEFAULT_MAX_TABLE_SIZE,
            Some(DEFAULT_MAX_TABLE_SIZE),
            DEFAULT_MAX_TABLE_SIZE
        ),
        None
    ));
}

fn test_element_section(
    table_init: u32,
    table_max: Option<u32>,
    function_count: u32,
) -> Option<engine_state::Error> {
    // Ensures proper initialization of table elements for different number of function pointers
    //
    // This should ensure there's no hidden lazy allocation and initialization that might still
    // overallocate memory, burn cpu cycles allocating etc.

    // (module
    //     (table 0 1 anyfunc)
    //     (memory $0 1)
    //     (export "memory" (memory $0))
    //     (export "foo1" (func $foo1))
    //     (export "foo2" (func $foo2))
    //     (export "foo3" (func $foo3))
    //     (export "main" (func $main))
    //     (func $foo1 (; 0 ;)
    //     )
    //     (func $foo2 (; 1 ;)
    //     )
    //     (func $foo3 (; 2 ;)
    //     )
    //     (func $main (; 3 ;) (result i32)
    //      (i32.const 0)
    //     )
    //     (elem (i32.const 0) $foo1 $foo2 $foo3)
    // )

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let mut wat = String::new();

    if let Some(max) = table_max {
        writeln!(
            wat,
            r#"(module
            (table {table_init} {max} anyfunc)"#
        )
        .unwrap();
    } else {
        writeln!(
            wat,
            r#"(module
            (table {table_init} anyfunc)"#
        )
        .unwrap();
    }

    wat += "(memory $0 1)\n";
    wat += r#"(export "memory" (memory $0))"#;
    wat += "\n";
    wat += r#"(export "call" (func $call))"#;
    wat += "\n";
    for i in 0..function_count {
        writeln!(wat, r#"(export "foo{i}" (func $foo{i}))"#).unwrap();
    }
    for i in 0..function_count {
        writeln!(wat, "(func $foo{i} (; 0 ;))").unwrap();
    }
    wat += "(func $call)\n";
    wat += "\n";
    wat += "(elem (i32.const 0) ";
    for i in 0..function_count {
        write!(wat, "$foo{i} ").unwrap();
    }
    wat += ")\n";
    wat += ")";

    let module_bytes = wabt::wat2wasm(wat).unwrap();
    let exec_request = ExecuteRequestBuilder::module_bytes(
        *DEFAULT_ACCOUNT_ADDR,
        module_bytes,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request).commit();

    builder.get_error()
}

#[ignore]
#[test]
fn should_not_allow_more_than_one_table() {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    // wabt::wat2wasm doesn't allow multiple tables so we'll go with a builder

    let module = builder::module()
        // table 1
        .table()
        .with_min(0)
        .build()
        // table 2
        .table()
        .with_min(1)
        .build()
        .function()
        // A signature with 0 params and no return type
        .signature()
        .build()
        .body()
        // Generated instructions for our entrypoint
        .with_instructions(Instructions::new(vec![Instruction::Nop, Instruction::End]))
        .build()
        .build()
        // Export above function
        .export()
        .field(DEFAULT_ENTRY_POINT_NAME)
        .build()
        // Memory section is mandatory
        .memory()
        .build()
        .build();
    let module_bytes = casper_wasm::serialize(module).expect("should serialize");

    let exec_request = ExecuteRequestBuilder::module_bytes(
        *DEFAULT_ACCOUNT_ADDR,
        module_bytes,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request).expect_failure().commit();

    let error = builder.get_error().unwrap();

    assert!(
        matches!(
            error,
            engine_state::Error::WasmPreprocessing(PreprocessingError::WasmValidation(
                WasmValidationError::MoreThanOneTable
            ))
        ),
        "{:?}",
        error
    );
}

/// Generates arbitrary length br_table opcode trying to exploit memory allocation in the wasm
/// parsing code.
fn make_arbitrary_br_table(size: usize) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    // (module
    // (type (;0;) (func (param i32) (result i32)))
    // (type (;1;) (func))
    // (func (;0;) (type 0) (param i32) (result i32)
    //   block  ;; label = @1
    //     block  ;; label = @2
    //       block  ;; label = @3
    //         block  ;; label = @4
    //           local.get 0
    //           br_table 2 (;@2;) 1 (;@3;) 0 (;@4;) 3 (;@1;)
    //         end
    //         i32.const 100
    //         return
    //       end
    //       i32.const 101
    //       return
    //     end
    //     i32.const 102
    //     return
    //   end
    //   i32.const 103
    //   return)
    // (func (;1;) (type 1)
    //   i32.const 0
    //   call 0
    //   drop)
    // (memory (;0;) 0)
    // (export "call" (func 1)))

    let mut src = String::new();
    writeln!(src, "(module")?;
    writeln!(src, "(memory (;0;) 0)")?;
    writeln!(src, r#"(export "call" (func $call))"#)?;
    writeln!(src, r#"(func $switch_like (param $p i32) (result i32)"#)?;

    let mut bottom = ";;\n(get_local $p)\n".to_string();
    bottom += "(br_table\n";

    for (br_table_offset, n) in (0..=size - 1).rev().enumerate() {
        writeln!(bottom, "  {n} ;; param == {br_table_offset} => (br {n})")?; // p == 0 => (br n)
    }
    writeln!(bottom, "{size})) ;; else => (br {size})")?;

    bottom += ";;";

    for n in 0..=size {
        let mut wrap = String::new();
        writeln!(wrap, "(block")?;
        writeln!(wrap, "{bottom}")?;
        writeln!(wrap, "(i32.const {val})", val = 100 + n)?;
        writeln!(wrap, "(return))")?;
        bottom = wrap;
    }

    writeln!(src, "{bottom}")?;

    writeln!(
        src,
        r#"(func $call (drop (call $switch_like (i32.const 0))))"#
    )?;

    writeln!(src, ")")?;

    let module_bytes = wat::parse_str(&src)?;
    Ok(module_bytes)
}

#[ignore]
#[test]
fn should_allow_large_br_table() {
    // Anything larger than that fails wasmi interpreter with a runtime stack overflow.
    let module_bytes = make_arbitrary_br_table(DEFAULT_BR_TABLE_MAX_SIZE as usize)
        .expect("should create module bytes");

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let exec_request = ExecuteRequestBuilder::module_bytes(
        *DEFAULT_ACCOUNT_ADDR,
        module_bytes,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request).expect_success().commit();
}

#[ignore]
#[test]
fn should_not_allow_large_br_table() {
    let module_bytes =
        make_arbitrary_br_table(FAILING_BR_TABLE_SIZE).expect("should create module bytes");

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let exec_request = ExecuteRequestBuilder::module_bytes(
        *DEFAULT_ACCOUNT_ADDR,
        module_bytes,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request).expect_failure().commit();

    let error = builder.get_error().expect("should fail");

    assert!(
        matches!(
            error,
            engine_state::Error::WasmPreprocessing(PreprocessingError::WasmValidation(WasmValidationError::BrTableSizeExceeded { max, actual }))
            if max == DEFAULT_BR_TABLE_MAX_SIZE && actual == FAILING_BR_TABLE_SIZE
        ),
        "{:?}",
        error,
    );
}

fn make_arbitrary_global(size: usize) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    // (module
    //   (memory $0 1)
    //   (global $global0 i32 (i32.const 1))
    //   (global $global1 i32 (i32.const 2))
    //   (global $global2 i32 (i32.const 3))
    //   (func (export "call")
    //     global.get $global0
    //     global.get $global1
    //     global.get $global2
    //     i32.add
    //     i32.add
    //     drop
    //   )
    // )
    let mut src = String::new();
    writeln!(src, "(module")?;
    writeln!(src, "  (memory $0 1)")?;

    for i in 0..size {
        writeln!(
            src,
            "  (global $global{i} i32 (i32.const {value}))",
            value = i + 1
        )?;
    }

    writeln!(src, r#"  (func (export "call")"#)?;
    debug_assert!(size >= 2);
    writeln!(src, "    global.get $global{last}", last = size - 2)?;
    writeln!(src, "    global.get $global{last}", last = size - 1)?;
    writeln!(src, "    i32.add")?;
    writeln!(src, "    drop")?; // drop the result
    writeln!(src, "  )")?;
    writeln!(src, ")")?;
    let module_bytes = wat::parse_str(&src)?;
    Ok(module_bytes)
}

#[ignore]
#[test]
fn should_allow_multiple_globals() {
    let module_bytes =
        make_arbitrary_global(DEFAULT_MAX_GLOBALS as usize).expect("should make arbitrary global");

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let exec_request = ExecuteRequestBuilder::module_bytes(
        *DEFAULT_ACCOUNT_ADDR,
        module_bytes,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request).expect_success().commit();
}

#[ignore]
#[test]
fn should_not_allow_too_many_globals() {
    let module_bytes =
        make_arbitrary_global(FAILING_GLOBALS_SIZE).expect("should make arbitrary global");

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let exec_request = ExecuteRequestBuilder::module_bytes(
        *DEFAULT_ACCOUNT_ADDR,
        module_bytes,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request).expect_failure().commit();

    let error = builder.get_error().expect("should fail");

    assert!(
        matches!(
            error,
            engine_state::Error::WasmPreprocessing(PreprocessingError::WasmValidation(WasmValidationError::TooManyGlobals { max, actual }))
            if max == DEFAULT_MAX_GLOBALS && actual == FAILING_GLOBALS_SIZE
        ),
        "{:?}",
        error,
    );
}

#[ignore]
#[test]
fn should_verify_max_param_count() {
    let module_bytes_max_params =
        wasm_utils::make_n_arg_call_bytes(DEFAULT_MAX_PARAMETER_COUNT as usize, "i32")
            .expect("should create wasm bytes");

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let exec_request = ExecuteRequestBuilder::module_bytes(
        *DEFAULT_ACCOUNT_ADDR,
        module_bytes_max_params,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request).expect_success().commit();

    let module_bytes_100_params =
        wasm_utils::make_n_arg_call_bytes(100, "i32").expect("should create wasm bytes");

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let exec_request = ExecuteRequestBuilder::module_bytes(
        *DEFAULT_ACCOUNT_ADDR,
        module_bytes_100_params,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request).expect_success().commit();
}

#[ignore]
#[test]
fn should_not_allow_too_many_params() {
    let module_bytes = wasm_utils::make_n_arg_call_bytes(FAILING_PARAMS_COUNT, "i32")
        .expect("should create wasm bytes");

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let exec_request = ExecuteRequestBuilder::module_bytes(
        *DEFAULT_ACCOUNT_ADDR,
        module_bytes,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request).expect_failure().commit();

    let error = builder.get_error().expect("should fail");

    assert!(
        matches!(
            error,
            engine_state::Error::WasmPreprocessing(PreprocessingError::WasmValidation(WasmValidationError::TooManyParameters { max, actual }))
            if max == DEFAULT_MAX_PARAMETER_COUNT && actual == FAILING_PARAMS_COUNT
        ),
        "{:?}",
        error,
    );
}

#[ignore]
#[test]
fn should_not_allow_to_import_gas_function() {
    let module_bytes = wat::parse_str(
        r#"(module
            (func $gas (import "env" "gas") (param i32))
            (memory $0 1)
        )"#,
    )
    .unwrap();

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let exec_request = ExecuteRequestBuilder::module_bytes(
        *DEFAULT_ACCOUNT_ADDR,
        module_bytes,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request).expect_failure().commit();

    let error = builder.get_error().expect("should fail");

    assert!(
        matches!(
            error,
            engine_state::Error::WasmPreprocessing(PreprocessingError::WasmValidation(
                WasmValidationError::MissingHostFunction
            ))
        ),
        "{:?}",
        error,
    );
}

#[ignore]
#[test]
fn should_not_get_non_existing_global() {
    let get_undeclared_global = r#"(module
        (memory $memory 16)
        (export "call" (func $call_fn))
        (func $call_fn
            global.get 0
            drop
        )
    )"#;

    test_non_existing_global(get_undeclared_global, 0);
}

#[ignore]
#[test]
fn should_not_get_global_above_declared_range() {
    let get_undeclared_global = r#"(module
        (memory $memory 16)
        (export "call" (func $call_fn))
        (func $call_fn
            global.get 3
            drop
        )
        (global $global0 i32 (i32.const 0))
        (global $global1 i32 (i32.const 1))
        (global $global256 i32 (i32.const 2))
    )"#;

    test_non_existing_global(get_undeclared_global, 3);
}

#[ignore]
#[test]
fn should_not_set_non_existing_global() {
    let set_undeclared_global = r#"(module
        (memory $memory 16)
        (export "call" (func $call_fn))
        (func $call_fn
            i32.const 123
            global.set 0
            drop
        )
    )"#;

    test_non_existing_global(set_undeclared_global, 0);
}

#[ignore]
#[test]
fn should_not_set_non_existing_global_u32_max() {
    let set_undeclared_global = format!(
        r#"(module
        (memory $memory 16)
        (export "call" (func $call_fn))
        (func $call_fn
            i32.const 0
            global.set {index}
        )
        (global $global0 (mut i32) (i32.const 0))
        (global $global1 (mut i32) (i32.const 1))
        (global $global256 (mut i32) (i32.const 2))
    )"#,
        index = u32::MAX
    );

    test_non_existing_global(&set_undeclared_global, u32::MAX);
}

#[ignore]
#[test]
fn should_not_get_non_existing_global_u32_max() {
    let set_undeclared_global = format!(
        r#"(module
        (memory $memory 16)
        (export "call" (func $call_fn))
        (func $call_fn
            global.get {index}
            drop
        )
        (global $global0 (mut i32) (i32.const 0))
    )"#,
        index = u32::MAX
    );

    test_non_existing_global(&set_undeclared_global, u32::MAX);
}

#[ignore]
#[test]
fn should_not_set_non_existing_global_above_declared_range() {
    let set_undeclared_global = r#"(module
        (memory $memory 16)
        (export "call" (func $call_fn))
        (func $call_fn
            i32.const 0
            global.set 123
        )
        (global $global0 (mut i32) (i32.const 0))
        (global $global1 (mut i32) (i32.const 1))
        (global $global256 (mut i32) (i32.const 2))
    )"#;

    test_non_existing_global(set_undeclared_global, 123);
}

fn test_non_existing_global(module_wat: &str, index: u32) {
    let module_bytes = wat::parse_str(module_wat).unwrap();
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());
    let exec_request = ExecuteRequestBuilder::module_bytes(
        *DEFAULT_ACCOUNT_ADDR,
        module_bytes,
        RuntimeArgs::default(),
    )
    .build();
    builder.exec(exec_request).expect_failure().commit();
    let error = builder.get_error().expect("should fail");
    assert!(
        matches!(
            error,
            engine_state::Error::WasmPreprocessing(PreprocessingError::WasmValidation(WasmValidationError::IncorrectGlobalOperation { index: incorrect_index }))
            if incorrect_index == index
        ),
        "{:?}",
        error,
    );
}
