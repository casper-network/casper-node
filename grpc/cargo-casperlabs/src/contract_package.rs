//! Consts and functions used to generate the files comprising the "contract" package when running
//! the tool.

use std::path::PathBuf;

use lazy_static::lazy_static;

use crate::{
    common::{self, CL_CONTRACT, CL_TYPES},
    ARGS, TOOLCHAIN,
};

const PACKAGE_NAME: &str = "contract";

const MAIN_RS_CONTENTS: &str = r#"#![cfg_attr(
    not(target_arch = "wasm32"),
    crate_type = "target arch should be wasm32"
)]
#![no_main]

use casperlabs_contract::{
    contract_api::{runtime, storage},
};
use casperlabs_types::{Key, URef};

const KEY: &str = "special_value";
const ARG_MESSAGE: &str = "message";

fn store(value: String) {
    // Store `value` under a new unforgeable reference.
    let value_ref: URef = storage::new_uref(value);

    // Wrap the unforgeable reference in a value of type `Key`.
    let value_key: Key = value_ref.into();

    // Store this key under the name "special_value" in context-local storage.
    runtime::put_key(KEY, value_key);
}

// All session code must have a `call` entrypoint.
#[no_mangle]
pub extern "C" fn call() {
    // Get the optional first argument supplied to the argument.
    let value: String = runtime::get_named_arg(ARG_MESSAGE);
    store(value);
}
"#;

const CONFIG_TOML_CONTENTS: &str = r#"[build]
target = "wasm32-unknown-unknown"
"#;

lazy_static! {
    static ref CARGO_TOML: PathBuf = ARGS.root_path().join(PACKAGE_NAME).join("Cargo.toml");
    static ref RUST_TOOLCHAIN: PathBuf = ARGS.root_path().join(PACKAGE_NAME).join("rust-toolchain");
    static ref MAIN_RS: PathBuf = ARGS.root_path().join(PACKAGE_NAME).join("src/main.rs");
    static ref CONFIG_TOML: PathBuf = ARGS
        .root_path()
        .join(PACKAGE_NAME)
        .join(".cargo/config.toml");
    static ref CARGO_TOML_ADDITIONAL_CONTENTS: String = format!(
        r#"{}
{}

[[bin]]
name = "{}"
path = "src/main.rs"
bench = false
doctest = false
test = false

[features]
default = ["casperlabs-contract/std", "casperlabs-types/std", "casperlabs-contract/test-support"]

[profile.release]
lto = true
"#,
        *CL_CONTRACT, *CL_TYPES, PACKAGE_NAME
    );
}

pub fn run_cargo_new() {
    common::run_cargo_new(PACKAGE_NAME);
}

pub fn update_cargo_toml() {
    common::append_to_file(&*CARGO_TOML, &*CARGO_TOML_ADDITIONAL_CONTENTS);
}

pub fn add_rust_toolchain() {
    common::write_file(&*RUST_TOOLCHAIN, format!("{}\n", TOOLCHAIN));
}

pub fn update_main_rs() {
    common::write_file(&*MAIN_RS, MAIN_RS_CONTENTS);
}

pub fn add_config_toml() {
    let folder = CONFIG_TOML.parent().expect("should have parent");
    common::create_dir_all(folder);
    common::write_file(&*CONFIG_TOML, CONFIG_TOML_CONTENTS);
}
