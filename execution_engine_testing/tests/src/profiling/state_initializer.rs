//! This executable is designed to be run to set up global state in preparation for running other
//! standalone test executable(s).  This will allow profiling to be done on executables running only
//! meaningful code, rather than including test setup effort in the profile results.

use std::{env, path::PathBuf};

use clap::{crate_version, App};

use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, ARG_AMOUNT, DEFAULT_ACCOUNTS,
    DEFAULT_ACCOUNT_ADDR, DEFAULT_AUCTION_DELAY, DEFAULT_GENESIS_CONFIG_HASH,
    DEFAULT_GENESIS_TIMESTAMP_MILLIS, DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS, DEFAULT_PAYMENT,
    DEFAULT_PROTOCOL_VERSION, DEFAULT_ROUND_SEIGNIORAGE_RATE, DEFAULT_SYSTEM_CONFIG,
    DEFAULT_UNBONDING_DELAY, DEFAULT_VALIDATOR_SLOTS, DEFAULT_WASM_CONFIG,
};
use casper_engine_tests::profiling;
use casper_execution_engine::core::engine_state::{
    engine_config::{EngineConfig, DEFAULT_FEE_HANDLING, DEFAULT_REFUND_HANDLING},
    genesis::ExecConfigBuilder,
    run_genesis_request::RunGenesisRequest,
};
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

    let engine_config = EngineConfig::default();
    let mut builder = LmdbWasmTestBuilder::new_with_config(&data_dir, engine_config);

    let exec_config = ExecConfigBuilder::default()
        .with_accounts(DEFAULT_ACCOUNTS.clone())
        .with_wasm_config(*DEFAULT_WASM_CONFIG)
        .with_system_config(*DEFAULT_SYSTEM_CONFIG)
        .with_validator_slots(DEFAULT_VALIDATOR_SLOTS)
        .with_auction_delay(DEFAULT_AUCTION_DELAY)
        .with_locked_funds_period_millis(DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS)
        .with_round_seigniorage_rate(DEFAULT_ROUND_SEIGNIORAGE_RATE)
        .with_unbonding_delay(DEFAULT_UNBONDING_DELAY)
        .with_genesis_timestamp_millis(DEFAULT_GENESIS_TIMESTAMP_MILLIS)
        .with_refund_handling(DEFAULT_REFUND_HANDLING)
        .with_fee_handling(DEFAULT_FEE_HANDLING)
        .build();

    let run_genesis_request = RunGenesisRequest::new(
        *DEFAULT_GENESIS_CONFIG_HASH,
        *DEFAULT_PROTOCOL_VERSION,
        exec_config,
    );

    let post_state_hash = builder
        .run_genesis(&run_genesis_request)
        .exec(exec_request)
        .expect_success()
        .commit()
        .get_post_state_hash();
    println!("{:?}", post_state_hash);
}
