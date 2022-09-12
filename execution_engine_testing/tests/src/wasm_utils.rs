//! Wasm helpers.
use std::fmt::Write;

use parity_wasm::builder;

use casper_types::contracts::DEFAULT_ENTRY_POINT_NAME;

/// Creates minimal session code that does nothing
pub fn do_nothing_bytes() -> Vec<u8> {
    let module = builder::module()
        .function()
        // A signature with 0 params and no return type
        .signature()
        .build()
        .body()
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
    parity_wasm::serialize(module).expect("should serialize")
}

/// Creates minimal session code that contains a function with arbitrary number of parameters.
pub fn make_n_arg_call_bytes(
    arity: usize,
    arg_type: &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut call_args = String::new();
    for i in 0..arity {
        write!(call_args, "({}.const {}) ", arg_type, i)?;
    }

    let mut func_params = String::new();
    for i in 0..arity {
        write!(func_params, "(param $arg{} {}) ", i, arg_type)?;
    }

    // This wasm module contains a function with a specified amount of arguments in it.
    let wat = format!(
        r#"(module
        (func $call (call $func {call_args}) (return))
        (func $func {func_params} (return))
        (export "func" (func $func))
        (export "call" (func $call))
        (memory $memory 1)
      )"#
    );
    let module_bytes = wabt::wat2wasm(wat)?;
    Ok(module_bytes)
}
