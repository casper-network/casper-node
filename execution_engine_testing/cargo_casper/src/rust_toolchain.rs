use crate::{common, ARGS};

const FILENAME: &str = "rust-toolchain";
pub const CONTENTS: &str = r#"nightly-2021-06-17
"#;

pub fn create() {
    common::write_file(ARGS.root_path().join(FILENAME), CONTENTS);
}

#[cfg(test)]
mod tests {
    use std::{env, fs};

    use super::CONTENTS;

    const PATH_PREFIX: &str = "/execution_engine_testing/cargo_casper";

    #[test]
    fn check_toolchain_version() {
        let mut toolchain_path = env::current_dir().unwrap().display().to_string();
        let index = toolchain_path.find(PATH_PREFIX).unwrap_or_else(|| {
            panic!(
                "test should be run from within casper-node workspace: {}",
                toolchain_path
            )
        });
        toolchain_path.replace_range(index.., "/rust-toolchain");

        let toolchain_contents =
            fs::read(&toolchain_path).unwrap_or_else(|_| panic!("should read {}", toolchain_path));
        let expected_toolchain_value = String::from_utf8_lossy(&toolchain_contents).to_string();

        // If this fails, ensure `CONTENTS` is updated to match the value in
        // "casper-node/rust-toolchain".
        assert_eq!(&*expected_toolchain_value, CONTENTS);
    }
}
