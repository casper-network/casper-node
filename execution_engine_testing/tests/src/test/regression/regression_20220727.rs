use std::fmt::Write;

use parity_wasm::{
    builder,
    elements::{Instruction, Instructions},
};

use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::{
    core::{engine_state, execution},
    shared::wasm_prep::{PreprocessingError, DEFAULT_MAX_TABLE_SIZE},
};
use casper_types::{contracts::DEFAULT_ENTRY_POINT_NAME, RuntimeArgs};

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

const OOM_INIT: (u32, Option<u32>) = (2805655325, None);
const FAILURE_ONE_ABOVE_LIMIT: (u32, Option<u32>) = (DEFAULT_MAX_TABLE_SIZE + 1, None);
const FAILURE_MAX_ABOVE_LIMIT: (u32, Option<u32>) = (DEFAULT_MAX_TABLE_SIZE, Some(u32::MAX));
const FAILURE_INIT_ABOVE_LIMIT: (u32, Option<u32>) = (u32::MAX, Some(u32::MAX));
const ALLOWED_NO_MAX: (u32, Option<u32>) = (DEFAULT_MAX_TABLE_SIZE, None);
const ALLOWED_LIMITS: (u32, Option<u32>) = (DEFAULT_MAX_TABLE_SIZE, Some(DEFAULT_MAX_TABLE_SIZE));

#[ignore]
#[test]
fn should_not_oom() {
    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&*DEFAULT_RUN_GENESIS_REQUEST);

    let test_cases = vec![
        OOM_INIT,
        FAILURE_ONE_ABOVE_LIMIT,
        FAILURE_MAX_ABOVE_LIMIT,
        FAILURE_INIT_ABOVE_LIMIT,
    ];

    for (initial, maximum) in test_cases {
        let module_bytes = make_oom_payload(initial, maximum);
        let exec_request = ExecuteRequestBuilder::module_bytes(
            *DEFAULT_ACCOUNT_ADDR,
            module_bytes,
            RuntimeArgs::default(),
        )
        .build();

        builder.exec(exec_request).expect_failure().commit();

        let error = builder.get_error().unwrap();

        assert!(matches!(
            error,
            engine_state::Error::WasmPreprocessing(PreprocessingError::InvalidWasm(ref _msg))
        ))
    }
}

#[ignore]
#[test]
fn should_pass_table_validation() {
    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&*DEFAULT_RUN_GENESIS_REQUEST);

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

    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&*DEFAULT_RUN_GENESIS_REQUEST);

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
    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&*DEFAULT_RUN_GENESIS_REQUEST);

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
    let module_bytes = parity_wasm::serialize(module).expect("should serialize");

    let exec_request = ExecuteRequestBuilder::module_bytes(
        *DEFAULT_ACCOUNT_ADDR,
        module_bytes,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request).expect_failure().commit();

    let error = builder.get_error().unwrap();

    // Thankfully we don't fail at preprocess stage and wasmi actually rejects instances with more
    // than 1 table.
    assert!(
        matches!(
            error,
            engine_state::Error::Exec(execution::Error::Interpreter(ref msg))
            if msg == "too many tables in index space: 2"
        ),
        "{:?}",
        error
    );
}
