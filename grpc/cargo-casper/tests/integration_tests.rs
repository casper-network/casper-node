use std::process::Output;

use assert_cmd::Command;
use lazy_static::lazy_static;
use tempdir::TempDir;

const FAILURE_EXIT_CODE: i32 = 101;
const SUCCESS_EXIT_CODE: i32 = 0;
const USE_SYSTEM_CONTRACTS: &str = "--use-system-contracts";
const TURBO: &str = "turbo";

lazy_static! {
    static ref WORKSPACE_PATH_ARG: String =
        format!("--workspace-path={}/../../", env!("CARGO_MANIFEST_DIR"));
    static ref TEST_DIR: TempDir = TempDir::new("cargo-casper").unwrap();
}

#[test]
fn should_fail_when_target_path_already_exists() {
    let output_error = Command::cargo_bin(env!("CARGO_PKG_NAME"))
        .unwrap()
        .arg(TEST_DIR.path())
        .unwrap_err();

    let exit_code = output_error.as_output().unwrap().status.code().unwrap();
    assert_eq!(FAILURE_EXIT_CODE, exit_code);

    let stderr: String = String::from_utf8_lossy(&output_error.as_output().unwrap().stderr).into();
    let expected_msg_fragment = format!(
        ": destination '{}' already exists",
        TEST_DIR.path().display()
    );
    assert!(stderr.contains(&expected_msg_fragment));
    assert!(stderr.contains("error"));
}

/// Runs `cmd` and returns the `Output` if successful, or panics on failure.
fn output_from_command(mut command: Command) -> Output {
    match command.ok() {
        Ok(output) => output,
        Err(error) => {
            panic!(
                "\nFailed to execute {:?}\n===== stderr begin =====\n{}\n===== stderr end =====\n",
                command,
                String::from_utf8_lossy(&error.as_output().unwrap().stderr)
            );
        }
    }
}

fn run_tool_and_resulting_tests(turbo: bool) {
    // Run 'cargo-casper <test dir>/<subdir> --workspace-path=<path to casper-node root>'
    let subdir = if turbo { TURBO } else { USE_SYSTEM_CONTRACTS };
    let test_dir = TEST_DIR.path().join(subdir);
    let mut tool_cmd = Command::cargo_bin(env!("CARGO_PKG_NAME")).unwrap();
    tool_cmd.arg(&test_dir);
    tool_cmd.arg(&*WORKSPACE_PATH_ARG);
    // Also pass '--use-system-contracts' for non-turbo mode
    if !turbo {
        tool_cmd.arg(&*USE_SYSTEM_CONTRACTS);
    }
    // The CI environment doesn't have a Git user configured, so we can set the env var `USER` for
    // use by 'cargo new' which is called as a subprocess of 'cargo-casper'.
    tool_cmd.env("USER", "tester");
    let tool_output = output_from_command(tool_cmd);
    assert_eq!(SUCCESS_EXIT_CODE, tool_output.status.code().unwrap());

    // Run 'cargo test' in the 'tests' folder of the generated project.  This builds the Wasm
    // contract as well as the tests.
    let mut test_cmd = Command::new(env!("CARGO"));
    test_cmd.arg("test").current_dir(test_dir.join("tests"));
    let test_output = output_from_command(test_cmd);
    assert_eq!(SUCCESS_EXIT_CODE, test_output.status.code().unwrap());
}

#[test]
fn should_succeed_without_using_system_contracts() {
    run_tool_and_resulting_tests(true);
}

#[test]
fn should_succeed_using_system_contracts() {
    run_tool_and_resulting_tests(false);
}
