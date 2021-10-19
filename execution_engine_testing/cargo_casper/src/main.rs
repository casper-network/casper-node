//! Command line tool for creating a Wasm contract and tests for use on the Casper Platform.

#![deny(warnings)]

use std::{
    env,
    path::{Path, PathBuf},
};

use clap::{crate_description, crate_name, crate_version, App, Arg};
use once_cell::sync::Lazy;

pub mod common;
mod contract_package;
pub mod dependency;
//mod erc20;
mod makefile;
mod rust_toolchain;
mod simple;
mod tests_package;
mod travis_yml;

const USAGE: &str = r#"cargo casper [FLAGS] <path>
    cd <path>
    make prepare
    make test"#;
const AFTER_HELP: &str = r#"NOTE:
    If no other flag is provided, a trivial example contract and tests are created"#;

const ROOT_PATH_ARG_NAME: &str = "path";
const ROOT_PATH_ARG_VALUE_NAME: &str = "path";
const ROOT_PATH_ARG_ABOUT: &str = "Path to new folder for contract and tests";

const WORKSPACE_PATH_ARG_NAME: &str = "workspace-path";
const WORKSPACE_PATH_ARG_LONG: &str = "workspace-path";

const FAILURE_EXIT_CODE: i32 = 101;

static ARGS: Lazy<Args> = Lazy::new(Args::new);

// #[derive(Copy, Clone, PartialEq, Eq, Debug)]
// enum ProjectKind {
//     Simple,
//     Erc20,
// }

#[derive(Debug)]
struct Args {
    root_path: PathBuf,
    workspace_path: Option<PathBuf>,
}

impl Args {
    fn new() -> Self {
        // If run normally, the args passed are 'cargo-casper', '<target dir>'.  However, if run as
        // a cargo subcommand (i.e. cargo casper <target dir>), then cargo injects a new arg:
        // 'cargo-casper', 'casper', '<target dir>'.  We need to filter this extra arg out.
        //
        // This yields the situation where if the binary receives args of 'cargo-casper', 'casper'
        // then it might be a valid call (not a cargo subcommand - the user entered
        // 'cargo-casper casper' meaning to create a target dir called 'casper') or it might be an
        // invalid call (the user entered 'cargo casper' with no target dir specified).  The latter
        // case is assumed as being more likely.
        let filtered_args_iter = env::args().enumerate().filter_map(|(index, value)| {
            if index == 1 && value.as_str() == "casper" {
                None
            } else {
                Some(value)
            }
        });

        let root_path_arg = Arg::new(ROOT_PATH_ARG_NAME)
            .required(true)
            .value_name(ROOT_PATH_ARG_VALUE_NAME)
            .about(ROOT_PATH_ARG_ABOUT);

        let workspace_path_arg = Arg::new(WORKSPACE_PATH_ARG_NAME)
            .long(WORKSPACE_PATH_ARG_LONG)
            .takes_value(true)
            .hidden(true);

        let arg_matches = App::new(crate_name!())
            .version(crate_version!())
            .about(crate_description!())
            .override_usage(USAGE)
            .after_help(AFTER_HELP)
            .arg(root_path_arg)
            .arg(workspace_path_arg)
            .get_matches_from(filtered_args_iter);

        let root_path = arg_matches
            .value_of(ROOT_PATH_ARG_NAME)
            .expect("expected path")
            .into();

        let workspace_path = arg_matches
            .value_of(WORKSPACE_PATH_ARG_NAME)
            .map(PathBuf::from);

        Args {
            root_path,
            workspace_path,
        }
    }

    pub fn root_path(&self) -> &Path {
        &self.root_path
    }

    pub fn workspace_path(&self) -> Option<&Path> {
        self.workspace_path.as_deref()
    }
}

fn main() {
    if ARGS.root_path().exists() {
        common::print_error_and_exit(&format!(
            ": destination '{}' already exists",
            ARGS.root_path().display()
        ));
    }

    common::create_dir_all(ARGS.root_path());
    contract_package::create();
    tests_package::create();
    rust_toolchain::create();
    makefile::create();
    travis_yml::create();
}
