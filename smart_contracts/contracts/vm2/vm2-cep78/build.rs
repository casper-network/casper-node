// use std::{env, fs, path::Path};

// use casper_sdk_codegen::Codegen;

// const SCHEMA: &str = include_str!("cep18_schema.json");

fn main() {
    // Check if target arch is wasm32 and set link flags accordingly
    if std::env::var("TARGET").unwrap() == "wasm32-unknown-unknown" {
        println!("cargo:rustc-link-arg=--import-memory");
        println!("cargo:rustc-link-arg=--export-table");
    }

    // let mut codegen = Codegen::from_str(SCHEMA).unwrap();
    // let source = codegen.gen();

    // let target_dir = env::var_os("OUT_DIR").unwrap();
    // let target_path = Path::new(&target_dir).join("cep18_schema.rs");
    // fs::write(&target_path, source).unwrap();
}
