use assert_matches::assert_matches;
use lazy_static::lazy_static;

use casper_engine_test_support::{
    internal::{
        utils, DeployItemBuilder, ExecuteRequestBuilder, InMemoryWasmTestBuilder,
        UpgradeRequestBuilder, DEFAULT_ACCOUNT_KEY, DEFAULT_PAYMENT, DEFAULT_PROTOCOL_VERSION,
        DEFAULT_RUN_GENESIS_REQUEST,
    },
    DEFAULT_ACCOUNT_ADDR, DEFAULT_ACCOUNT_INITIAL_BALANCE, MINIMUM_ACCOUNT_CREATION_BALANCE,
};
use casper_execution_engine::{
    core::{
        engine_state::{upgrade::ActivationPoint, Error, CONV_RATE, MAX_PAYMENT},
        execution,
    },
    shared::{
        host_function_costs::{Cost, HostFunction, HostFunctionCosts},
        motes::Motes,
        opcode_costs::OpcodeCosts,
        storage_costs::StorageCosts,
        transform::Transform,
        wasm_config::{WasmConfig, DEFAULT_INITIAL_MEMORY, DEFAULT_MAX_STACK_HEIGHT},
    },
};
use casper_types::{
    account::AccountHash, runtime_args, ApiError, ProtocolVersion, RuntimeArgs, U512,
};

const ACCOUNT_1_ADDR: AccountHash = AccountHash::new([42u8; 32]);
const DO_NOTHING_WASM: &str = "do_nothing.wasm";
const TRANSFER_PURSE_TO_ACCOUNT_WASM: &str = "transfer_purse_to_account.wasm";
const REVERT_WASM: &str = "revert.wasm";
const ENDLESS_LOOP_WASM: &str = "endless_loop.wasm";
// const STANDARD_PAYMENT_WASM: &str = "standard_payment.wasm";
const ARG_AMOUNT: &str = "amount";
const ARG_TARGET: &str = "target";
lazy_static! {
    static ref EXHAUSTIVE_HOST_FUNCTION_COSTS: HostFunctionCosts = HostFunctionCosts {
        // Settings where all opcodes are so expensive so we can run out of gas very quickly
        get_main_purse: HostFunction::fixed(Cost::max_value()),
        .. Default::default()
    };

static ref EXHAUSTIVE_WASM_CONFIG: WasmConfig = WasmConfig::new(
    DEFAULT_INITIAL_MEMORY,
    DEFAULT_MAX_STACK_HEIGHT,
    OpcodeCosts::default(),
    StorageCosts::new(u32::max_value()),
    *EXHAUSTIVE_HOST_FUNCTION_COSTS,
);
}

#[ignore]
#[test]
fn should_raise_insufficient_payment_when_caller_lacks_minimum_balance() {
    let account_1_account_hash = ACCOUNT_1_ADDR;

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        TRANSFER_PURSE_TO_ACCOUNT_WASM,
        runtime_args! { ARG_TARGET => account_1_account_hash, ARG_AMOUNT => *MAX_PAYMENT - U512::one() },
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    let _response = builder
        .run_genesis(&DEFAULT_RUN_GENESIS_REQUEST)
        .exec(exec_request)
        .expect_success()
        .commit()
        .get_exec_response(0)
        .expect("there should be a response")
        .to_owned();

    let account_1_request =
        ExecuteRequestBuilder::standard(ACCOUNT_1_ADDR, REVERT_WASM, RuntimeArgs::default())
            .build();

    let account_1_response = builder
        .exec(account_1_request)
        .commit()
        .get_exec_response(1)
        .expect("there should be a response");

    let error_message = utils::get_error_message(account_1_response);

    assert!(
        error_message.contains("InsufficientPayment"),
        "expected insufficient payment, got: {}",
        error_message
    );

    let expected_transfers_count = 0;
    let transforms = builder.get_transforms();
    let transform = &transforms[1];

    assert_eq!(
        transform.len(),
        expected_transfers_count,
        "there should be no transforms if the account main purse has less than max payment"
    );
}

#[cfg(feature = "use-system-contracts")]
#[ignore]
#[test]
fn should_raise_insufficient_payment_when_payment_code_does_not_pay_enough() {
    let account_1_account_hash = ACCOUNT_1_ADDR;

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let proposer_reward_starting_balance = builder.get_proposer_purse_balance();

    let exec_request = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_deploy_hash([1; 32])
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => U512::from(1)})
            .with_session_code(
                TRANSFER_PURSE_TO_ACCOUNT_WASM,
                runtime_args! { ARG_TARGET => account_1_account_hash, ARG_AMOUNT => U512::from(1) },
            )
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_KEY])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(exec_request).commit();

    let proposer_reward_modified_balance = builder.get_proposer_purse_balance();

    let paid_transaction_fee = proposer_reward_modified_balance - proposer_reward_starting_balance;

    let modified_balance = builder.get_purse_balance(
        builder
            .get_account(*DEFAULT_ACCOUNT_ADDR)
            .expect("should have account")
            .main_purse(),
    );

    let initial_balance: U512 = U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE);
    let penalty_payment_amount = *MAX_PAYMENT;

    assert_eq!(
        modified_balance,
        initial_balance - penalty_payment_amount,
        "modified balance is incorrect"
    );

    assert_eq!(
        paid_transaction_fee, penalty_payment_amount,
        "transaction fee is incorrect"
    );

    assert_eq!(
        initial_balance,
        (modified_balance + paid_transaction_fee),
        "no net resources should be gained or lost post-distribution"
    );

    let response = builder
        .get_exec_response(0)
        .expect("there should be a response");

    let execution_result = utils::get_success_result(response);
    let error_message = format!(
        "{:?}",
        execution_result.as_error().expect("should have error")
    );

    assert_eq!(
        error_message, "InsufficientPayment",
        "expected insufficient payment"
    );
}

#[cfg(feature = "use-system-contracts")]
#[ignore]
#[test]
fn should_raise_insufficient_payment_error_when_out_of_gas() {
    let account_1_account_hash = ACCOUNT_1_ADDR;
    let payment_purse_amount: U512 = U512::from(1);
    let transferred_amount = U512::from(1);

    let exec_request = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_deploy_hash([1; 32])
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => payment_purse_amount})
            .with_session_code(
                TRANSFER_PURSE_TO_ACCOUNT_WASM,
                runtime_args! { ARG_TARGET => account_1_account_hash, ARG_AMOUNT => transferred_amount }
            )
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_KEY])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let proposer_reward_starting_balance = builder.get_proposer_purse_balance();

    builder.exec(exec_request).commit().finish();

    let transaction_fee = builder.get_proposer_purse_balance() - proposer_reward_starting_balance;
    let initial_balance: U512 = U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE);
    let expected_reward_balance = *MAX_PAYMENT;

    let modified_balance = builder.get_purse_balance(
        builder
            .get_account(*DEFAULT_ACCOUNT_ADDR)
            .expect("should have account")
            .main_purse(),
    );

    assert_eq!(
        modified_balance,
        initial_balance - expected_reward_balance,
        "modified balance is incorrect"
    );

    assert_eq!(
        transaction_fee, expected_reward_balance,
        "transaction fee is incorrect"
    );

    assert_eq!(
        initial_balance,
        (modified_balance + transaction_fee),
        "no net resources should be gained or lost post-distribution"
    );

    let response = builder
        .get_exec_response(0)
        .expect("there should be a response");

    let execution_result = utils::get_success_result(response);
    let error_message = format!(
        "{:?}",
        execution_result.as_error().expect("should have error")
    );

    assert_eq!(
        error_message, "InsufficientPayment",
        "expected insufficient payment"
    );
}

#[ignore]
#[test]
fn should_forward_payment_execution_runtime_error() {
    let account_1_account_hash = ACCOUNT_1_ADDR;
    let transferred_amount = U512::from(1);

    let exec_request = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_deploy_hash([1; 32])
            .with_payment_code(REVERT_WASM, RuntimeArgs::default())
            .with_session_code(
                TRANSFER_PURSE_TO_ACCOUNT_WASM,
                runtime_args! { ARG_TARGET => account_1_account_hash, ARG_AMOUNT => transferred_amount }
            )
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_KEY])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let proposer_reward_starting_balance = builder.get_proposer_purse_balance();

    builder.exec(exec_request).commit().finish();

    let transaction_fee = builder.get_proposer_purse_balance() - proposer_reward_starting_balance;
    let initial_balance: U512 = U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE);
    let expected_reward_balance = *MAX_PAYMENT;

    let modified_balance = builder.get_purse_balance(
        builder
            .get_account(*DEFAULT_ACCOUNT_ADDR)
            .expect("should have account")
            .main_purse(),
    );

    assert_eq!(
        modified_balance,
        initial_balance - expected_reward_balance,
        "modified balance is incorrect"
    );

    assert_eq!(
        transaction_fee, expected_reward_balance,
        "transaction fee is incorrect"
    );

    assert_eq!(
        initial_balance,
        (modified_balance + transaction_fee),
        "no net resources should be gained or lost post-distribution"
    );

    let response = builder
        .get_exec_response(0)
        .expect("there should be a response");

    let execution_result = utils::get_success_result(response);
    let error = execution_result.as_error().expect("should have error");
    assert_matches!(
        error,
        Error::Exec(execution::Error::Revert(ApiError::User(100)))
    );
}

#[ignore]
#[test]
fn should_forward_payment_execution_gas_limit_error() {
    let account_1_account_hash = ACCOUNT_1_ADDR;
    let transferred_amount = U512::from(1);

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let exec_request = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_deploy_hash([1; 32])
            .with_payment_code(ENDLESS_LOOP_WASM, RuntimeArgs::default())
            .with_session_code(
                TRANSFER_PURSE_TO_ACCOUNT_WASM,
                runtime_args! { ARG_TARGET => account_1_account_hash, ARG_AMOUNT => transferred_amount }
            )
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_KEY])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    let proposer_reward_starting_balance = builder.get_proposer_purse_balance();

    builder.exec(exec_request).commit().finish();

    let transaction_fee = builder.get_proposer_purse_balance() - proposer_reward_starting_balance;
    let initial_balance: U512 = U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE);
    let expected_reward_balance = *MAX_PAYMENT;

    let modified_balance = builder.get_purse_balance(
        builder
            .get_account(*DEFAULT_ACCOUNT_ADDR)
            .expect("should have account")
            .main_purse(),
    );

    assert_eq!(
        modified_balance,
        initial_balance - expected_reward_balance,
        "modified balance is incorrect"
    );

    assert_eq!(
        transaction_fee, expected_reward_balance,
        "transaction fee is incorrect"
    );

    assert_eq!(
        initial_balance,
        (modified_balance + transaction_fee),
        "no net resources should be gained or lost post-distribution"
    );

    let response = builder
        .get_exec_response(0)
        .expect("there should be a response");

    let execution_result = utils::get_success_result(response);
    let error = execution_result.as_error().expect("should have error");
    assert_matches!(error, Error::Exec(execution::Error::GasLimit));
}

#[ignore]
#[test]
fn should_run_out_of_gas_when_session_code_exceeds_gas_limit() {
    // Run tests as an account that starts with a smaller balance
    let account_1_balance: U512 = U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE);

    //
    // Increase cost of host function used inside test to decrease runtime of this test in debug
    // mode.
    //
    let new_protocol_version = ProtocolVersion::from_parts(
        DEFAULT_PROTOCOL_VERSION.value().major,
        DEFAULT_PROTOCOL_VERSION.value().minor,
        DEFAULT_PROTOCOL_VERSION.value().patch + 1,
    );

    const DEFAULT_ACTIVATION_POINT: ActivationPoint = 1;

    let mut upgrade_request = UpgradeRequestBuilder::new()
        .with_current_protocol_version(*DEFAULT_PROTOCOL_VERSION)
        .with_new_protocol_version(new_protocol_version)
        .with_activation_point(DEFAULT_ACTIVATION_POINT)
        .with_new_wasm_config(*EXHAUSTIVE_WASM_CONFIG)
        .build();

    // Fund separate account
    let fund_exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        TRANSFER_PURSE_TO_ACCOUNT_WASM,
        runtime_args! { ARG_TARGET => ACCOUNT_1_ADDR, ARG_AMOUNT => account_1_balance },
    )
    .with_protocol_version(new_protocol_version)
    .build();

    let exec_request =
        ExecuteRequestBuilder::standard(ACCOUNT_1_ADDR, ENDLESS_LOOP_WASM, RuntimeArgs::new())
            .with_protocol_version(new_protocol_version)
            .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    builder.upgrade_with_upgrade_request(&mut upgrade_request);

    builder.exec(fund_exec_request).expect_success().commit();

    builder.exec(exec_request).commit();

    let response = builder
        .get_exec_response(1)
        .expect("there should be a response");

    let execution_result = utils::get_success_result(response);
    let error = execution_result.as_error().expect("should have error");
    assert_matches!(error, Error::Exec(execution::Error::GasLimit));
}

#[ignore]
#[test]
fn should_correctly_charge_when_session_code_runs_out_of_gas() {
    let payment_purse_amount = *DEFAULT_PAYMENT;

    let exec_request = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_deploy_hash([1; 32])
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => payment_purse_amount })
            .with_session_code(ENDLESS_LOOP_WASM, RuntimeArgs::default())
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_KEY])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    let mut builder = InMemoryWasmTestBuilder::default();

    builder
        .run_genesis(&DEFAULT_RUN_GENESIS_REQUEST)
        .exec(exec_request)
        .commit()
        .finish();

    let default_account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should get genesis account");
    let modified_balance: U512 = builder.get_purse_balance(default_account.main_purse());
    let initial_balance: U512 = U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE);

    assert_ne!(
        modified_balance, initial_balance,
        "balance should be less than initial balance"
    );

    let response = builder
        .get_exec_response(0)
        .expect("there should be a response");

    let success_result = utils::get_success_result(&response);
    let gas = success_result.cost();
    let motes = Motes::from_gas(gas, CONV_RATE).expect("should have motes");

    let tally = motes.value() + modified_balance;

    assert_eq!(
        initial_balance, tally,
        "no net resources should be gained or lost post-distribution"
    );

    let execution_result = utils::get_success_result(response);
    let error = execution_result.as_error().expect("should have error");
    assert_matches!(error, Error::Exec(execution::Error::GasLimit));
}

#[ignore]
#[test]
fn should_correctly_charge_when_session_code_fails() {
    let account_1_account_hash = ACCOUNT_1_ADDR;
    let payment_purse_amount = *DEFAULT_PAYMENT;
    let transferred_amount = 1;

    let exec_request = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_deploy_hash([1; 32])
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => payment_purse_amount })
            .with_session_code(
                REVERT_WASM,
                runtime_args! { ARG_TARGET => account_1_account_hash, ARG_AMOUNT => U512::from(transferred_amount) },
            )
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_KEY])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    let mut builder = InMemoryWasmTestBuilder::default();

    builder
        .run_genesis(&DEFAULT_RUN_GENESIS_REQUEST)
        .exec(exec_request)
        .commit()
        .finish();

    let default_account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should get genesis account");
    let modified_balance: U512 = builder.get_purse_balance(default_account.main_purse());
    let initial_balance: U512 = U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE);

    assert_ne!(
        modified_balance, initial_balance,
        "balance should be less than initial balance"
    );

    let response = builder
        .get_exec_response(0)
        .expect("there should be a response")
        .clone();

    let success_result = utils::get_success_result(&response);
    let gas = success_result.cost();
    let motes = Motes::from_gas(gas, CONV_RATE).expect("should have motes");
    let tally = motes.value() + modified_balance;

    assert_eq!(
        initial_balance, tally,
        "no net resources should be gained or lost post-distribution"
    );
}

#[ignore]
#[test]
fn should_correctly_charge_when_session_code_succeeds() {
    let account_1_account_hash = ACCOUNT_1_ADDR;
    let payment_purse_amount = *DEFAULT_PAYMENT;
    let transferred_amount = 1;

    let exec_request = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_deploy_hash([1; 32])
            .with_session_code(
                TRANSFER_PURSE_TO_ACCOUNT_WASM,
                runtime_args! { ARG_TARGET => account_1_account_hash, ARG_AMOUNT => U512::from(transferred_amount) },
            )
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => payment_purse_amount })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_KEY])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    let mut builder = InMemoryWasmTestBuilder::default();

    builder
        .run_genesis(&DEFAULT_RUN_GENESIS_REQUEST)
        .exec(exec_request)
        .expect_success()
        .commit()
        .finish();

    let default_account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should get genesis account");
    let modified_balance: U512 = builder.get_purse_balance(default_account.main_purse());
    let initial_balance: U512 = U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE);

    assert_ne!(
        modified_balance, initial_balance,
        "balance should be less than initial balance"
    );

    let response = builder
        .get_exec_response(0)
        .expect("there should be a response")
        .clone();

    let success_result = utils::get_success_result(&response);
    let gas = success_result.cost();
    let motes = Motes::from_gas(gas, CONV_RATE).expect("should have motes");
    let total = motes.value() + U512::from(transferred_amount);
    let tally = total + modified_balance;

    assert_eq!(
        initial_balance, tally,
        "no net resources should be gained or lost post-distribution"
    );
    assert_eq!(
        initial_balance, tally,
        "no net resources should be gained or lost post-distribution"
    )
}

#[ignore]
#[test]
fn should_finalize_to_rewards_purse() {
    let account_1_account_hash = ACCOUNT_1_ADDR;
    let payment_purse_amount = *DEFAULT_PAYMENT;
    let transferred_amount = 1;

    let exec_request = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_session_code(
                TRANSFER_PURSE_TO_ACCOUNT_WASM,
                runtime_args! { ARG_TARGET => account_1_account_hash, ARG_AMOUNT => U512::from(transferred_amount) },
            )
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => payment_purse_amount })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_KEY])
            .with_deploy_hash([1; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let proposer_reward_starting_balance = builder.get_proposer_purse_balance();

    builder.exec(exec_request).expect_success().commit();

    let modified_reward_starting_balance = builder.get_proposer_purse_balance();

    assert!(
        modified_reward_starting_balance > proposer_reward_starting_balance,
        "proposer's balance should be higher after exec"
    );
}

#[ignore]
#[test]
fn independent_standard_payments_should_not_write_the_same_keys() {
    let account_1_account_hash = ACCOUNT_1_ADDR;
    let payment_purse_amount = *DEFAULT_PAYMENT;
    let transfer_amount = MINIMUM_ACCOUNT_CREATION_BALANCE;

    let mut builder = InMemoryWasmTestBuilder::default();

    let setup_exec_request = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_session_code(
                TRANSFER_PURSE_TO_ACCOUNT_WASM,
                runtime_args! { ARG_TARGET => account_1_account_hash, ARG_AMOUNT => U512::from(transfer_amount) },
            )
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => payment_purse_amount })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_KEY])
            .with_deploy_hash([1; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    // create another account via transfer
    builder
        .run_genesis(&DEFAULT_RUN_GENESIS_REQUEST)
        .exec(setup_exec_request)
        .expect_success()
        .commit();

    let exec_request_from_genesis = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_session_code(DO_NOTHING_WASM, RuntimeArgs::default())
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => payment_purse_amount })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_KEY])
            .with_deploy_hash([2; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    let exec_request_from_account_1 = {
        let deploy = DeployItemBuilder::new()
            .with_address(ACCOUNT_1_ADDR)
            .with_session_code(DO_NOTHING_WASM, RuntimeArgs::default())
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => payment_purse_amount })
            .with_authorization_keys(&[account_1_account_hash])
            .with_deploy_hash([1; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    // run two independent deploys
    builder
        .exec(exec_request_from_genesis)
        .expect_success()
        .commit()
        .exec(exec_request_from_account_1)
        .expect_success()
        .commit();

    let transforms = builder.get_transforms();
    let transforms_from_genesis = &transforms[1];
    let transforms_from_account_1 = &transforms[2];

    // confirm the two deploys have no overlapping writes
    let common_write_keys = transforms_from_genesis.keys().filter(|k| {
        match (
            transforms_from_genesis.get(k),
            transforms_from_account_1.get(k),
        ) {
            (Some(Transform::Write(_)), Some(Transform::Write(_))) => true,
            _ => false,
        }
    });

    assert_eq!(common_write_keys.count(), 0);
}
