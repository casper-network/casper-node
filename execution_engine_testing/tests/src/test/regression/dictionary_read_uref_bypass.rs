use std::fmt::Write;

use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR, LOCAL_GENESIS_REQUEST,
};
use casper_types::{
    bytesrepr::ToBytes, AccessRights, CLValue, Key, RuntimeArgs, StoredValue, URef,
};

const SECRET_UREF_NAME: &str = "no_rights_secret";
const SECRET_VALUE: &str = "dictionary_read must not read this";

fn wat_bytes(bytes: &[u8]) -> String {
    let mut escaped = String::new();
    for byte in bytes {
        write!(&mut escaped, "\\{byte:02x}").expect("must write byte escape");
    }
    escaped
}

/// Builds a WAT module that imports `casper_dictionary_read` directly and traps on success. The
/// session should never succeed because the supplied key is `Key::URef`, not `Key::Dictionary`.
fn raw_dictionary_read_with_uref_key_wasm(key: Key) -> Vec<u8> {
    let key_bytes = key.to_bytes().expect("key must serialize");
    let key_len = key_bytes.len();
    let key_data = wat_bytes(&key_bytes);
    let module = format!(
        r#"
        (module
            (import "env" "casper_dictionary_read"
                (func $dictionary_read (param i32 i32 i32) (result i32)))
            (memory (export "memory") 1)
            (data (i32.const 0) "{key_data}")
            (func (export "call")
                i32.const 0
                i32.const {key_len}
                i32.const 1024
                call $dictionary_read
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

/// Regression for audit-confirmed-05: the raw `casper_dictionary_read` host function must not
/// accept a `Key::URef` and act as a generic read primitive.
#[ignore]
#[test]
fn should_reject_raw_dictionary_read_with_non_dictionary_uref_key() {
    let secret_uref = URef::new([7; 32], AccessRights::NONE);
    let secret_key = Key::URef(secret_uref);

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let mut account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("default account must exist");
    account
        .named_keys_mut()
        .insert(SECRET_UREF_NAME.to_string(), secret_key);

    builder.write_data_and_commit(
        vec![
            (
                Key::Account(*DEFAULT_ACCOUNT_ADDR),
                StoredValue::Account(account),
            ),
            (
                secret_key,
                StoredValue::CLValue(CLValue::from_t(SECRET_VALUE).expect("must encode secret")),
            ),
        ]
        .into_iter(),
    );

    let module_bytes = raw_dictionary_read_with_uref_key_wasm(secret_key);
    let exec_request = ExecuteRequestBuilder::module_bytes(
        *DEFAULT_ACCOUNT_ADDR,
        module_bytes,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request).expect_success().commit();
}
