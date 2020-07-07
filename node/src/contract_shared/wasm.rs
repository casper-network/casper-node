use parity_wasm::elements::Module;

use crate::contract_shared::wasm_prep::{PreprocessingError, Preprocessor};

static DO_NOTHING: &str = r#"
    (module
      (type (;0;) (func))
      (func $call (type 0))
      (table (;0;) 1 1 funcref)
      (memory (;0;) 16)
      (global (;0;) (mut i32) (i32.const 1048576))
      (global (;1;) i32 (i32.const 1048576))
      (global (;2;) i32 (i32.const 1048576))
      (export "memory" (memory 0))
      (export "call" (func $call))
      (export "__data_end" (global 1))
      (export "__heap_base" (global 2)))
    "#;

pub fn do_nothing_bytes() -> Vec<u8> {
    wabt::wat2wasm(DO_NOTHING).expect("failed to parse wat")
}

pub fn do_nothing_module(preprocessor: &Preprocessor) -> Result<Module, PreprocessingError> {
    let do_nothing_bytes = do_nothing_bytes();
    preprocessor.preprocess(&do_nothing_bytes)
}
