use std::env;

use vergen::EmitBuilder;

fn main() {
    if let Err(error) = EmitBuilder::builder().fail_on_error().git_sha(true).emit() {
        println!("cargo:warning={}", error);
        println!("cargo:warning=casper-rpc-sidecar build version will not include git short hash");
    }

    // Make the build profile available to rustc at compile time.
    println!(
        "cargo:rustc-env=SIDECAR_BUILD_PROFILE={}",
        env::var("PROFILE").unwrap()
    );
}
