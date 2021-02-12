use crate::{common, ARGS};

const FILENAME: &str = ".travis.yml";
const CONTENTS: &str = r#"language: rust
script:
  - cd tests && cargo build
  - cd tests && cargo test
"#;

pub fn create() {
    common::write_file(ARGS.root_path().join(FILENAME), CONTENTS);
}
