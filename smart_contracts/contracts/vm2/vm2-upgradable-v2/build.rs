fn main() {
    // Check if target arch is wasm32 and set link flags accordingly
    if std::env::var("TARGET").unwrap() == "wasm32-unknown-unknown" {
        println!("cargo:rustc-link-arg=--import-memory");
        println!("cargo:rustc-link-arg=--export-table");
    }
}
