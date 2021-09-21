use std::{fs, process::Output};

use assert_cmd::Command;
use once_cell::sync::Lazy;

const FAILURE_EXIT_CODE: i32 = 101;
const SUCCESS_EXIT_CODE: i32 = 0;
const TEST_PATH: &str = "test";

static WORKSPACE_PATH_ARG: Lazy<String> =
    Lazy::new(|| format!("--workspace-path={}/../../", env!("CARGO_MANIFEST_DIR")));

#[test]
fn should_fail_when_target_path_already_exists() {
    let test_dir = tempfile::tempdir().unwrap().into_path();
    let output_error = Command::cargo_bin(env!("CARGO_PKG_NAME"))
        .unwrap()
        .arg(&test_dir)
        .unwrap_err();

    let exit_code = output_error.as_output().unwrap().status.code().unwrap();
    assert_eq!(FAILURE_EXIT_CODE, exit_code);

    let stderr: String = String::from_utf8_lossy(&output_error.as_output().unwrap().stderr).into();
    let expected_msg_fragment = format!(": destination '{}' already exists", test_dir.display());
    assert!(stderr.contains(&expected_msg_fragment));
    assert!(stderr.contains("error"));

    fs::remove_dir_all(&test_dir).unwrap();
}

/// Runs `cmd` and returns the `Output` if successful, or panics on failure.
fn output_from_command(mut command: Command) -> Output {
    match command.ok() {
        Ok(output) => output,
        Err(error) => {
            panic!(
                "\nFailed to execute {:?}\n===== stderr begin =====\n{}\n===== stderr end \
                =====\n===== stdout begin =====\n{}\n===== stdout end =====\n",
                command,
                String::from_utf8_lossy(&error.as_output().unwrap().stderr),
                String::from_utf8_lossy(&error.as_output().unwrap().stdout)
            );
        }
    }
}

fn run_tool_and_resulting_tests(maybe_extra_flag: Option<&str>) {
    let temp_dir = tempfile::tempdir().unwrap().into_path();

    // Run 'cargo-casper <test dir>/<subdir> --workspace-path=<path to casper-node root>'
    let subdir = TEST_PATH;
    let test_dir = temp_dir.join(subdir);
    let mut tool_cmd = Command::cargo_bin(env!("CARGO_PKG_NAME")).unwrap();
    tool_cmd.arg(&test_dir);
    if let Some(extra_flag) = maybe_extra_flag {
        tool_cmd.arg(extra_flag);
    }
    tool_cmd.arg(&*WORKSPACE_PATH_ARG);

    // The CI environment doesn't have a Git user configured, so we can set the env var `USER` for
    // use by 'cargo new' which is called as a subprocess of 'cargo-casper'.
    tool_cmd.env("USER", "tester");
    let tool_output = output_from_command(tool_cmd);
    assert_eq!(SUCCESS_EXIT_CODE, tool_output.status.code().unwrap());

    // Run 'make test' in the root of the generated project.  This builds the Wasm contract as well
    // as the tests.  This requires the use of a nightly version of Rust, so we use rustup to
    // execute the appropriate cargo version.
    let mut test_cmd = Command::new("rustup");
    let nightly_version = fs::read_to_string(format!(
        "{}/../../smart_contracts/rust-toolchain",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    test_cmd
        .arg("run")
        .arg(nightly_version.trim())
        .arg("make")
        .arg("test")
        .current_dir(test_dir);

    let test_output = output_from_command(test_cmd);
    assert_eq!(SUCCESS_EXIT_CODE, test_output.status.code().unwrap());

    // Cleans up temporary directory, but leaves it otherwise if the test failed.
    fs::remove_dir_all(&temp_dir).unwrap();
}

#[test]
fn should_run_cargo_casper_for_simple_example() {
    run_tool_and_resulting_tests(None);
}

#[test]
fn should_run_cargo_casper_for_erc20_example() {
    run_tool_and_resulting_tests(Some("--erc20"));
}
