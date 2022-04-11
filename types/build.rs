use std::fs;

const SCHEMAS_DIR: &str = "schemas/capnp/";

fn main() {
    let mut compiler = ::capnpc::CompilerCommand::new();

    let entries =
        fs::read_dir(SCHEMAS_DIR).expect(&format!("unable to access schema dir: {}", SCHEMAS_DIR));

    for entry in entries {
        match &entry {
            Ok(entry) => {
                compiler.file(entry.path());
            }
            Err(err) => panic!("error accessing 'capnp' schema: {:?} {}", entry, err),
        }
    }

    compiler.run().expect("unable to compile 'capnp' schemas")
}
