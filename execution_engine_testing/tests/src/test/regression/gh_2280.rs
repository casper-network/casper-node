use once_cell::sync::Lazy;

use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, InMemoryWasmTestBuilder, UpgradeRequestBuilder,
    DEFAULT_ACCOUNT_ADDR, DEFAULT_MAX_ASSOCIATED_KEYS, DEFAULT_PROTOCOL_VERSION,
    DEFAULT_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::{
    core::engine_state::{EngineConfig, DEFAULT_MAX_QUERY_DEPTH},
    shared::{
        host_function_costs::{Cost, HostFunction, HostFunctionCosts},
        opcode_costs::OpcodeCosts,
        storage_costs::StorageCosts,
        system_config::{
            auction_costs::AuctionCosts, handle_payment_costs::HandlePaymentCosts,
            mint_costs::MintCosts, standard_payment_costs::StandardPaymentCosts, SystemConfig,
            DEFAULT_WASMLESS_TRANSFER_COST,
        },
        wasm_config::{WasmConfig, DEFAULT_MAX_STACK_HEIGHT, DEFAULT_WASM_MAX_MEMORY},
    },
};
use casper_types::{
    account::AccountHash, runtime_args, EraId, Gas, Motes, ProtocolVersion, PublicKey, RuntimeArgs,
    SecretKey, U512,
};

const TRANSFER_TO_ACCOUNT_CONTRACT: &str = "transfer_to_account.wasm";
const CREATE_PURSE_01_CONTRACT: &str = "create_purse_01.wasm";

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

const HOST_FUNCTION_COST_CHANGE: Cost = 13_730_593; // random prime number

#[ignore]
#[test]
fn gh_2280_transfer_should_always_cost_the_same_gas() {
    let mut builder = setup();

    let faucet_args_1 = runtime_args! {
        ARG_TARGET => *ACCOUNT_1_ADDR,
        ARG_AMOUNT => TOKEN_AMOUNT,
    };

    let fund_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        TRANSFER_TO_ACCOUNT_CONTRACT,
        faucet_args_1,
    )
    .build();
    builder.exec(fund_request_1).expect_success().commit();

    let gas_cost_1 = builder.last_exec_gas_cost();

    // Next time pay exactly the amount that was reported which should be also the minimum you
    // should be able to pay next time.
    let payment_amount = Motes::from_gas(gas_cost_1, 1).unwrap();

    let fund_request_2 = {
        let account_hash = *DEFAULT_ACCOUNT_ADDR;
        let deploy_hash: [u8; 32] = [55; 32];
        let faucet_args_2 = runtime_args! {
            ARG_TARGET => *ACCOUNT_2_ADDR,
            ARG_AMOUNT => TOKEN_AMOUNT,
        };

        let deploy = DeployItemBuilder::new()
            .with_address(account_hash)
            .with_session_code(TRANSFER_TO_ACCOUNT_CONTRACT, faucet_args_2)
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

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(*OLD_PROTOCOL_VERSION)
            .with_new_protocol_version(*NEW_PROTOCOL_VERSION)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .build()
    };

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

    let new_wasm_config = WasmConfig::new(
        DEFAULT_WASM_MAX_MEMORY,
        DEFAULT_MAX_STACK_HEIGHT,
        OpcodeCosts::default(),
        StorageCosts::default(),
        new_host_function_costs,
    );

    // Inflate affected system contract entry point cost to the maximum
    let new_mint_create_cost = u32::MAX;
    let new_mint_costs = MintCosts {
        create: new_mint_create_cost,
        ..Default::default()
    };

    let new_system_config = SystemConfig::new(
        DEFAULT_WASMLESS_TRANSFER_COST,
        AuctionCosts::default(),
        new_mint_costs,
        HandlePaymentCosts::default(),
        StandardPaymentCosts::default(),
    );

    let new_engine_config = EngineConfig::new(
        DEFAULT_MAX_QUERY_DEPTH,
        DEFAULT_MAX_ASSOCIATED_KEYS,
        new_wasm_config,
        new_system_config,
    );

    builder.upgrade_with_upgrade_request(new_engine_config, &mut upgrade_request);

    let fund_request_3 = {
        let account_hash = *DEFAULT_ACCOUNT_ADDR;
        let deploy_hash: [u8; 32] = [77; 32];
        let faucet_args_3 = runtime_args! {
            ARG_TARGET => *ACCOUNT_3_ADDR,
            ARG_AMOUNT => TOKEN_AMOUNT,
        };

        let deploy = DeployItemBuilder::new()
            .with_address(account_hash)
            .with_session_code(TRANSFER_TO_ACCOUNT_CONTRACT, faucet_args_3)
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
    let mut builder = setup();

    let create_purse_args_1 = runtime_args! {
        ARG_PURSE_NAME => TEST_PURSE_NAME
    };

    let fund_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CREATE_PURSE_01_CONTRACT,
        create_purse_args_1,
    )
    .build();
    builder.exec(fund_request_1).expect_success().commit();

    let gas_cost_1 = builder.last_exec_gas_cost();

    // Next time pay exactly the amount that was reported which should be also the minimum you
    // should be able to pay next time.
    let payment_amount = Motes::from_gas(gas_cost_1, 1).unwrap();

    let fund_request_2 = {
        let account_hash = *DEFAULT_ACCOUNT_ADDR;
        let deploy_hash: [u8; 32] = [55; 32];
        let create_purse_args_2 = runtime_args! {
            ARG_PURSE_NAME => TEST_PURSE_NAME,
        };

        let deploy = DeployItemBuilder::new()
            .with_address(account_hash)
            .with_session_code(CREATE_PURSE_01_CONTRACT, create_purse_args_2)
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

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(*OLD_PROTOCOL_VERSION)
            .with_new_protocol_version(*NEW_PROTOCOL_VERSION)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .build()
    };

    // Increase "transfer_to_account" host function call exactly by X, so we can assert that
    // transfer cost increased by exactly X without hidden fees.
    let default_host_function_costs = HostFunctionCosts::default();

    let default_create_purse_cost = default_host_function_costs.create_purse.cost();
    let new_create_purse_cost = default_create_purse_cost
        .checked_add(HOST_FUNCTION_COST_CHANGE)
        .expect("should add without overflow");
    let new_create_purse = HostFunction::fixed(new_create_purse_cost);

    let new_host_function_costs = HostFunctionCosts {
        create_purse: new_create_purse,
        ..default_host_function_costs
    };

    let new_wasm_config = WasmConfig::new(
        DEFAULT_WASM_MAX_MEMORY,
        DEFAULT_MAX_STACK_HEIGHT,
        OpcodeCosts::default(),
        StorageCosts::default(),
        new_host_function_costs,
    );

    // Inflate affected system contract entry point cost to the maximum
    let new_mint_create_cost = u32::MAX;
    let new_mint_costs = MintCosts {
        create: new_mint_create_cost,
        ..Default::default()
    };

    let new_system_config = SystemConfig::new(
        DEFAULT_WASMLESS_TRANSFER_COST,
        AuctionCosts::default(),
        new_mint_costs,
        HandlePaymentCosts::default(),
        StandardPaymentCosts::default(),
    );

    let new_engine_config = EngineConfig::new(
        DEFAULT_MAX_QUERY_DEPTH,
        DEFAULT_MAX_ASSOCIATED_KEYS,
        new_wasm_config,
        new_system_config,
    );

    builder.upgrade_with_upgrade_request(new_engine_config, &mut upgrade_request);

    let fund_request_3 = {
        let account_hash = *DEFAULT_ACCOUNT_ADDR;
        let deploy_hash: [u8; 32] = [77; 32];
        let create_purse_args_3 = runtime_args! {
            ARG_PURSE_NAME => TEST_PURSE_NAME,
        };

        let deploy = DeployItemBuilder::new()
            .with_address(account_hash)
            .with_session_code(CREATE_PURSE_01_CONTRACT, create_purse_args_3)
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

fn setup() -> InMemoryWasmTestBuilder {
    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&*DEFAULT_RUN_GENESIS_REQUEST);
    builder
}
