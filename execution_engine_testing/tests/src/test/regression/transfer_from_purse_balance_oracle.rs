use std::fmt::Write;

use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_ACCOUNT_INITIAL_BALANCE, DEFAULT_PROPOSER_ADDR, LOCAL_GENESIS_REQUEST,
};
use casper_types::{account::AccountHash, bytesrepr::ToBytes, RuntimeArgs, URef, U512};

fn wat_bytes(bytes: &[u8]) -> String {
    let mut escaped = String::new();
    for byte in bytes {
        write!(&mut escaped, "\\{byte:02x}").expect("must write byte escape");
    }
    escaped
}

/// Builds a WAT session that imports `casper_transfer_from_purse_to_account` directly and traps
/// if the host function returns at all. The source URef belongs to a purse the caller does not
/// possess and the amount exceeds that purse's balance: the host must reject the unknown source
/// rather than fall through to an `InsufficientFunds` mint error that would leak the balance.
fn raw_transfer_from_unknown_purse_wasm(
    source: URef,
    target: AccountHash,
    amount: U512,
) -> Vec<u8> {
    let source_bytes = source.to_bytes().expect("source URef must serialize");
    let target_bytes = target.to_bytes().expect("target account must serialize");
    let amount_bytes = amount.to_bytes().expect("amount must serialize");
    let id_bytes = Option::<u64>::None
        .to_bytes()
        .expect("transfer id must serialize");
    let source_len = source_bytes.len();
    let target_len = target_bytes.len();
    let amount_len = amount_bytes.len();
    let id_len = id_bytes.len();
    let source_data = wat_bytes(&source_bytes);
    let target_data = wat_bytes(&target_bytes);
    let amount_data = wat_bytes(&amount_bytes);
    let id_data = wat_bytes(&id_bytes);

    let module = format!(
        r#"
        (module
            (import "env" "casper_transfer_from_purse_to_account"
                (func $transfer_from_purse_to_account
                    (param i32 i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
            (memory (export "memory") 1)
            (data (i32.const 0) "{source_data}")
            (data (i32.const 64) "{target_data}")
            (data (i32.const 128) "{amount_data}")
            (data (i32.const 192) "{id_data}")
            (func (export "call")
                i32.const 0
                i32.const {source_len}
                i32.const 64
                i32.const {target_len}
                i32.const 128
                i32.const {amount_len}
                i32.const 192
                i32.const {id_len}
                i32.const 256
                call $transfer_from_purse_to_account
                drop
                unreachable
            )
        )
        "#
    );

    wat::parse_str(module).expect("wat must parse")
}

/// Regression for audit-confirmed-10: the raw `casper_transfer_from_purse_to_account` host
/// function must reject an ungranted source URef before any mint precondition (balance) check,
/// so callers cannot use the host as a balance oracle for purses they do not possess.
#[ignore]
#[test]
fn should_reject_unknown_transfer_source_before_checking_balance() {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let proposer = builder
        .get_entity_by_account_hash(*DEFAULT_PROPOSER_ADDR)
        .expect("proposer account must exist");
    let proposer_main_purse = proposer.main_purse();
    let target = AccountHash::new([99; 32]);
    let amount = U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE) + U512::one();

    let module_bytes = raw_transfer_from_unknown_purse_wasm(proposer_main_purse, target, amount);
    let exec_request = ExecuteRequestBuilder::module_bytes(
        *DEFAULT_ACCOUNT_ADDR,
        module_bytes,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request).commit();
    let error_message = builder
        .get_error_message()
        .expect("unknown source URef must be rejected");
    assert!(
        error_message.contains("Forged reference"),
        "unexpected error: {error_message}"
    );
}
