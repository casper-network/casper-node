//! This executable is designed to be run to set up global state in preparation for running other
//! standalone test executable(s).  This will allow profiling to be done on executables running only
//! meaningful code, rather than including test setup effort in the profile results.

use std::{env, path::PathBuf};

use clap::{crate_version, App};

use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, ARG_AMOUNT,
    DEFAULT_ACCOUNT_ADDR, DEFAULT_PAYMENT, PRODUCTION_PATH,
};
use casper_engine_tests::profiling;
use casper_types::{runtime_args, RuntimeArgs};

const ABOUT: &str = "Initializes global state in preparation for profiling runs. Outputs the root \
                     hash from the commit response.";
const STATE_INITIALIZER_CONTRACT: &str = "state_initializer.wasm";
const ARG_ACCOUNT1_HASH: &str = "account_1_account_hash";
const ARG_ACCOUNT1_AMOUNT: &str = "account_1_amount";
const ARG_ACCOUNT2_HASH: &str = "account_2_account_hash";

fn data_dir() -> PathBuf {
    let exe_name = profiling::exe_name();
    let data_dir_arg = profiling::data_dir_arg();
    let arg_matches = App::new(&exe_name)
        .version(crate_version!())
        .about(ABOUT)
        .arg(data_dir_arg)
        .get_matches();
    profiling::data_dir(&arg_matches)
}

fn main() {
    let data_dir = data_dir();

    let genesis_account_hash = *DEFAULT_ACCOUNT_ADDR;
    let account_1_account_hash = profiling::account_1_account_hash();
    let account_1_initial_amount = profiling::account_1_initial_amount();
    let account_2_account_hash = profiling::account_2_account_hash();

    let exec_request = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_deploy_hash([1; 32])
            .with_session_code(
                STATE_INITIALIZER_CONTRACT,
                runtime_args! {
                    ARG_ACCOUNT1_HASH => account_1_account_hash,
                    ARG_ACCOUNT1_AMOUNT => account_1_initial_amount,
                    ARG_ACCOUNT2_HASH => account_2_account_hash,
                },
            )
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_authorization_keys(&[genesis_account_hash])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    let mut builder = LmdbWasmTestBuilder::new_with_config(&data_dir, &*PRODUCTION_PATH);

    let post_state_hash = builder
        .run_genesis_with_default_genesis_accounts()
        .exec(exec_request)
        .expect_success()
        .commit()
        .get_post_state_hash();
    println!("{:?}", post_state_hash);
}
