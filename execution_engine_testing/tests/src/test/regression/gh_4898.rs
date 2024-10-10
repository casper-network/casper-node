use casper_wasm::builder;
use num_traits::Zero;
use once_cell::sync::Lazy;

use casper_types::{GenesisAccount, GenesisValidator, Key};

use casper_engine_test_support::{
    utils, DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNTS,
    DEFAULT_ACCOUNT_ADDR, DEFAULT_ACCOUNT_INITIAL_BALANCE, DEFAULT_ACCOUNT_PUBLIC_KEY,
    DEFAULT_PAYMENT, LOCAL_GENESIS_REQUEST,
};
use casper_execution_engine::{
    engine_state::Error, execution::ExecError, runtime::PreprocessingError,
};
use casper_types::{
    account::AccountHash,
    addressable_entity::DEFAULT_ENTRY_POINT_NAME,
    runtime_args,
    system::auction::{self, DelegationRate},
    Motes, PublicKey, RuntimeArgs, SecretKey, DEFAULT_DELEGATE_COST, U512,
};

use crate::wasm_utils;

const ARG_DATA: &str = "data";
const GH_4898_REGRESSION_WASM: &str = "gh_4898_regression.wasm";

#[ignore]
#[test]
fn should_not_contain_f64_opcodes() {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        GH_4898_REGRESSION_WASM,
        runtime_args! {
            ARG_DATA => "account-hash-2c4a11c062a8a337bfc97e27fd66291caeb2c65865dcb5d3ef3759c4c97efecb"
        },
    )
    .build();

    builder.exec(exec_request).commit();

    // let error = builder
    //     .get_last_exec_result()
    //     .expect("should have results")
    //     .error()
    //     .cloned()
    //     .expect("should have error");
}