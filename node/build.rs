use std::env;

use vergen::{Config, ShaKind};

fn main() {
    let mut config = Config::default();
    *config.git_mut().sha_kind_mut() = ShaKind::Short;
    *config.git_mut().rerun_on_head_change_mut() = true;
    vergen::vergen(config).expect("should generate the cargo keys");

    // Make the build profile available to rustc at compile time.
    println!(
        "cargo:rustc-env=NODE_BUILD_PROFILE={}",
        env::var("PROFILE").unwrap()
    );
}
