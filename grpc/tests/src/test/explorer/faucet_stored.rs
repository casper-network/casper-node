use engine_test_support::{
    internal::{
        utils, DeployItemBuilder, ExecuteRequestBuilder, InMemoryWasmTestBuilder,
        DEFAULT_ACCOUNT_KEY, DEFAULT_RUN_GENESIS_REQUEST,
    },
    DEFAULT_ACCOUNT_ADDR,
};
use types::{account::AccountHash, runtime_args, ApiError, RuntimeArgs, U512};

const FAUCET: &str = "faucet";
const CALL_FAUCET: &str = "call_faucet";
const NEW_ACCOUNT_ADDR: AccountHash = AccountHash::new([99u8; 32]);

fn get_builder() -> InMemoryWasmTestBuilder {
    let mut builder = InMemoryWasmTestBuilder::default();
    {
        // first, store contract
        let store_request = ExecuteRequestBuilder::standard(
            DEFAULT_ACCOUNT_ADDR,
            &format!("{}_stored.wasm", FAUCET),
            runtime_args! {},
        )
        .build();

        builder.run_genesis(&*DEFAULT_RUN_GENESIS_REQUEST);
        builder.exec_commit_finish(store_request);
    }
    builder
}

#[ignore]
#[test]
fn should_get_funds_from_faucet_stored() {
    let mut builder = get_builder();

    let default_account = builder
        .get_account(DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let contract_hash = default_account
        .named_keys()
        .get(FAUCET)
        .expect("contract_hash should exist")
        .into_hash()
        .expect("should be a hash");

    let amount = U512::from(1000);

    // call stored faucet
    let exec_request = {
        let deploy = DeployItemBuilder::new()
            .with_address(DEFAULT_ACCOUNT_ADDR)
            .with_stored_session_hash(
                contract_hash,
                CALL_FAUCET,
                runtime_args! { "target" => NEW_ACCOUNT_ADDR, "amount" => amount },
            )
            .with_empty_payment_bytes(runtime_args! { "amount" => U512::from(10_000_000) })
            .with_authorization_keys(&[DEFAULT_ACCOUNT_KEY])
            .with_deploy_hash([2; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };
    builder.exec(exec_request).expect_success().commit();

    let account = builder
        .get_account(NEW_ACCOUNT_ADDR)
        .expect("should get account");

    let account_purse = account.main_purse();
    let account_balance = builder.get_purse_balance(account_purse);
    assert_eq!(
        account_balance, amount,
        "faucet should have created account with requested amount"
    );
}

#[ignore]
#[test]
fn should_fail_if_already_funded() {
    let mut builder = get_builder();

    let default_account = builder
        .get_account(DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let contract_hash = default_account
        .named_keys()
        .get(FAUCET)
        .expect("contract_hash should exist")
        .into_hash()
        .expect("should be a hash");

    let amount = U512::from(1000);

    // call stored faucet
    let exec_request = {
        let deploy = DeployItemBuilder::new()
            .with_address(DEFAULT_ACCOUNT_ADDR)
            .with_stored_session_hash(
                contract_hash,
                CALL_FAUCET,
                runtime_args! { "target" => NEW_ACCOUNT_ADDR, "amount" => amount },
            )
            .with_empty_payment_bytes(runtime_args! { "amount" => U512::from(10_000_000) })
            .with_authorization_keys(&[DEFAULT_ACCOUNT_KEY])
            .with_deploy_hash([2; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };
    builder.exec(exec_request).expect_success().commit();

    // call stored faucet again; should error
    let exec_request = {
        let deploy = DeployItemBuilder::new()
            .with_address(DEFAULT_ACCOUNT_ADDR)
            .with_stored_session_hash(
                contract_hash,
                CALL_FAUCET,
                runtime_args! { "target" => NEW_ACCOUNT_ADDR, "amount" => amount },
            )
            .with_empty_payment_bytes(runtime_args! { "amount" => U512::from(10_000_000) })
            .with_authorization_keys(&[DEFAULT_ACCOUNT_KEY])
            .with_deploy_hash([2; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(exec_request);

    let exec_response = builder
        .get_exec_response(2)
        .expect("Expected to be called after run()");

    let error_message = utils::get_error_message(exec_response);
    assert!(
        error_message.contains(&format!("{:?}", ApiError::User(1))),
        "should have reverted with user error 1 (already funded)"
    );
}
