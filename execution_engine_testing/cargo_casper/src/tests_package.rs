//! Consts and functions used to generate the files comprising the "tests" package when running the
//! tool.

use std::path::PathBuf;

use once_cell::sync::Lazy;

use crate::{
    common::{self, PATCH_SECTION},
    simple, ARGS,
};

const PACKAGE_NAME: &str = "tests";

static CONTRACT_PACKAGE_ROOT: Lazy<PathBuf> = Lazy::new(|| ARGS.root_path().join(PACKAGE_NAME));
static CARGO_TOML: Lazy<PathBuf> = Lazy::new(|| CONTRACT_PACKAGE_ROOT.join("Cargo.toml"));
static INTEGRATION_TESTS_RS: Lazy<PathBuf> =
    Lazy::new(|| CONTRACT_PACKAGE_ROOT.join("src/integration_tests.rs"));
static _TEST_FIXTURE_RS: Lazy<PathBuf> =
    Lazy::new(|| CONTRACT_PACKAGE_ROOT.join("src/test_fixture.rs"));

static CARGO_TOML_CONTENTS: Lazy<String> = Lazy::new(|| {
    format!(
        r#"[package]
name = "tests"
version = "0.1.0"
edition = "2018"

[dev-dependencies]
{}
[[bin]]
name = "integration-tests"
path = "src/integration_tests.rs"
bench = false
doctest = false

{}"#,
        &*simple::TEST_DEPENDENCIES,
        &*PATCH_SECTION
    )
});

pub fn create() {
    // Create "tests/src" folder and write test files inside.
    let tests_folder = INTEGRATION_TESTS_RS.parent().expect("should have parent");
    common::create_dir_all(tests_folder);

    // Write "tests/integration_tests.rs".
    common::write_file(
        &*INTEGRATION_TESTS_RS,
        &*simple::INTEGRATION_TESTS_RS_CONTENTS,
    );

    // Write "tests/Cargo.toml".
    common::write_file(&*CARGO_TOML, &*CARGO_TOML_CONTENTS);
}
