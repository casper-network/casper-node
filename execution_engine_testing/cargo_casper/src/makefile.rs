use crate::{common, ARGS};

const FILENAME: &str = "Makefile";
const MAKEFILE_CONTENTS: &str = include_str!("../resources/Makefile.in");

pub fn create() {
    common::write_file(ARGS.root_path().join(FILENAME), MAKEFILE_CONTENTS);
}
