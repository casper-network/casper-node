use crate::{common, simple, ARGS};

const FILENAME: &str = "Makefile";

pub fn create() {
    common::write_file(ARGS.root_path().join(FILENAME), simple::MAKEFILE_CONTENTS);
}
