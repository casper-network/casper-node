use std::{env, fs};

use vergen::ConstantsFlags;

const SCHEMAS_DIR: &str = "src/capnp/schemas";

fn main() {
    let mut flags = ConstantsFlags::empty();
    flags.toggle(ConstantsFlags::SHA_SHORT);
    flags.toggle(ConstantsFlags::REBUILD_ON_HEAD_CHANGE);
    vergen::generate_cargo_keys(flags).expect("should generate the cargo keys");

    // Make the build profile available to rustc at compile time.
    println!(
        "cargo:rustc-env=NODE_BUILD_PROFILE={}",
        env::var("PROFILE").unwrap()
    );

    // Compile capnp schemas
    let mut compiler = ::capnpc::CompilerCommand::new();

    let entries =
        fs::read_dir(SCHEMAS_DIR).expect(&format!("unable to access schema dir: {}", SCHEMAS_DIR));

    for entry in entries {
        match &entry {
            Ok(entry) => match entry.path().extension() {
                Some(extension) => {
                    if extension == "capnp" {
                        compiler.file(entry.path());
                    }
                }
                None => (),
            },
            Err(err) => panic!("error accessing 'capnp' schema: {:?} {}", entry, err),
        }
    }

    compiler.run().expect("unable to compile 'capnp' schemas")
}
