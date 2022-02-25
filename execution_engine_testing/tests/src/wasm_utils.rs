//! Wasm helpers.
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
