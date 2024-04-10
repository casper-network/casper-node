//! A tool to update versions of all published Casper packages.

#![warn(unused, missing_copy_implementations, missing_docs)]
#![deny(
    deprecated_in_future,
    future_incompatible,
    macro_use_extern_crate,
    rust_2018_idioms,
    nonstandard_style,
    single_use_lifetimes,
    trivial_casts,
    trivial_numeric_casts,
    unsafe_code,
    unstable_features,
    unused_import_braces,
    unused_lifetimes,
    unused_qualifications,
    unused_results,
    warnings,
    clippy::all
)]
#![forbid(
    arithmetic_overflow,
    invalid_type_param_default,
    macro_expanded_macro_exports_accessed_by_absolute_paths,
    mutable_transmutes,
    no_mangle_const_items,
    order_dependent_trait_objects,
    overflowing_literals,
    pub_use_of_private_extern_crate,
    unknown_crate_types
)]

mod dependent_file;
mod package;
mod regex_data;

use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

use clap::{
    builder::{PathBufValueParser, PossibleValue},
    crate_version, Arg, ArgAction, Command as App,
};
use once_cell::sync::Lazy;
use semver::Version;

use package::Package;

const APP_NAME: &str = "Casper Updater";

const ROOT_DIR_ARG_NAME: &str = "root-dir";
const ROOT_DIR_ARG_SHORT: char = 'r';
const ROOT_DIR_ARG_VALUE_NAME: &str = "PATH";
const ROOT_DIR_ARG_HELP: &str =
    "Path to casper-node root directory.  If not supplied, assumes it is at ../..";

const BUMP_ARG_NAME: &str = "bump";
const BUMP_ARG_SHORT: char = 'b';
const BUMP_ARG_VALUE_NAME: &str = "VERSION-COMPONENT";
const BUMP_ARG_HELP: &str =
    "Increases all crates' versions automatically without asking for user input.  For a crate at \
    version x.y.z, the version will be bumped to (x+1).0.0, x.(y+1).0, or x.y.(z+1) depending on \
    which version component is specified.  If this option is specified, --activation-point must \
    also be specified.";
const MAJOR: &str = "major";
const MINOR: &str = "minor";
const PATCH: &str = "patch";

const DRY_RUN_ARG_NAME: &str = "dry-run";
const DRY_RUN_ARG_SHORT: char = 'd';
const DRY_RUN_ARG_HELP: &str = "Checks all regexes get matches in current casper-node repo";

const ALLOW_EARLIER_VERSION_NAME: &str = "allow-earlier-version";
const ALLOW_EARLIER_VERSION_HELP: &str = "Allows manual setting of version earlier than current";

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub(crate) enum BumpVersion {
    Major,
    Minor,
    Patch,
}

impl BumpVersion {
    pub(crate) fn update(self, current_version: &Version) -> Version {
        match self {
            BumpVersion::Major => Version::new(current_version.major + 1, 0, 0),
            BumpVersion::Minor => Version::new(current_version.major, current_version.minor + 1, 0),
            BumpVersion::Patch => Version::new(
                current_version.major,
                current_version.minor,
                current_version.patch + 1,
            ),
        }
    }
}

struct Args {
    root_dir: PathBuf,
    bump_version: Option<BumpVersion>,
    dry_run: bool,
    allow_earlier_version: bool,
}

/// The full path to the casper-node root directory.
pub(crate) fn root_dir() -> &'static Path {
    &ARGS.root_dir
}

/// The version component to bump, if any.
pub(crate) fn bump_version() -> Option<BumpVersion> {
    ARGS.bump_version
}

/// Whether we're doing a dry run or not.
pub(crate) fn is_dry_run() -> bool {
    ARGS.dry_run
}

/// If we allow reverting version to previous (used for master back to previous release branch)
pub(crate) fn allow_earlier_version() -> bool {
    ARGS.allow_earlier_version
}

static ARGS: Lazy<Args> = Lazy::new(get_args);

fn get_args() -> Args {
    let arg_matches = App::new(APP_NAME)
        .version(crate_version!())
        .arg(
            Arg::new(ROOT_DIR_ARG_NAME)
                .long(ROOT_DIR_ARG_NAME)
                .short(ROOT_DIR_ARG_SHORT)
                .value_name(ROOT_DIR_ARG_VALUE_NAME)
                .help(ROOT_DIR_ARG_HELP)
                .value_parser(PathBufValueParser::new()),
        )
        .arg(
            Arg::new(BUMP_ARG_NAME)
                .long(BUMP_ARG_NAME)
                .short(BUMP_ARG_SHORT)
                .value_name(BUMP_ARG_VALUE_NAME)
                .help(BUMP_ARG_HELP)
                .value_parser([
                    PossibleValue::new(MAJOR),
                    PossibleValue::new(MINOR),
                    PossibleValue::new(PATCH),
                ]),
        )
        .arg(
            Arg::new(DRY_RUN_ARG_NAME)
                .long(DRY_RUN_ARG_NAME)
                .short(DRY_RUN_ARG_SHORT)
                .action(ArgAction::SetTrue)
                .help(DRY_RUN_ARG_HELP),
        )
        .arg(
            Arg::new(ALLOW_EARLIER_VERSION_NAME)
                .long(ALLOW_EARLIER_VERSION_NAME)
                .action(ArgAction::SetTrue)
                .help(ALLOW_EARLIER_VERSION_HELP),
        )
        .get_matches();

    let root_dir = match arg_matches.get_one::<PathBuf>(ROOT_DIR_ARG_NAME) {
        Some(path) => path.clone(),
        None => env::current_dir()
            .expect("should be able to access current working dir")
            .parent()
            .expect("current working dir should have parent")
            .parent()
            .expect("current working dir should have two parents")
            .to_path_buf(),
    };

    let bump_version = arg_matches
        .get_one::<&str>(BUMP_ARG_NAME)
        .map(|value| match *value {
            MAJOR => BumpVersion::Major,
            MINOR => BumpVersion::Minor,
            PATCH => BumpVersion::Patch,
            _ => unreachable!(),
        });

    let dry_run = arg_matches.get_flag(DRY_RUN_ARG_NAME);

    let allow_earlier_version = arg_matches.get_flag(ALLOW_EARLIER_VERSION_NAME);

    Args {
        root_dir,
        bump_version,
        dry_run,
        allow_earlier_version,
    }
}

fn main() {
    let rust_packages = [
        Package::cargo("types", &regex_data::types::DEPENDENT_FILES),
        Package::cargo("binary_port", &regex_data::binary_port::DEPENDENT_FILES),
        Package::cargo("storage", &regex_data::storage::DEPENDENT_FILES),
        Package::cargo(
            "execution_engine",
            &regex_data::execution_engine::DEPENDENT_FILES,
        ),
        Package::cargo("node", &regex_data::node::DEPENDENT_FILES),
        Package::cargo(
            "smart_contracts/contract",
            &regex_data::smart_contracts_contract::DEPENDENT_FILES,
        ),
        Package::cargo(
            "execution_engine_testing/test_support",
            &regex_data::execution_engine_testing_test_support::DEPENDENT_FILES,
        ),
    ];

    for rust_package in &rust_packages {
        rust_package.update()
    }

    let smart_contracts_contract_as = Package::assembly_script(
        "smart_contracts/contract_as",
        &regex_data::smart_contracts_contract_as::DEPENDENT_FILES,
    );
    smart_contracts_contract_as.update();

    // Update Cargo.lock if this isn't a dry run.
    if !is_dry_run() {
        let mut command = Command::new(env!("CARGO"));
        let _ = command
            .current_dir(root_dir())
            .arg("update")
            .arg("--offline");
        for rust_package in &rust_packages {
            let _ = command.arg("--package").arg(rust_package.name());
        }
        let status = command
            .status()
            .unwrap_or_else(|error| panic!("Failed to execute '{:?}': {}", command, error));
        assert!(status.success(), "Failed to update Cargo.lock");
    }
}
