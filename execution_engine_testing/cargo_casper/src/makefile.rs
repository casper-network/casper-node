use crate::{common, erc20, simple, ProjectKind, ARGS};

const FILENAME: &str = "Makefile";

pub fn create() {
    let contents = match ARGS.project_kind() {
        ProjectKind::Simple => simple::MAKEFILE_CONTENTS,
        ProjectKind::Erc20 => erc20::MAKEFILE_CONTENTS,
    };
    common::write_file(ARGS.root_path().join(FILENAME), contents);
}
