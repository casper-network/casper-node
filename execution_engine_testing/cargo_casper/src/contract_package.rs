//! Consts and functions used to generate the files comprising the "contract" package when running
//! the tool.

use std::path::PathBuf;

use once_cell::sync::Lazy;

use crate::{
    common::{self, CL_CONTRACT, CL_TYPES, PATCH_SECTION},
    ARGS,
};

const PACKAGE_NAME: &str = "contract";
#[allow(clippy::single_char_pattern)]
static CONTRACT_PACKAGE_ROOT: Lazy<PathBuf> =
    Lazy::new(|| ARGS.root_path().join(PACKAGE_NAME.replace("-", "_")));
static CARGO_TOML: Lazy<PathBuf> = Lazy::new(|| CONTRACT_PACKAGE_ROOT.join("Cargo.toml"));
static MAIN_RS: Lazy<PathBuf> = Lazy::new(|| CONTRACT_PACKAGE_ROOT.join("src/main.rs"));
static CONFIG_TOML: Lazy<PathBuf> = Lazy::new(|| CONTRACT_PACKAGE_ROOT.join(".cargo/config.toml"));

static CONTRACT_DEPENDENCIES: Lazy<String> = Lazy::new(|| {
    format!(
        "{}{}",
        CL_CONTRACT.display_with_features(true, vec![]),
        CL_TYPES.display_with_features(true, vec![]),
    )
});

const CONFIG_TOML_CONTENTS: &str = r#"[build]
target = "wasm32-unknown-unknown"
"#;

#[allow(clippy::single_char_pattern)]
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
        PACKAGE_NAME,
        &*CONTRACT_DEPENDENCIES,
        PACKAGE_NAME.replace("-", "_"),
        &*PATCH_SECTION
    )
});

const MAIN_RS_CONTENTS: &str = include_str!("../resources/main.rs.in");

pub fn create() {
    // Create "<PACKAGE_NAME>/src" folder and write "main.rs" inside.
    let src_folder = MAIN_RS.parent().expect("should have parent");
    common::create_dir_all(src_folder);

    common::write_file(&*MAIN_RS, MAIN_RS_CONTENTS);

    // Create "<PACKAGE_NAME>/.cargo" folder and write "config.toml" inside.
    let config_folder = CONFIG_TOML.parent().expect("should have parent");
    common::create_dir_all(config_folder);
    common::write_file(&*CONFIG_TOML, CONFIG_TOML_CONTENTS);

    // Write "<PACKAGE_NAME>/Cargo.toml".
    common::write_file(&*CARGO_TOML, &*CARGO_TOML_CONTENTS);
}
