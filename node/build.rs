use std::env;

use vergen::ConstantsFlags;

fn main() {
    let mut flags = ConstantsFlags::empty();
    flags.toggle(ConstantsFlags::SHA_SHORT);
    flags.toggle(ConstantsFlags::REBUILD_ON_HEAD_CHANGE);
    vergen::generate_cargo_keys(flags).expect("should generate the cargo keys");

    // Make the build profile available to rustc at compile time.
    println!(
        "cargo:rustc-env=NODE_BUILD_PROFILE={}",
        env::var("PROFILE").expect("should have PROFILE env variable") //?
    );
}
