use std::{fs, path::Path, process, str};

use colour::e_red;
use once_cell::sync::Lazy;

use crate::{dependency::Dependency, FAILURE_EXIT_CODE};

pub static CL_CONTRACT: Lazy<Dependency> =
    Lazy::new(|| Dependency::new("casper-contract", "1.0.0", "smart_contracts/contract"));
pub static CL_TYPES: Lazy<Dependency> =
    Lazy::new(|| Dependency::new("casper-types", "1.0.0", "types"));
pub static CL_ENGINE_TEST_SUPPORT: Lazy<Dependency> = Lazy::new(|| {
    Dependency::new(
        "casper-engine-test-support",
        "1.0.0",
        "execution_engine_testing/test_support",
    )
});

pub fn print_error_and_exit(msg: &str) -> ! {
    e_red!("error");
    eprintln!("{}", msg);
    process::exit(FAILURE_EXIT_CODE)
}

pub fn create_dir_all<P: AsRef<Path>>(path: P) {
    if let Err(error) = fs::create_dir_all(path.as_ref()) {
        print_error_and_exit(&format!(
            ": failed to create '{}': {}",
            path.as_ref().display(),
            error
        ));
    }
}

pub fn write_file<P: AsRef<Path>, C: AsRef<[u8]>>(path: P, contents: C) {
    if let Err(error) = fs::write(path.as_ref(), contents) {
        print_error_and_exit(&format!(
            ": failed to write to '{}': {}",
            path.as_ref().display(),
            error
        ));
    }
}

#[cfg(test)]
pub mod tests {
    use std::{env, fs};

    use toml::Value;

    use super::*;

    const CL_CONTRACT_TOML_PATH: &str = "smart_contracts/contract/Cargo.toml";
    const CL_TYPES_TOML_PATH: &str = "types/Cargo.toml";
    const CL_ENGINE_TEST_SUPPORT_TOML_PATH: &str =
        "execution_engine_testing/test_support/Cargo.toml";
    const PACKAGE_FIELD_NAME: &str = "package";
    const VERSION_FIELD_NAME: &str = "version";
    const PATH_PREFIX: &str = "/execution_engine_testing/cargo_casper";

    /// Returns the absolute path of `relative_path` where this is relative to "casper-node".
    /// Panics if the current working directory is not within "casper-node".
    pub fn full_path_from_path_relative_to_workspace(relative_path: &str) -> String {
        let mut full_path = env::current_dir().unwrap().display().to_string();
        let index = full_path.find(PATH_PREFIX).unwrap_or_else(|| {
            panic!(
                "test should be run from within casper-node workspace: {} relative path: {}",
                full_path, relative_path,
            )
        });
        full_path.replace_range(index + 1.., relative_path);
        full_path
    }

    /// Checks the version of the package specified by the Cargo.toml at `toml_path` is equal to
    /// the hard-coded one specified in `dep.version()`.
    pub fn check_package_version(dep: &Dependency, toml_path: &str) {
        let toml_path = full_path_from_path_relative_to_workspace(toml_path);

        let raw_toml_contents =
            fs::read(&toml_path).unwrap_or_else(|_| panic!("should read {}", toml_path));
        let toml_contents = String::from_utf8_lossy(&raw_toml_contents).to_string();
        let toml = toml_contents.parse::<Value>().unwrap();

        let expected_version = toml[PACKAGE_FIELD_NAME][VERSION_FIELD_NAME]
            .as_str()
            .unwrap();
        // If this fails, ensure `dep.version()` is updated to match the value in the Cargo.toml at
        // `toml_path`.
        assert_eq!(
            expected_version,
            dep.version(),
            "\n\nEnsure local version of {:?} is updated to {} as defined in {}\n\n",
            dep,
            expected_version,
            toml_path
        );
    }

    #[test]
    fn check_cl_contract_version() {
        check_package_version(&*CL_CONTRACT, CL_CONTRACT_TOML_PATH);
    }

    #[test]
    fn check_cl_types_version() {
        check_package_version(&*CL_TYPES, CL_TYPES_TOML_PATH);
    }

    #[test]
    fn check_cl_engine_test_support_version() {
        check_package_version(&*CL_ENGINE_TEST_SUPPORT, CL_ENGINE_TEST_SUPPORT_TOML_PATH);
    }
}
