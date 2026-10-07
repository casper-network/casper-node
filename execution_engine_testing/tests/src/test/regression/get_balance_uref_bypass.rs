use std::fmt::Write;

use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR, DEFAULT_PROPOSER_ADDR,
    LOCAL_GENESIS_REQUEST,
};
use casper_types::{bytesrepr::ToBytes, RuntimeArgs, URef};

fn wat_bytes(bytes: &[u8]) -> String {
    let mut escaped = String::new();
    for byte in bytes {
        write!(&mut escaped, "\\{byte:02x}").expect("must write byte escape");
    }
    escaped
}

/// Builds a WAT session that imports `casper_get_balance` directly and traps if the host call
/// returns success. The host function must reject a purse URef that was not granted to the
/// caller.
fn raw_get_balance_with_unknown_purse_wasm(purse: URef) -> Vec<u8> {
    let purse_bytes = purse.to_bytes().expect("purse URef must serialize");
    let purse_len = purse_bytes.len();
    let purse_data = wat_bytes(&purse_bytes);

    let module = format!(
        r#"
        (module
            (import "env" "casper_get_balance"
                (func $get_balance (param i32 i32 i32) (result i32)))
            (memory (export "memory") 1)
            (data (i32.const 0) "{purse_data}")
            (func (export "call")
                i32.const 0
                i32.const {purse_len}
                i32.const 64
                call $get_balance
                i32.eqz
                if
                    unreachable
                end
            )
        )
        "#
    );

    wat::parse_str(module).expect("wat must parse")
}

/// Regression for audit-confirmed-09: `casper_get_balance` must validate the caller's
/// access-rights for the supplied purse URef before reading its balance.
#[ignore]
#[test]
fn should_reject_raw_get_balance_with_unknown_purse_uref() {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let proposer = builder
        .get_entity_by_account_hash(*DEFAULT_PROPOSER_ADDR)
        .expect("proposer account must exist");
    let proposer_main_purse = proposer.main_purse();

    let module_bytes = raw_get_balance_with_unknown_purse_wasm(proposer_main_purse);
    let exec_request = ExecuteRequestBuilder::module_bytes(
        *DEFAULT_ACCOUNT_ADDR,
        module_bytes,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request).commit();
    let error_message = builder
        .get_error_message()
        .expect("unknown purse URef must be rejected");
    assert!(
        error_message.contains("Forged reference"),
        "unexpected error: {error_message}"
    );
}
