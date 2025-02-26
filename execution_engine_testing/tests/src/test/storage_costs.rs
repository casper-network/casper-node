use std::collections::{BTreeMap, BTreeSet};

use num_rational::Ratio;
use num_traits::Zero;
use once_cell::sync::Lazy;

#[cfg(not(feature = "use-as-wasm"))]
use casper_engine_test_support::DEFAULT_ACCOUNT_PUBLIC_KEY;
use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, UpgradeRequestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_PROTOCOL_VERSION, LOCAL_GENESIS_REQUEST,
};
#[cfg(not(feature = "use-as-wasm"))]
use casper_types::DEFAULT_ADD_BID_COST;
use casper_types::{
    bytesrepr::{Bytes, ToBytes},
    contracts::{ContractHash, ContractPackage, ContractVersionKey},
    AddressableEntityHash, BrTableCost, CLValue, ControlFlowCosts, EraId, Gas, Group, Groups,
    HostFunctionCosts, Key, MessageLimits, OpcodeCosts, ProtocolVersion, RuntimeArgs, StorageCosts,
    StoredValue, URef, WasmConfig, WasmV1Config, DEFAULT_V1_MAX_STACK_HEIGHT,
    DEFAULT_V1_WASM_MAX_MEMORY, U512,
};
#[cfg(not(feature = "use-as-wasm"))]
use casper_types::{
    runtime_args,
    system::{
        auction::{self, DelegationRate},
        AUCTION,
    },
};

const DEFAULT_ACTIVATION_POINT: EraId = EraId::new(0);
const STORAGE_COSTS_NAME: &str = "storage_costs.wasm";
#[cfg(not(feature = "use-as-wasm"))]
const SYSTEM_CONTRACT_HASHES_NAME: &str = "system_contract_hashes.wasm";
#[cfg(not(feature = "use-as-wasm"))]
const DO_NOTHING_WASM: &str = "do_nothing.wasm";
const CONTRACT_KEY_NAME: &str = "contract";

const WRITE_FUNCTION_SMALL_NAME: &str = "write_function_small";
const WRITE_FUNCTION_LARGE_NAME: &str = "write_function_large";
const ADD_FUNCTION_SMALL_NAME: &str = "add_function_small";
const ADD_FUNCTION_LARGE_NAME: &str = "add_function_large";
const NEW_UREF_FUNCTION: &str = "new_uref_function";
const PUT_KEY_FUNCTION: &str = "put_key_function";
const REMOVE_KEY_FUNCTION: &str = "remove_key_function";
const CREATE_CONTRACT_PACKAGE_AT_HASH_FUNCTION: &str = "create_contract_package_at_hash_function";
const CREATE_CONTRACT_USER_GROUP_FUNCTION_FUNCTION: &str = "create_contract_user_group_function";
const PROVISION_UREFS_FUNCTION: &str = "provision_urefs_function";
const REMOVE_CONTRACT_USER_GROUP_FUNCTION: &str = "remove_contract_user_group_function";
const NEW_UREF_SUBCALL_FUNCTION: &str = "new_uref_subcall";

const WRITE_SMALL_VALUE: &[u8] = b"1";
const WRITE_LARGE_VALUE: &[u8] = b"1111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111";

const ADD_SMALL_VALUE: u64 = 1;
const ADD_LARGE_VALUE: u64 = u64::MAX;

const NEW_OPCODE_COSTS: OpcodeCosts = OpcodeCosts {
    bit: 0,
    add: 0,
    mul: 0,
    div: 0,
    load: 0,
    store: 0,
    op_const: 0,
    local: 0,
    global: 0,
    control_flow: ControlFlowCosts {
        block: 0,
        op_loop: 0,
        op_if: 0,
        op_else: 0,
        end: 0,
        br: 0,
        br_if: 0,
        br_table: BrTableCost {
            cost: 0,
            size_multiplier: 0,
        },
        op_return: 0,
        call: 0,
        call_indirect: 0,
        drop: 0,
        select: 0,
    },
    integer_comparison: 0,
    conversion: 0,
    unreachable: 0,
    nop: 0,
    current_memory: 0,
    grow_memory: 0,
    sign: 0,
};

static NEW_HOST_FUNCTION_COSTS: Lazy<HostFunctionCosts> = Lazy::new(HostFunctionCosts::zero);
static NO_COSTS_WASM_CONFIG: Lazy<WasmConfig> = Lazy::new(|| {
    let wasm_v1_config = WasmV1Config::new(
        DEFAULT_V1_WASM_MAX_MEMORY,
        DEFAULT_V1_MAX_STACK_HEIGHT,
        NEW_OPCODE_COSTS,
        *NEW_HOST_FUNCTION_COSTS,
    );
    WasmConfig::new(MessageLimits::default(), wasm_v1_config)
});

static NEW_PROTOCOL_VERSION: Lazy<ProtocolVersion> = Lazy::new(|| {
    ProtocolVersion::from_parts(
        DEFAULT_PROTOCOL_VERSION.value().major,
        DEFAULT_PROTOCOL_VERSION.value().minor,
        DEFAULT_PROTOCOL_VERSION.value().patch + 1,
    )
});

/*
NOTE: in this test suite, to isolation specific micro function,
we are using specific costs that are not indicative of production values

Do not interpret statements in this test suite as global statements of fact
rather, they are self-reflective.

For instance, "should not charge for x" does not mean production usage would allow zero
cost host interaction. It only means in this controlled setup we have isolated that value
for fine grained testing.
*/

fn initialize_isolated_storage_costs() -> LmdbWasmTestBuilder {
    // This test runs a contract that's after every call extends the same key with
    // more data
    let mut builder = LmdbWasmTestBuilder::default();
    //
    // Isolate storage costs without host function costs, and without opcode costs
    //
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let mut upgrade_request = UpgradeRequestBuilder::new()
        .with_current_protocol_version(DEFAULT_PROTOCOL_VERSION)
        .with_new_protocol_version(*NEW_PROTOCOL_VERSION)
        .with_activation_point(DEFAULT_ACTIVATION_POINT)
        .build();

    let updated_chainspec = builder
        .chainspec()
        .clone()
        .with_wasm_config(*NO_COSTS_WASM_CONFIG);

    builder
        .with_chainspec(updated_chainspec)
        .upgrade(&mut upgrade_request);

    builder
}

#[cfg(not(feature = "use-as-wasm"))]
#[ignore]
#[test]
fn should_verify_isolate_host_side_payment_code_is_free() {
    let mut builder = initialize_isolated_storage_costs();

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        DO_NOTHING_WASM,
        RuntimeArgs::default(),
    )
    .build();

    let account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");
    let balance_before = builder.get_purse_balance(account.main_purse());

    let proposer_reward_starting_balance = builder.get_proposer_purse_balance();

    builder.exec(exec_request).expect_success().commit();

    let transaction_fee = builder.get_proposer_purse_balance() - proposer_reward_starting_balance;

    let balance_after = builder.get_purse_balance(account.main_purse());

    assert_eq!(
        balance_after,
        balance_before - transaction_fee,
        "balance before and after should match"
    );
    assert_eq!(builder.last_exec_gas_consumed().value(), U512::zero());
}

#[cfg(not(feature = "use-as-wasm"))]
#[ignore]
#[test]
fn should_verify_isolated_auction_storage_is_free() {
    const BOND_AMOUNT: u64 = 42;
    const DELEGATION_RATE: DelegationRate = 10;

    let mut builder = initialize_isolated_storage_costs();

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        SYSTEM_CONTRACT_HASHES_NAME,
        RuntimeArgs::default(),
    )
    .build();
    builder.exec(exec_request).expect_success().commit();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let bond_amount = U512::from(BOND_AMOUNT);

    let exec_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        account
            .named_keys()
            .get(AUCTION)
            .unwrap()
            .into_entity_hash_addr()
            .unwrap()
            .into(),
        auction::METHOD_ADD_BID,
        runtime_args! {
            auction::ARG_PUBLIC_KEY => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
            auction::ARG_AMOUNT => bond_amount,
            auction::ARG_DELEGATION_RATE => DELEGATION_RATE,
        },
    )
    .build();

    let balance_before = builder.get_purse_balance(account.main_purse());

    let proposer_reward_starting_balance = builder.get_proposer_purse_balance();

    builder.exec(exec_request).expect_success().commit();

    let balance_after = builder.get_purse_balance(account.main_purse());

    let transaction_fee = builder.get_proposer_purse_balance() - proposer_reward_starting_balance;

    let expected = balance_before - bond_amount - transaction_fee;

    assert_eq!(
        balance_after,
        expected,
        "before and after should match; off by: {}",
        expected - balance_after
    );
    assert_eq!(
        builder.last_exec_gas_consumed().value(),
        U512::from(DEFAULT_ADD_BID_COST)
    );
}

#[ignore]
#[test]
fn should_measure_gas_cost_for_storage_usage_write() {
    let cost_per_byte = U512::from(StorageCosts::default().gas_per_byte());

    let mut builder = initialize_isolated_storage_costs();

    let install_exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        STORAGE_COSTS_NAME,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(install_exec_request).expect_success().commit();

    assert!(!builder.last_exec_gas_consumed().value().is_zero());

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let contract_hash: AddressableEntityHash = account
        .named_keys()
        .get(CONTRACT_KEY_NAME)
        .expect("contract hash")
        .into_entity_hash()
        .expect("should be hash");

    //
    // Measure  small write
    //

    let small_write_function_cost = {
        let mut builder_a = builder.clone();

        let small_write_exec_request = ExecuteRequestBuilder::contract_call_by_hash(
            *DEFAULT_ACCOUNT_ADDR,
            contract_hash,
            WRITE_FUNCTION_SMALL_NAME,
            RuntimeArgs::default(),
        )
        .build();

        builder_a
            .exec(small_write_exec_request)
            .expect_success()
            .commit();

        builder_a.last_exec_gas_consumed()
    };

    let expected_small_write_data =
        StoredValue::from(CLValue::from_t(Bytes::from(WRITE_SMALL_VALUE.to_vec())).unwrap());

    let expected_small_cost = U512::from(expected_small_write_data.serialized_length());

    let small_write_cost = Ratio::new(small_write_function_cost.value(), cost_per_byte);

    assert_eq!(
        small_write_cost.fract().to_integer(),
        U512::zero(),
        "small cost does not divide without remainder"
    );
    assert!(
        small_write_cost.to_integer() >= expected_small_cost,
        "small write function call should cost at least the expected amount"
    );

    //
    // Measure large write
    //

    let large_write_function_cost = {
        let mut builder_b = builder;

        let large_write_exec_request = ExecuteRequestBuilder::contract_call_by_hash(
            *DEFAULT_ACCOUNT_ADDR,
            contract_hash,
            WRITE_FUNCTION_LARGE_NAME,
            RuntimeArgs::default(),
        )
        .build();

        builder_b
            .exec(large_write_exec_request)
            .expect_success()
            .commit();

        builder_b.last_exec_gas_consumed()
    };

    let expected_large_write_data =
        StoredValue::from(CLValue::from_t(Bytes::from(WRITE_LARGE_VALUE.to_vec())).unwrap());
    let expected_large_cost = U512::from(expected_large_write_data.serialized_length());

    let large_write_cost = Ratio::new(large_write_function_cost.value(), cost_per_byte);

    assert_eq!(
        large_write_cost.fract().to_integer(),
        U512::zero(),
        "cost does not divide without remainder"
    );
    assert!(
        large_write_cost.to_integer() >= expected_large_cost,
        "difference between large and small cost at least the expected write amount {}",
        expected_large_cost,
    );
}

#[ignore]
#[test]
fn should_measure_unisolated_gas_cost_for_storage_usage_write() {
    let cost_per_byte = U512::from(StorageCosts::default().gas_per_byte());

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let install_exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        STORAGE_COSTS_NAME,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(install_exec_request).expect_success().commit();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let contract_hash: AddressableEntityHash = account
        .named_keys()
        .get(CONTRACT_KEY_NAME)
        .expect("contract hash")
        .into_entity_hash()
        .expect("should be hash");

    //
    // Measure  small write
    //

    let small_write_function_cost = {
        let mut builder_a = builder.clone();

        let small_write_exec_request = ExecuteRequestBuilder::contract_call_by_hash(
            *DEFAULT_ACCOUNT_ADDR,
            contract_hash,
            WRITE_FUNCTION_SMALL_NAME,
            RuntimeArgs::default(),
        )
        .build();

        builder_a
            .exec(small_write_exec_request)
            .expect_success()
            .commit();

        builder_a.last_exec_gas_consumed()
    };

    let expected_small_write_data =
        StoredValue::from(CLValue::from_t(Bytes::from(WRITE_SMALL_VALUE.to_vec())).unwrap());

    let expected_small_cost = U512::from(expected_small_write_data.serialized_length());

    let small_write_cost = Ratio::new(small_write_function_cost.value(), cost_per_byte);

    assert_eq!(
        small_write_cost.fract().to_integer(),
        U512::zero(),
        "small cost does not divide without remainder"
    );
    assert!(
        small_write_cost.to_integer() >= expected_small_cost,
        "small write function call should cost at least the expected amount"
    );

    //
    // Measure large write
    //

    let large_write_function_cost = {
        let mut builder_b = builder;

        let large_write_exec_request = ExecuteRequestBuilder::contract_call_by_hash(
            *DEFAULT_ACCOUNT_ADDR,
            contract_hash,
            WRITE_FUNCTION_LARGE_NAME,
            RuntimeArgs::default(),
        )
        .build();

        builder_b
            .exec(large_write_exec_request)
            .expect_success()
            .commit();

        builder_b.last_exec_gas_consumed()
    };

    let expected_large_write_data =
        StoredValue::from(CLValue::from_t(Bytes::from(WRITE_LARGE_VALUE.to_vec())).unwrap());
    let expected_large_cost = U512::from(expected_large_write_data.serialized_length());

    let large_write_cost = Ratio::new(large_write_function_cost.value(), cost_per_byte);

    assert_eq!(
        large_write_cost.fract().to_integer(),
        U512::zero(),
        "cost does not divide without remainder"
    );
    assert!(
        large_write_cost.to_integer() >= expected_large_cost,
        "difference between large and small cost at least the expected write amount {}",
        expected_large_cost,
    );
}

#[ignore]
#[test]
fn should_measure_gas_cost_for_storage_usage_add() {
    let cost_per_byte = U512::from(StorageCosts::default().gas_per_byte());

    let mut builder = initialize_isolated_storage_costs();

    let install_exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        STORAGE_COSTS_NAME,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(install_exec_request).expect_success().commit();

    // let mut builder_a = builder.clone();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let contract_hash: AddressableEntityHash = account
        .named_keys()
        .get(CONTRACT_KEY_NAME)
        .expect("contract hash")
        .into_entity_hash()
        .expect("should be hash");

    //
    // Measure small add
    //

    let small_add_function_cost = {
        let mut builder_a = builder.clone();

        let small_add_exec_request = ExecuteRequestBuilder::contract_call_by_hash(
            *DEFAULT_ACCOUNT_ADDR,
            contract_hash,
            ADD_FUNCTION_SMALL_NAME,
            RuntimeArgs::default(),
        )
        .build();

        builder_a
            .exec(small_add_exec_request)
            .expect_success()
            .commit();

        builder_a.last_exec_gas_consumed()
    };

    let expected_small_add_data =
        StoredValue::from(CLValue::from_t(U512::from(ADD_SMALL_VALUE)).unwrap());

    let expected_small_cost = U512::from(expected_small_add_data.serialized_length());

    let small_add_cost = Ratio::new(small_add_function_cost.value(), cost_per_byte);

    assert_eq!(
        small_add_cost.fract().to_integer(),
        U512::zero(),
        "small cost does not divide without remainder"
    );
    assert!(
        small_add_cost.to_integer() >= expected_small_cost,
        "small write function call should cost at least the expected amount"
    );

    //
    // Measure large add
    //

    let large_add_function_cost = {
        let mut builder_b = builder;

        let large_write_exec_request = ExecuteRequestBuilder::contract_call_by_hash(
            *DEFAULT_ACCOUNT_ADDR,
            contract_hash,
            ADD_FUNCTION_LARGE_NAME,
            RuntimeArgs::default(),
        )
        .build();

        builder_b
            .exec(large_write_exec_request)
            .expect_success()
            .commit();

        builder_b.last_exec_gas_consumed()
    };

    let expected_large_write_data =
        StoredValue::from(CLValue::from_t(U512::from(ADD_LARGE_VALUE)).unwrap());
    let expected_large_cost = U512::from(expected_large_write_data.serialized_length());

    assert!(expected_large_cost > expected_small_cost);

    let large_write_cost = Ratio::new(large_add_function_cost.value(), cost_per_byte);

    assert_eq!(
        large_write_cost.fract().to_integer(),
        U512::zero(),
        "cost does not divide without remainder"
    );
    assert!(
        large_write_cost.to_integer() >= expected_large_cost,
        "difference between large and small cost at least the expected write amount {}",
        expected_large_cost,
    );
}

#[ignore]
#[test]
fn should_measure_unisolated_gas_cost_for_storage_usage_add() {
    let cost_per_byte = U512::from(StorageCosts::default().gas_per_byte());

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let install_exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        STORAGE_COSTS_NAME,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(install_exec_request).expect_success().commit();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let contract_hash: AddressableEntityHash = account
        .named_keys()
        .get(CONTRACT_KEY_NAME)
        .expect("contract hash")
        .into_entity_hash()
        .expect("should be hash");

    //
    // Measure small add
    //

    let small_add_function_cost = {
        let mut builder_a = builder.clone();

        let small_add_exec_request = ExecuteRequestBuilder::contract_call_by_hash(
            *DEFAULT_ACCOUNT_ADDR,
            contract_hash,
            ADD_FUNCTION_SMALL_NAME,
            RuntimeArgs::default(),
        )
        .build();

        builder_a
            .exec(small_add_exec_request)
            .expect_success()
            .commit();

        builder_a.last_exec_gas_consumed()
    };

    let expected_small_add_data =
        StoredValue::from(CLValue::from_t(U512::from(ADD_SMALL_VALUE)).unwrap());

    let expected_small_cost = U512::from(expected_small_add_data.serialized_length());

    let small_add_cost = Ratio::new(small_add_function_cost.value(), cost_per_byte);

    assert_eq!(
        small_add_cost.fract().to_integer(),
        U512::zero(),
        "small cost does not divide without remainder"
    );
    assert!(
        small_add_cost.to_integer() >= expected_small_cost,
        "small write function call should cost at least the expected amount"
    );

    //
    // Measure large add
    //

    let large_add_function_cost = {
        let mut builder_b = builder;

        let large_write_exec_request = ExecuteRequestBuilder::contract_call_by_hash(
            *DEFAULT_ACCOUNT_ADDR,
            contract_hash,
            ADD_FUNCTION_LARGE_NAME,
            RuntimeArgs::default(),
        )
        .build();

        builder_b
            .exec(large_write_exec_request)
            .expect_success()
            .commit();

        builder_b.last_exec_gas_consumed()
    };

    let expected_large_write_data =
        StoredValue::from(CLValue::from_t(U512::from(ADD_LARGE_VALUE)).unwrap());
    let expected_large_cost = U512::from(expected_large_write_data.serialized_length());

    assert!(expected_large_cost > expected_small_cost);

    let large_write_cost = Ratio::new(large_add_function_cost.value(), cost_per_byte);

    assert_eq!(
        large_write_cost.fract().to_integer(),
        U512::zero(),
        "cost does not divide without remainder"
    );
    assert!(
        large_write_cost.to_integer() >= expected_large_cost,
        "difference between large and small cost at least the expected write amount {}",
        expected_large_cost,
    );
}

#[ignore]
#[test]
fn should_verify_new_uref_storage_cost() {
    let mut builder = initialize_isolated_storage_costs();

    let install_exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        STORAGE_COSTS_NAME,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(install_exec_request).expect_success().commit();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let contract_hash: AddressableEntityHash = account
        .named_keys()
        .get(CONTRACT_KEY_NAME)
        .expect("contract hash")
        .into_entity_hash()
        .expect("should be hash");

    let exec_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        NEW_UREF_FUNCTION,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request).expect_success().commit();

    assert_eq!(
        // should charge for storage of a u64 behind a URef
        builder.last_exec_gas_consumed(),
        StorageCosts::default().calculate_gas_cost(
            StoredValue::CLValue(CLValue::from_t(0u64).expect("should create CLValue"))
                .serialized_length()
        )
    )
}

#[ignore]
#[test]
fn should_verify_put_key_is_charging_for_storage() {
    let mut builder = initialize_isolated_storage_costs();

    let install_exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        STORAGE_COSTS_NAME,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(install_exec_request).expect_success().commit();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let contract_hash: AddressableEntityHash = account
        .named_keys()
        .get(CONTRACT_KEY_NAME)
        .expect("contract hash")
        .into_entity_hash()
        .expect("should be hash");

    let exec_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        PUT_KEY_FUNCTION,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request).expect_success().commit();

    assert_eq!(
        // should charge for storage of a named key
        builder.last_exec_gas_consumed(),
        StorageCosts::default().calculate_gas_cost(
            StoredValue::CLValue(
                CLValue::from_t(("new_key".to_string(), Key::Hash([0u8; 32]))).unwrap()
            )
            .serialized_length()
        ),
    )
}

#[ignore]
#[test]
fn should_verify_remove_key_is_not_charging_for_storage() {
    let mut builder = initialize_isolated_storage_costs();

    let install_exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        STORAGE_COSTS_NAME,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(install_exec_request).expect_success().commit();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let contract_hash: AddressableEntityHash = account
        .named_keys()
        .get(CONTRACT_KEY_NAME)
        .expect("contract hash")
        .into_entity_hash()
        .expect("should be hash");

    let exec_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        REMOVE_KEY_FUNCTION,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request).expect_success().commit();

    if builder.chainspec().core_config.enable_addressable_entity {
        assert_eq!(
            // should charge zero, because we do not charge for storage when removing a key
            builder.last_exec_gas_consumed(),
            StorageCosts::default().calculate_gas_cost(0),
        )
    } else {
        assert!(builder.last_exec_gas_consumed() > Gas::zero())
    }
}

#[ignore]
#[test]
fn should_verify_create_contract_at_hash_is_charging_for_storage() {
    let mut builder = initialize_isolated_storage_costs();

    let install_exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        STORAGE_COSTS_NAME,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(install_exec_request).expect_success().commit();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let contract_hash: AddressableEntityHash = account
        .named_keys()
        .get(CONTRACT_KEY_NAME)
        .expect("contract hash")
        .into_entity_hash()
        .expect("should be hash");

    let exec_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        CREATE_CONTRACT_PACKAGE_AT_HASH_FUNCTION,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request).expect_success().commit();

    assert_eq!(
        // should charge at least enough for storage of a package and unit CLValue (for a URef)
        builder.last_exec_gas_consumed(),
        StorageCosts::default().calculate_gas_cost(
            StoredValue::ContractPackage(ContractPackage::default()).serialized_length()
                + StoredValue::CLValue(CLValue::unit()).serialized_length()
        )
    )
}

#[ignore]
#[test]
fn should_verify_create_contract_user_group_is_charging_for_storage() {
    let mut builder = initialize_isolated_storage_costs();

    let install_exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        STORAGE_COSTS_NAME,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(install_exec_request).expect_success().commit();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let contract_hash: AddressableEntityHash = account
        .named_keys()
        .get(CONTRACT_KEY_NAME)
        .expect("contract hash")
        .into_entity_hash()
        .expect("should be hash");

    let exec_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        CREATE_CONTRACT_USER_GROUP_FUNCTION_FUNCTION,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request).expect_success().commit();

    let mut groups = Groups::new();
    groups.insert(Group::new("Label"), BTreeSet::new());

    let mut package = ContractPackage::new(
        URef::default(),
        [(ContractVersionKey::new(2, 1), ContractHash::new([0u8; 32]))]
            .iter()
            .cloned()
            .collect::<BTreeMap<_, _>>(),
        Default::default(),
        groups,
        Default::default(),
    );

    assert_eq!(
        // should charge for storage of the new package
        builder.last_exec_gas_consumed(),
        StorageCosts::default()
            .calculate_gas_cost(StoredValue::ContractPackage(package.clone()).serialized_length()),
    );

    let exec_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        PROVISION_UREFS_FUNCTION,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request).expect_success().commit();

    package
        .groups_mut()
        .get_mut(&Group::new("Label"))
        .unwrap()
        .insert(URef::new([0u8; 32], Default::default()));

    assert!(
        // should charge for storage of the new package and a unit CLValue (for a URef)
        builder.last_exec_gas_consumed() > Gas::zero()
    );

    let exec_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        REMOVE_CONTRACT_USER_GROUP_FUNCTION,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request).expect_success().commit();

    package.remove_group(&Group::new("Label"));

    assert!(
        // should charge for storage of the new package
        builder.last_exec_gas_consumed() > Gas::zero()
    )
}

#[ignore]
#[test]
fn should_verify_subcall_new_uref_is_charging_for_storage() {
    let mut builder = initialize_isolated_storage_costs();

    let install_exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        STORAGE_COSTS_NAME,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(install_exec_request).expect_success().commit();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let contract_hash: AddressableEntityHash = account
        .named_keys()
        .get(CONTRACT_KEY_NAME)
        .expect("contract hash")
        .into_entity_hash()
        .expect("should be hash");

    let exec_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        CREATE_CONTRACT_USER_GROUP_FUNCTION_FUNCTION,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request).expect_success().commit();

    let exec_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        PROVISION_UREFS_FUNCTION,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request).expect_success().commit();

    let exec_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        NEW_UREF_SUBCALL_FUNCTION,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request).expect_success().commit();

    assert_eq!(
        // should charge for storage of a u64 behind a URef
        builder.last_exec_gas_consumed(),
        StorageCosts::default().calculate_gas_cost(
            StoredValue::CLValue(CLValue::from_t(0u64).expect("should create CLValue"))
                .serialized_length()
        )
    )
}
