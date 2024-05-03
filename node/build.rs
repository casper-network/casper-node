use std::{env, process::Command};

const NODE_BUILD_PROFILE_ENV_VAR: &str = "NODE_BUILD_PROFILE";

const DEFAULT_BUILD_PROFILE: &str = "PROFILE";

///
/// `casper-node` build script to capture the git revision hash and export it to cargo to include
/// it in the version information
///
/// Notes: This script exports information to cargo via println! with the old invocation prefix of
/// `cargo:`, if/when the node uses a Rust version `1.77` or above, this should be changed to
/// `cargo::` as the prefix changed in that version of rust
fn main() {
    match Command::new("git")
        .arg("rev-parse")
        .arg("--short")
        .arg("HEAD")
        .output()
    {
        Ok(output) => {
            //In the event the git command is successful, export the properly formatted git hash to
            // cargo at compile time.
            let git_hash_raw =
                String::from_utf8(output.stdout).expect("Failed to obtain commit hash to string");
            let git_hash = git_hash_raw.trim_end_matches('\n');

            println!(
                "cargo:rustc-env={}={}",
                NODE_BUILD_PROFILE_ENV_VAR, git_hash
            );
        }

        Err(error) => {
            println!("cargo:warning={}", error);
            println!("cargo:warning=casper-node build version will not include git short hash");
            // In the event of an error, export the error, and export a default build profile to
            // cargo at compile time.
            println!(
                "cargo:rustc-env={}={}",
                NODE_BUILD_PROFILE_ENV_VAR,
                env::var(DEFAULT_BUILD_PROFILE).unwrap()
            );
        }
    }
}
