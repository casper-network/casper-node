//! Consts and functions used to generate the files comprising the "tests" package when running the
//! tool.

use std::path::{Path, PathBuf};

use lazy_static::lazy_static;

use crate::{
    common::{self, CL_CONTRACT, CL_TYPES},
    dependency::Dependency,
    ARGS, TOOLCHAIN,
};

const PACKAGE_NAME: &str = "tests";
const MINT_INSTALL: &str = "mint_install.wasm";
const POS_INSTALL: &str = "pos_install.wasm";
const STANDARD_PAYMENT: &str = "standard_payment.wasm";
const STANDARD_PAYMENT_INSTALL: &str = "standard_payment_install.wasm";
const AUCTION_INSTALL: &str = "auction_install.wasm";

const INTEGRATION_TESTS_RS_CONTENTS: &str = r#"#[cfg(test)]
mod tests {
    use casper_engine_test_support::{
        Code, Error, SessionBuilder, TestContextBuilder, Value,
    };
    use casper_types::{runtime_args, RuntimeArgs, U512, account::AccountHash, PublicKey};

    const MY_ACCOUNT: [u8; 32] = [7u8; 32];
    const MY_ADDR: [u8; 32] = [8u8; 32];
    // define KEY constant to match that in the contract
    const KEY: &str = "special_value";
    const VALUE: &str = "hello world";
    const ARG_MESSAGE: &str = "message";

    #[test]
    fn should_store_hello_world() {
        let public_key = PublicKey::Ed25519(MY_ACCOUNT);
        let account_addr = AccountHash::new(MY_ADDR);

        let mut context = TestContextBuilder::new()
            .with_public_key(public_key, account_addr, U512::from(128_000_000_000_000u64))
            .build();

        // The test framework checks for compiled Wasm files in '<current working dir>/wasm'.  Paths
        // relative to the current working dir (e.g. 'wasm/contract.wasm') can also be used, as can
        // absolute paths.
        let session_code = Code::from("contract.wasm");
        let session_args = runtime_args! {
            ARG_MESSAGE => VALUE,
        };
        let session = SessionBuilder::new(session_code, session_args)
            .with_address(account_addr)
            .with_authorization_keys(&[account_addr])
            .build();

        let result_of_query: Result<Value, Error> =
            context.run(session).query(account_addr, &[KEY]);

        let returned_value = result_of_query.expect("should be a value");

        let expected_value = Value::from_t(VALUE.to_string()).expect("should construct Value");
        assert_eq!(expected_value, returned_value);
    }
}

fn main() {
    panic!("Execute \"cargo test\" to test the contract, not \"cargo run\".");
}
"#;

const BUILD_RS_CONTENTS: &str = r#"use std::{env, fs, path::PathBuf, process::Command};

const CONTRACT_ROOT: &str = "../contract";
const CONTRACT_CARGO_TOML: &str = "../contract/Cargo.toml";
const CONTRACT_MAIN_RS: &str = "../contract/src/main.rs";
const BUILD_ARGS: [&str; 2] = ["build", "--release"];
const WASM_FILENAME: &str = "contract.wasm";
const ORIGINAL_WASM_DIR: &str = "../contract/target/wasm32-unknown-unknown/release";
const NEW_WASM_DIR: &str = "wasm";

fn main() {
    // Watch contract source files for changes.
    println!("cargo:rerun-if-changed={}", CONTRACT_CARGO_TOML);
    println!("cargo:rerun-if-changed={}", CONTRACT_MAIN_RS);

    // Build the contract.
    let output = Command::new("cargo")
        .current_dir(CONTRACT_ROOT)
        .args(&BUILD_ARGS)
        .output()
        .expect("Expected to build Wasm contracts");
    assert!(
        output.status.success(),
        "Failed to build Wasm contracts:\n{:?}",
        output
    );

    // Move the compiled Wasm file to our own build folder ("wasm/contract.wasm").
    let new_wasm_dir = env::current_dir().unwrap().join(NEW_WASM_DIR);
    let _ = fs::create_dir(&new_wasm_dir);

    let original_wasm_file = PathBuf::from(ORIGINAL_WASM_DIR).join(WASM_FILENAME);
    let copied_wasm_file = new_wasm_dir.join(WASM_FILENAME);
    fs::copy(original_wasm_file, copied_wasm_file).unwrap();
}
"#;

lazy_static! {
    static ref CARGO_TOML: PathBuf = ARGS.root_path().join(PACKAGE_NAME).join("Cargo.toml");
    static ref RUST_TOOLCHAIN: PathBuf = ARGS.root_path().join(PACKAGE_NAME).join("rust-toolchain");
    static ref BUILD_RS: PathBuf = ARGS.root_path().join(PACKAGE_NAME).join("build.rs");
    static ref MAIN_RS: PathBuf = ARGS.root_path().join(PACKAGE_NAME).join("src/main.rs");
    static ref INTEGRATION_TESTS_RS: PathBuf = ARGS
        .root_path()
        .join(PACKAGE_NAME)
        .join("src/integration_tests.rs");
    static ref ENGINE_TEST_SUPPORT: Dependency =
        Dependency::new("casper-engine-test-support", "0.1.0", "grpc/test_support");
    static ref CARGO_TOML_ADDITIONAL_CONTENTS: String = format!(
        r#"
[dev-dependencies]
{}
{}
{}

[[bin]]
name = "integration-tests"
path = "src/integration_tests.rs"

[features]
default = ["casper-contract/std", "casper-types/std", "casper-engine-test-support/test-support", "casper-contract/test-support""#,
        *CL_CONTRACT, *CL_TYPES, *ENGINE_TEST_SUPPORT,
    );
    static ref WASM_SRC_DIR: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("wasm");
    static ref WASM_DEST_DIR: PathBuf = ARGS.root_path().join(PACKAGE_NAME).join("wasm");
}

pub fn run_cargo_new() {
    common::run_cargo_new(PACKAGE_NAME);
}

pub fn update_cargo_toml(use_system_contracts: bool) {
    let cargo_toml_additional_contents = format!(
        "{}{}\n",
        &*CARGO_TOML_ADDITIONAL_CONTENTS,
        if use_system_contracts {
            ", \"casper-engine-test-support/use-system-contracts\"]"
        } else {
            "]"
        }
    );
    common::append_to_file(&*CARGO_TOML, cargo_toml_additional_contents);
}

pub fn add_rust_toolchain() {
    common::write_file(&*RUST_TOOLCHAIN, format!("{}\n", TOOLCHAIN));
}

pub fn add_build_rs() {
    common::write_file(&*BUILD_RS, BUILD_RS_CONTENTS);
}

pub fn replace_main_rs() {
    common::remove_file(&*MAIN_RS);
    common::write_file(&*INTEGRATION_TESTS_RS, INTEGRATION_TESTS_RS_CONTENTS);
}

pub fn copy_wasm_files() {
    common::create_dir_all(&*WASM_DEST_DIR);
    common::copy_file(
        WASM_SRC_DIR.join(MINT_INSTALL),
        WASM_DEST_DIR.join(MINT_INSTALL),
    );
    common::copy_file(
        WASM_SRC_DIR.join(POS_INSTALL),
        WASM_DEST_DIR.join(POS_INSTALL),
    );
    common::copy_file(
        WASM_SRC_DIR.join(STANDARD_PAYMENT),
        WASM_DEST_DIR.join(STANDARD_PAYMENT),
    );
    common::copy_file(
        WASM_SRC_DIR.join(STANDARD_PAYMENT_INSTALL),
        WASM_DEST_DIR.join(STANDARD_PAYMENT_INSTALL),
    );
    common::copy_file(
        WASM_SRC_DIR.join(AUCTION_INSTALL),
        WASM_DEST_DIR.join(AUCTION_INSTALL),
    );
}

#[cfg(test)]
pub mod tests {
    use super::*;

    const ENGINE_TEST_SUPPORT_TOML_PATH: &str = "grpc/test_support/Cargo.toml";

    #[test]
    fn check_engine_test_support_version() {
        common::tests::check_package_version(&*ENGINE_TEST_SUPPORT, ENGINE_TEST_SUPPORT_TOML_PATH);
    }
}
