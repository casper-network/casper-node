//! Consts and functions used to generate the files comprising the "contract" package when running
//! the tool.

use std::path::PathBuf;

use once_cell::sync::Lazy;

use crate::{
    common::{self, PATCH_SECTION},
    simple, ARGS,
};

static PACKAGE_NAME: Lazy<&'static str> = Lazy::new(|| "contract");
static CONTRACT_PACKAGE_ROOT: Lazy<PathBuf> =
    Lazy::new(|| ARGS.root_path().join(PACKAGE_NAME.replace("-", "_")));
static CARGO_TOML: Lazy<PathBuf> = Lazy::new(|| CONTRACT_PACKAGE_ROOT.join("Cargo.toml"));
static MAIN_RS: Lazy<PathBuf> = Lazy::new(|| CONTRACT_PACKAGE_ROOT.join("src/main.rs"));
static CONFIG_TOML: Lazy<PathBuf> = Lazy::new(|| CONTRACT_PACKAGE_ROOT.join(".cargo/config.toml"));

const CONFIG_TOML_CONTENTS: &str = r#"[build]
target = "wasm32-unknown-unknown"
"#;

static CARGO_TOML_CONTENTS: Lazy<String> = Lazy::new(|| {
    format!(
        r#"[package]
name = "{}"
version = "0.1.0"
edition = "2018"

[dependencies]
{}
[[bin]]
name = "{}"
path = "src/main.rs"
bench = false
doctest = false
test = false

[profile.release]
codegen-units = 1
lto = true

{}"#,
        *PACKAGE_NAME,
        &*simple::CONTRACT_DEPENDENCIES,
        PACKAGE_NAME.replace("-", "_"),
        &*PATCH_SECTION
    )
});

pub fn create() {
    // Create "<PACKAGE_NAME>/src" folder and write "main.rs" inside.
    let src_folder = MAIN_RS.parent().expect("should have parent");
    common::create_dir_all(src_folder);

    common::write_file(&*MAIN_RS, simple::MAIN_RS_CONTENTS);

    // Create "<PACKAGE_NAME>/.cargo" folder and write "config.toml" inside.
    let config_folder = CONFIG_TOML.parent().expect("should have parent");
    common::create_dir_all(config_folder);
    common::write_file(&*CONFIG_TOML, CONFIG_TOML_CONTENTS);

    // Write "<PACKAGE_NAME>/Cargo.toml".
    common::write_file(&*CARGO_TOML, &*CARGO_TOML_CONTENTS);
}
