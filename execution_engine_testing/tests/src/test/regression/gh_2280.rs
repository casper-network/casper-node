use casper_execution_engine::engine_state::{EngineConfig, EngineConfigBuilder};
use once_cell::sync::Lazy;

use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, UpgradeRequestBuilder,
    DEFAULT_ACCOUNT_ADDR, DEFAULT_PROTOCOL_VERSION, MINIMUM_ACCOUNT_CREATION_BALANCE,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_types::{
    account::AccountHash, runtime_args, system::mint, AddressableEntityHash, EraId, Gas,
    HostFunction, HostFunctionCost, HostFunctionCosts, Key, MintCosts, Motes,
    ProtocolUpgradeConfig, ProtocolVersion, PublicKey, SecretKey, SystemConfig, WasmConfig,
    DEFAULT_MAX_STACK_HEIGHT, DEFAULT_WASM_MAX_MEMORY, U512,
};

const TRANSFER_TO_ACCOUNT_CONTRACT: &str = "transfer_to_account.wasm";
const TRANSFER_PURSE_TO_ACCOUNT_CONTRACT: &str = "transfer_purse_to_account.wasm";
const GH_2280_REGRESSION_CONTRACT: &str = "gh_2280_regression.wasm";
const GH_2280_REGRESSION_CALL_CONTRACT: &str = "gh_2280_regression_call.wasm";
const CREATE_PURSE_01_CONTRACT: &str = "create_purse_01.wasm";
const FAUCET_NAME: &str = "faucet";

static ACCOUNT_1_PK: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([200; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static ACCOUNT_1_ADDR: Lazy<AccountHash> = Lazy::new(|| ACCOUNT_1_PK.to_account_hash());

static ACCOUNT_2_PK: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([202; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static ACCOUNT_2_ADDR: Lazy<AccountHash> = Lazy::new(|| ACCOUNT_2_PK.to_account_hash());

static ACCOUNT_3_PK: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([204; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static ACCOUNT_3_ADDR: Lazy<AccountHash> = Lazy::new(|| ACCOUNT_3_PK.to_account_hash());

const ARG_TARGET: &str = "target";
const ARG_AMOUNT: &str = "amount";

const TOKEN_AMOUNT: u64 = 1_000_000;

const ARG_PURSE_NAME: &str = "purse_name";
const TEST_PURSE_NAME: &str = "test";

static OLD_PROTOCOL_VERSION: Lazy<ProtocolVersion> = Lazy::new(|| *DEFAULT_PROTOCOL_VERSION);
static NEW_PROTOCOL_VERSION: Lazy<ProtocolVersion> = Lazy::new(|| {
    ProtocolVersion::from_parts(
        OLD_PROTOCOL_VERSION.value().major,
        OLD_PROTOCOL_VERSION.value().minor,
        OLD_PROTOCOL_VERSION.value().patch + 1,
    )
});
const DEFAULT_ACTIVATION_POINT: EraId = EraId::new(1);

const HOST_FUNCTION_COST_CHANGE: HostFunctionCost = 13_730_593; // random prime number

const ARG_FAUCET_FUNDS: &str = "faucet_initial_balance";
const HASH_KEY_NAME: &str = "gh_2280_hash";
const ARG_CONTRACT_HASH: &str = "contract_hash";

#[ignore]
#[test]
fn gh_2280_transfer_should_always_cost_the_same_gas() {
    let session_file = TRANSFER_TO_ACCOUNT_CONTRACT;
    let account_hash = *DEFAULT_ACCOUNT_ADDR;

    let (mut builder, _) = setup();

    let faucet_args_1 = runtime_args! {
        ARG_TARGET => *ACCOUNT_1_ADDR,
        ARG_AMOUNT => TOKEN_AMOUNT,
    };

    let fund_request_1 =
        ExecuteRequestBuilder::standard(account_hash, session_file, faucet_args_1).build();
    builder.exec(fund_request_1).expect_success().commit();

    let gas_cost_1 = builder.last_exec_gas_cost();

    // Next time pay exactly the amount that was reported which should be also the minimum you
    // should be able to pay next time.
    let payment_amount = Motes::from_gas(gas_cost_1, 1).unwrap();

    let fund_request_2 = {
        let deploy_hash: [u8; 32] = [55; 32];
        let faucet_args_2 = runtime_args! {
            ARG_TARGET => *ACCOUNT_2_ADDR,
            ARG_AMOUNT => TOKEN_AMOUNT,
        };

        let deploy = DeployItemBuilder::new()
            .with_address(account_hash)
            .with_session_code(session_file, faucet_args_2)
            // + default_create_purse_cost
            .with_empty_payment_bytes(runtime_args! {
                ARG_AMOUNT => payment_amount.value()
            })
            .with_authorization_keys(&[account_hash])
            .with_deploy_hash(deploy_hash)
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy)
    }
    .build();
    builder.exec(fund_request_2).expect_success().commit();

    let gas_cost_2 = builder.last_exec_gas_cost();

    assert_eq!(gas_cost_1, gas_cost_2);

    let mut upgrade_request = make_upgrade_request();

    // Increase "transfer_to_account" host function call exactly by X, so we can assert that
    // transfer cost increased by exactly X without hidden fees.
    let default_host_function_costs = HostFunctionCosts::default();

    let default_transfer_to_account_cost = default_host_function_costs.transfer_to_account.cost();
    let new_transfer_to_account_cost = default_transfer_to_account_cost
        .checked_add(HOST_FUNCTION_COST_CHANGE)
        .expect("should add without overflow");
    let new_transfer_to_account = HostFunction::fixed(new_transfer_to_account_cost);

    let new_host_function_costs = HostFunctionCosts {
        transfer_to_account: new_transfer_to_account,
        ..default_host_function_costs
    };

    let new_wasm_config = make_wasm_config(
        new_host_function_costs,
        *builder.get_engine_state().config().wasm_config(),
    );

    // Inflate affected system contract entry point cost to the maximum
    let new_mint_create_cost = u32::MAX;
    let new_mint_costs = MintCosts {
        create: new_mint_create_cost,
        ..Default::default()
    };

    let new_engine_config = make_engine_config(
        new_mint_costs,
        new_wasm_config,
        *builder.get_engine_state().config().system_config(),
    );

    builder.upgrade_with_upgrade_request_and_config(Some(new_engine_config), &mut upgrade_request);

    let fund_request_3 = {
        let deploy_hash: [u8; 32] = [77; 32];
        let faucet_args_3 = runtime_args! {
            ARG_TARGET => *ACCOUNT_3_ADDR,
            ARG_AMOUNT => TOKEN_AMOUNT,
        };

        let deploy = DeployItemBuilder::new()
            .with_address(account_hash)
            .with_session_code(session_file, faucet_args_3)
            .with_empty_payment_bytes(runtime_args! {
                ARG_AMOUNT => payment_amount.value() + HOST_FUNCTION_COST_CHANGE
            })
            .with_authorization_keys(&[account_hash])
            .with_deploy_hash(deploy_hash)
            .build();

        ExecuteRequestBuilder::new()
            .push_deploy(deploy)
            .with_protocol_version(*NEW_PROTOCOL_VERSION)
            .build()
    };

    builder.exec(fund_request_3).expect_success().commit();

    let gas_cost_3 = builder.last_exec_gas_cost();

    assert!(gas_cost_3 > gas_cost_1);
    assert!(gas_cost_3 > gas_cost_2);

    let gas_cost_diff = gas_cost_3.checked_sub(gas_cost_2).unwrap_or_default();
    assert_eq!(
        gas_cost_diff,
        Gas::new(U512::from(HOST_FUNCTION_COST_CHANGE))
    );
}

#[ignore]
#[test]
fn gh_2280_create_purse_should_always_cost_the_same_gas() {
    let account_hash = *DEFAULT_ACCOUNT_ADDR;
    let session_file = CREATE_PURSE_01_CONTRACT;

    let (mut builder, _) = setup();

    let create_purse_args_1 = runtime_args! {
        ARG_PURSE_NAME => TEST_PURSE_NAME
    };

    let fund_request_1 =
        ExecuteRequestBuilder::standard(account_hash, session_file, create_purse_args_1).build();
    builder.exec(fund_request_1).expect_success().commit();

    let gas_cost_1 = builder.last_exec_gas_cost();

    // Next time pay exactly the amount that was reported which should be also the minimum you
    // should be able to pay next time.
    let payment_amount = Motes::from_gas(gas_cost_1, 1).unwrap();

    let fund_request_2 = {
        let deploy_hash: [u8; 32] = [55; 32];
        let create_purse_args_2 = runtime_args! {
            ARG_PURSE_NAME => TEST_PURSE_NAME,
        };

        let deploy = DeployItemBuilder::new()
            .with_address(account_hash)
            .with_session_code(session_file, create_purse_args_2)
            // + default_create_purse_cost
            .with_empty_payment_bytes(runtime_args! {
                ARG_AMOUNT => payment_amount.value()
            })
            .with_authorization_keys(&[account_hash])
            .with_deploy_hash(deploy_hash)
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy)
    }
    .build();
    builder.exec(fund_request_2).expect_success().commit();

    let gas_cost_2 = builder.last_exec_gas_cost();

    assert_eq!(gas_cost_1, gas_cost_2);

    let mut upgrade_request = make_upgrade_request();

    // Increase "transfer_to_account" host function call exactly by X, so we can assert that
    // transfer cost increased by exactly X without hidden fees.
    let host_function_costs = builder
        .get_engine_state()
        .config()
        .wasm_config()
        .take_host_function_costs();

    let default_create_purse_cost = host_function_costs.create_purse.cost();
    let new_create_purse_cost = default_create_purse_cost
        .checked_add(HOST_FUNCTION_COST_CHANGE)
        .expect("should add without overflow");
    let new_create_purse = HostFunction::fixed(new_create_purse_cost);

    let new_host_function_costs = HostFunctionCosts {
        create_purse: new_create_purse,
        ..host_function_costs
    };

    let new_wasm_config = make_wasm_config(
        new_host_function_costs,
        *builder.get_engine_state().config().wasm_config(),
    );

    // Inflate affected system contract entry point cost to the maximum
    let new_mint_create_cost = u32::MAX;
    let new_mint_costs = MintCosts {
        create: new_mint_create_cost,
        ..Default::default()
    };

    let new_engine_config = make_engine_config(
        new_mint_costs,
        new_wasm_config,
        *builder.get_engine_state().config().system_config(),
    );

    builder
        .upgrade_with_upgrade_request_and_config(Some(new_engine_config), &mut upgrade_request)
        .expect_upgrade_success();

    let fund_request_3 = {
        let deploy_hash: [u8; 32] = [77; 32];
        let create_purse_args_3 = runtime_args! {
            ARG_PURSE_NAME => TEST_PURSE_NAME,
        };

        let deploy = DeployItemBuilder::new()
            .with_address(account_hash)
            .with_session_code(session_file, create_purse_args_3)
            .with_empty_payment_bytes(runtime_args! {
                ARG_AMOUNT => payment_amount.value() + HOST_FUNCTION_COST_CHANGE
            })
            .with_authorization_keys(&[account_hash])
            .with_deploy_hash(deploy_hash)
            .build();

        ExecuteRequestBuilder::new()
            .push_deploy(deploy)
            .with_protocol_version(*NEW_PROTOCOL_VERSION)
            .build()
    };

    builder.exec(fund_request_3).expect_success().commit();

    let gas_cost_3 = builder.last_exec_gas_cost();

    assert!(gas_cost_3 > gas_cost_1);
    assert!(gas_cost_3 > gas_cost_2);

    let gas_cost_diff = gas_cost_3.checked_sub(gas_cost_2).unwrap_or_default();
    assert_eq!(
        gas_cost_diff,
        Gas::new(U512::from(HOST_FUNCTION_COST_CHANGE))
    );
}

#[ignore]
#[test]
fn gh_2280_transfer_purse_to_account_should_always_cost_the_same_gas() {
    let account_hash = *DEFAULT_ACCOUNT_ADDR;
    let session_file = TRANSFER_PURSE_TO_ACCOUNT_CONTRACT;

    let (mut builder, _) = setup();

    let faucet_args_1 = runtime_args! {
        ARG_TARGET => *ACCOUNT_1_ADDR,
        ARG_AMOUNT => U512::from(TOKEN_AMOUNT),
    };

    let fund_request_1 =
        ExecuteRequestBuilder::standard(account_hash, session_file, faucet_args_1).build();
    builder.exec(fund_request_1).expect_success().commit();

    let gas_cost_1 = builder.last_exec_gas_cost();

    // Next time pay exactly the amount that was reported which should be also the minimum you
    // should be able to pay next time.
    let payment_amount = Motes::from_gas(gas_cost_1, 1).unwrap();

    let fund_request_2 = {
        let deploy_hash: [u8; 32] = [55; 32];
        let faucet_args_2 = runtime_args! {
            ARG_TARGET => *ACCOUNT_2_ADDR,
            ARG_AMOUNT => U512::from(TOKEN_AMOUNT),
        };

        let deploy = DeployItemBuilder::new()
            .with_address(account_hash)
            .with_session_code(TRANSFER_PURSE_TO_ACCOUNT_CONTRACT, faucet_args_2)
            // + default_create_purse_cost
            .with_empty_payment_bytes(runtime_args! {
                ARG_AMOUNT => payment_amount.value()
            })
            .with_authorization_keys(&[account_hash])
            .with_deploy_hash(deploy_hash)
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy)
    }
    .build();
    builder.exec(fund_request_2).expect_success().commit();

    let gas_cost_2 = builder.last_exec_gas_cost();

    assert_eq!(gas_cost_1, gas_cost_2);

    let mut upgrade_request = make_upgrade_request();

    // Increase "transfer_to_account" host function call exactly by X, so we can assert that
    // transfer cost increased by exactly X without hidden fees.
    let default_host_function_costs = HostFunctionCosts::default();

    let default_transfer_from_purse_to_account_cost = default_host_function_costs
        .transfer_from_purse_to_account
        .cost();
    let new_transfer_from_purse_to_account_cost = default_transfer_from_purse_to_account_cost
        .checked_add(HOST_FUNCTION_COST_CHANGE)
        .expect("should add without overflow");
    let new_transfer_from_purse_to_account =
        HostFunction::fixed(new_transfer_from_purse_to_account_cost);

    let new_host_function_costs = HostFunctionCosts {
        transfer_from_purse_to_account: new_transfer_from_purse_to_account,
        ..default_host_function_costs
    };

    let new_wasm_config = make_wasm_config(
        new_host_function_costs,
        *builder.get_engine_state().config().wasm_config(),
    );

    // Inflate affected system contract entry point cost to the maximum
    let new_mint_create_cost = u32::MAX;
    let new_mint_costs = MintCosts {
        create: new_mint_create_cost,
        ..Default::default()
    };
    let new_engine_config = make_engine_config(
        new_mint_costs,
        new_wasm_config,
        *builder.get_engine_state().config().system_config(),
    );

    builder.upgrade_with_upgrade_request_and_config(Some(new_engine_config), &mut upgrade_request);

    let fund_request_3 = {
        let deploy_hash: [u8; 32] = [77; 32];
        let faucet_args_3 = runtime_args! {
            ARG_TARGET => *ACCOUNT_3_ADDR,
            ARG_AMOUNT => U512::from(TOKEN_AMOUNT),
        };

        let deploy = DeployItemBuilder::new()
            .with_address(account_hash)
            .with_session_code(session_file, faucet_args_3)
            .with_empty_payment_bytes(runtime_args! {
                ARG_AMOUNT => payment_amount.value() + HOST_FUNCTION_COST_CHANGE
            })
            .with_authorization_keys(&[account_hash])
            .with_deploy_hash(deploy_hash)
            .build();

        ExecuteRequestBuilder::new()
            .push_deploy(deploy)
            .with_protocol_version(*NEW_PROTOCOL_VERSION)
            .build()
    };

    builder.exec(fund_request_3).expect_success().commit();

    let gas_cost_3 = builder.last_exec_gas_cost();

    assert!(gas_cost_3 > gas_cost_1);
    assert!(gas_cost_3 > gas_cost_2);

    let gas_cost_diff = gas_cost_3.checked_sub(gas_cost_2).unwrap_or_default();
    assert_eq!(
        gas_cost_diff,
        Gas::new(U512::from(HOST_FUNCTION_COST_CHANGE))
    );
}

#[ignore]
#[test]
fn gh_2280_stored_transfer_to_account_should_always_cost_the_same_gas() {
    let account_hash = *DEFAULT_ACCOUNT_ADDR;
    let entry_point = FAUCET_NAME;

    let (mut builder, TestContext { gh_2280_regression }) = setup();

    let faucet_args_1 = runtime_args! {
        ARG_TARGET => *ACCOUNT_1_ADDR,
    };

    let fund_request_1 = ExecuteRequestBuilder::contract_call_by_hash(
        account_hash,
        gh_2280_regression,
        entry_point,
        faucet_args_1,
    )
    .build();
    builder.exec(fund_request_1).expect_success().commit();

    let gas_cost_1 = builder.last_exec_gas_cost();

    // Next time pay exactly the amount that was reported which should be also the minimum you
    // should be able to pay next time.
    let payment_amount = Motes::from_gas(gas_cost_1, 1).unwrap();

    let fund_request_2 = {
        let deploy_hash: [u8; 32] = [55; 32];
        let faucet_args_2 = runtime_args! {
            ARG_TARGET => *ACCOUNT_2_ADDR,
        };

        let deploy = DeployItemBuilder::new()
            .with_address(account_hash)
            .with_stored_session_hash(gh_2280_regression, entry_point, faucet_args_2)
            // + default_create_purse_cost
            .with_empty_payment_bytes(runtime_args! {
                ARG_AMOUNT => payment_amount.value()
            })
            .with_authorization_keys(&[account_hash])
            .with_deploy_hash(deploy_hash)
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy)
    }
    .build();
    builder.exec(fund_request_2).expect_success().commit();

    let gas_cost_2 = builder.last_exec_gas_cost();

    assert_eq!(gas_cost_1, gas_cost_2);

    let mut upgrade_request = make_upgrade_request();

    // Increase "transfer_to_account" host function call exactly by X, so we can assert that
    // transfer cost increased by exactly X without hidden fees.
    let default_host_function_costs = HostFunctionCosts::default();

    let default_transfer_from_purse_to_account_cost = default_host_function_costs
        .transfer_from_purse_to_account
        .cost();
    let new_transfer_from_purse_to_account_cost = default_transfer_from_purse_to_account_cost
        .checked_add(HOST_FUNCTION_COST_CHANGE)
        .expect("should add without overflow");
    let new_transfer_from_purse_to_account =
        HostFunction::fixed(new_transfer_from_purse_to_account_cost);

    let new_host_function_costs = HostFunctionCosts {
        transfer_from_purse_to_account: new_transfer_from_purse_to_account,
        ..default_host_function_costs
    };

    let new_wasm_config = make_wasm_config(
        new_host_function_costs,
        *builder.get_engine_state().config().wasm_config(),
    );

    // Inflate affected system contract entry point cost to the maximum
    let new_mint_create_cost = u32::MAX;
    let new_mint_costs = MintCosts {
        create: new_mint_create_cost,
        ..Default::default()
    };

    let new_engine_config = make_engine_config(
        new_mint_costs,
        new_wasm_config,
        *builder.get_engine_state().config().system_config(),
    );

    builder.upgrade_with_upgrade_request_and_config(Some(new_engine_config), &mut upgrade_request);

    let fund_request_3 = {
        let deploy_hash: [u8; 32] = [77; 32];
        let faucet_args_3 = runtime_args! {
            ARG_TARGET => *ACCOUNT_3_ADDR,
        };

        let deploy = DeployItemBuilder::new()
            .with_address(account_hash)
            .with_stored_session_hash(gh_2280_regression, entry_point, faucet_args_3)
            .with_empty_payment_bytes(runtime_args! {
                ARG_AMOUNT => payment_amount.value() + HOST_FUNCTION_COST_CHANGE
            })
            .with_authorization_keys(&[account_hash])
            .with_deploy_hash(deploy_hash)
            .build();

        ExecuteRequestBuilder::new()
            .push_deploy(deploy)
            .with_protocol_version(*NEW_PROTOCOL_VERSION)
            .build()
    };

    builder.exec(fund_request_3).expect_success().commit();

    let gas_cost_3 = builder.last_exec_gas_cost();

    assert!(gas_cost_3 > gas_cost_1, "{} <= {}", gas_cost_3, gas_cost_1);
    assert!(gas_cost_3 > gas_cost_2);

    let gas_cost_diff = gas_cost_3.checked_sub(gas_cost_2).unwrap_or_default();
    assert_eq!(
        gas_cost_diff,
        Gas::new(U512::from(HOST_FUNCTION_COST_CHANGE))
    );
}

#[ignore]
#[test]
fn gh_2280_stored_faucet_call_should_cost_the_same() {
    let session_file = GH_2280_REGRESSION_CALL_CONTRACT;
    let account_hash = *DEFAULT_ACCOUNT_ADDR;

    let (mut builder, TestContext { gh_2280_regression }) = setup();

    let faucet_args_1 = runtime_args! {
        ARG_CONTRACT_HASH => gh_2280_regression,
        ARG_TARGET => *ACCOUNT_1_ADDR,
    };

    let fund_request_1 =
        ExecuteRequestBuilder::standard(account_hash, session_file, faucet_args_1).build();
    builder.exec(fund_request_1).expect_success().commit();

    let gas_cost_1 = builder.last_exec_gas_cost();

    // Next time pay exactly the amount that was reported which should be also the minimum you
    // should be able to pay next time.
    let payment_amount = Motes::from_gas(gas_cost_1, 1).unwrap();

    let fund_request_2 = {
        let deploy_hash: [u8; 32] = [55; 32];
        let faucet_args_2 = runtime_args! {
            ARG_CONTRACT_HASH => gh_2280_regression,
            ARG_TARGET => *ACCOUNT_2_ADDR,
        };

        let deploy = DeployItemBuilder::new()
            .with_address(account_hash)
            .with_session_code(session_file, faucet_args_2)
            // + default_create_purse_cost
            .with_empty_payment_bytes(runtime_args! {
                ARG_AMOUNT => payment_amount.value()
            })
            .with_authorization_keys(&[account_hash])
            .with_deploy_hash(deploy_hash)
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy)
    }
    .build();
    builder.exec(fund_request_2).expect_success().commit();

    let gas_cost_2 = builder.last_exec_gas_cost();

    assert_eq!(gas_cost_1, gas_cost_2);

    let mut upgrade_request = make_upgrade_request();

    // Increase "transfer_to_account" host function call exactly by X, so we can assert that
    // transfer cost increased by exactly X without hidden fees.
    let default_host_function_costs = HostFunctionCosts::default();

    let default_transfer_from_purse_to_account_cost = default_host_function_costs
        .transfer_from_purse_to_account
        .cost();
    let new_transfer_from_purse_to_account_cost = default_transfer_from_purse_to_account_cost
        .checked_add(HOST_FUNCTION_COST_CHANGE)
        .expect("should add without overflow");
    let new_transfer_from_purse_to_account =
        HostFunction::fixed(new_transfer_from_purse_to_account_cost);

    let new_host_function_costs = HostFunctionCosts {
        transfer_from_purse_to_account: new_transfer_from_purse_to_account,
        ..default_host_function_costs
    };

    let new_wasm_config = make_wasm_config(
        new_host_function_costs,
        *builder.get_engine_state().config().wasm_config(),
    );

    // Inflate affected system contract entry point cost to the maximum
    let new_mint_create_cost = u32::MAX;
    let new_mint_costs = MintCosts {
        create: new_mint_create_cost,
        ..Default::default()
    };

    let new_engine_config = make_engine_config(
        new_mint_costs,
        new_wasm_config,
        *builder.get_engine_state().config().system_config(),
    );

    builder.upgrade_with_upgrade_request_and_config(Some(new_engine_config), &mut upgrade_request);

    let fund_request_3 = {
        let deploy_hash: [u8; 32] = [77; 32];
        let faucet_args_3 = runtime_args! {
            ARG_CONTRACT_HASH => gh_2280_regression,
            ARG_TARGET => *ACCOUNT_3_ADDR,
        };

        let deploy = DeployItemBuilder::new()
            .with_address(account_hash)
            .with_session_code(session_file, faucet_args_3)
            .with_empty_payment_bytes(runtime_args! {
                ARG_AMOUNT => payment_amount.value() + HOST_FUNCTION_COST_CHANGE
            })
            .with_authorization_keys(&[account_hash])
            .with_deploy_hash(deploy_hash)
            .build();

        ExecuteRequestBuilder::new()
            .push_deploy(deploy)
            .with_protocol_version(*NEW_PROTOCOL_VERSION)
            .build()
    };

    builder.exec(fund_request_3).expect_success().commit();

    let gas_cost_3 = builder.last_exec_gas_cost();

    assert!(gas_cost_3 > gas_cost_1, "{} <= {}", gas_cost_3, gas_cost_1);
    assert!(gas_cost_3 > gas_cost_2);

    let gas_cost_diff = gas_cost_3.checked_sub(gas_cost_2).unwrap_or_default();
    assert_eq!(
        gas_cost_diff,
        Gas::new(U512::from(HOST_FUNCTION_COST_CHANGE))
    );
}

struct TestContext {
    gh_2280_regression: AddressableEntityHash,
}

fn setup() -> (LmdbWasmTestBuilder, TestContext) {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let session_args = runtime_args! {
        mint::ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE),
        ARG_FAUCET_FUNDS => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE),
    };
    let install_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        GH_2280_REGRESSION_CONTRACT,
        session_args,
    )
    .build();

    builder.exec(install_request).expect_success().commit();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");
    let gh_2280_regression = account
        .named_keys()
        .get(HASH_KEY_NAME)
        .cloned()
        .and_then(Key::into_entity_hash_addr)
        .map(AddressableEntityHash::new)
        .expect("should have key");

    (builder, TestContext { gh_2280_regression })
}

fn make_engine_config(
    new_mint_costs: MintCosts,
    new_wasm_config: WasmConfig,
    old_system_config: SystemConfig,
) -> EngineConfig {
    let new_system_config = SystemConfig::new(
        old_system_config.wasmless_transfer_cost(),
        *old_system_config.auction_costs(),
        new_mint_costs,
        *old_system_config.handle_payment_costs(),
        *old_system_config.standard_payment_costs(),
    );
    EngineConfigBuilder::default()
        .with_wasm_config(new_wasm_config)
        .with_system_config(new_system_config)
        .build()
}

fn make_wasm_config(
    new_host_function_costs: HostFunctionCosts,
    old_wasm_config: WasmConfig,
) -> WasmConfig {
    WasmConfig::new(
        DEFAULT_WASM_MAX_MEMORY,
        DEFAULT_MAX_STACK_HEIGHT,
        old_wasm_config.opcode_costs(),
        old_wasm_config.storage_costs(),
        new_host_function_costs,
        old_wasm_config.messages_limits(),
    )
}

fn make_upgrade_request() -> ProtocolUpgradeConfig {
    UpgradeRequestBuilder::new()
        .with_current_protocol_version(*OLD_PROTOCOL_VERSION)
        .with_new_protocol_version(*NEW_PROTOCOL_VERSION)
        .with_activation_point(DEFAULT_ACTIVATION_POINT)
        .build()
}
