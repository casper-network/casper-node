use crate::{common, ARGS};

const FILENAME: &str = ".travis.yml";
const CONTENTS: &str = r#"language: rust
script:
  - make prepare
  - make check-lint
  - make test
"#;

pub fn create() {
    common::write_file(ARGS.root_path().join(FILENAME), CONTENTS);
}
