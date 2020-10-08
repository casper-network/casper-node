use casper_engine_grpc_server::engine_server::ipc::DeployCode;
use casper_engine_test_support::internal::{
    utils, InMemoryWasmTestBuilder, UpgradeRequestBuilder, DEFAULT_RUN_GENESIS_REQUEST,
    DEFAULT_WASM_CONFIG,
};
#[cfg(feature = "use-system-contracts")]
use casper_engine_test_support::{internal::ExecuteRequestBuilder, DEFAULT_ACCOUNT_ADDR};
#[cfg(feature = "use-system-contracts")]
use casper_execution_engine::shared::{stored_value::StoredValue, transform::Transform};
use casper_execution_engine::{
    core::engine_state::{upgrade::ActivationPoint, Error},
    shared::{opcode_costs::OpCodeCosts, storage_costs::StorageCosts, wasm_config::WasmConfig},
};
use casper_types::ProtocolVersion;
#[cfg(feature = "use-system-contracts")]
use casper_types::{runtime_args, CLValue, Key, RuntimeArgs, U512};

const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion::V1_0_0;
const DEFAULT_ACTIVATION_POINT: ActivationPoint = 1;
const MODIFIED_SYSTEM_UPGRADER_CONTRACT_NAME: &str = "modified_system_upgrader.wasm";
#[cfg(feature = "use-system-contracts")]
const MODIFIED_MINT_CALLER_CONTRACT_NAME: &str = "modified_mint_caller.wasm";
#[cfg(feature = "use-system-contracts")]
const PAYMENT_AMOUNT: u64 = 200_000_000;
#[cfg(feature = "use-system-contracts")]
const ARG_TARGET: &str = "target";

fn get_upgraded_wasm_config() -> WasmConfig {
    WasmConfig {
        initial_mem: 64,
        max_stack_height: 64 * 1024,
        opcode_costs: OpCodeCosts {
            bit: 2,
            add: 2,
            mul: 2,
            div: 2,
            load: 2,
            store: 2,
            op_const: 2,
            local: 2,
            global: 2,
            control_flow: 2,
            integer_comparsion: 2,
            conversion: 2,
            unreachable: 2,
            nop: 2,
            current_memory: 2,
            grow_memory: 8192,
            regular: 2,
        },
        storage_costs: StorageCosts {
            gas_per_byte: 630_000_000,
        },
        host_function_costs: Default::default(),
    }
}

#[ignore]
#[test]
fn should_upgrade_only_protocol_version() {
    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let sem_ver = PROTOCOL_VERSION.value();
    let new_protocol_version =
        ProtocolVersion::from_parts(sem_ver.major, sem_ver.minor, sem_ver.patch + 1);

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(PROTOCOL_VERSION)
            .with_new_protocol_version(new_protocol_version)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .build()
    };

    builder.upgrade_with_upgrade_request(&mut upgrade_request);

    let upgrade_response = builder
        .get_upgrade_response(0)
        .expect("should have response");

    assert!(upgrade_response.has_success(), "expected success");

    let upgraded_wasm_costs = builder
        .get_engine_state()
        .wasm_config(new_protocol_version)
        .expect("should have result")
        .expect("should have costs");

    assert_eq!(
        *DEFAULT_WASM_CONFIG, upgraded_wasm_costs,
        "upgraded costs should equal original costs"
    );
}

#[cfg(feature = "use-system-contracts")]
#[ignore]
#[test]
fn should_upgrade_system_contract() {
    let mut builder = InMemoryWasmTestBuilder::default();

    let new_protocol_version = ProtocolVersion::from_parts(2, 0, 0);

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let mut upgrade_request = {
        let bytes = utils::read_wasm_file_bytes(MODIFIED_SYSTEM_UPGRADER_CONTRACT_NAME);
        let mut installer_code = DeployCode::new();
        installer_code.set_code(bytes);
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(PROTOCOL_VERSION)
            .with_new_protocol_version(new_protocol_version)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .with_installer_code(installer_code)
            .build()
    };

    builder.upgrade_with_upgrade_request(&mut upgrade_request);

    let upgrade_response = builder
        .get_upgrade_response(0)
        .expect("should have response");

    assert!(
        upgrade_response.has_success(),
        "upgrade_response expected success"
    );

    let exec_request = {
        ExecuteRequestBuilder::standard(
            *DEFAULT_ACCOUNT_ADDR,
            &MODIFIED_MINT_CALLER_CONTRACT_NAME,
            runtime_args! { "amount" => U512::from(PAYMENT_AMOUNT) },
        )
        .with_protocol_version(new_protocol_version)
        .build()
    };

    builder.exec(exec_request).expect_success();

    let transforms = builder.get_transforms();
    let transform = &transforms[0];

    let new_keys = if let Some(Transform::AddKeys(keys)) =
        transform.get(&Key::Account(*DEFAULT_ACCOUNT_ADDR))
    {
        keys
    } else {
        panic!(
            "expected AddKeys transform for given key but received {:?}",
            transforms[0]
        );
    };

    let version_uref = new_keys
        .get("output_version")
        .expect("version_uref should exist");

    builder.commit();

    let version_value: StoredValue = builder
        .query(None, *version_uref, &[])
        .expect("should find version_uref value");

    assert_eq!(
        version_value,
        StoredValue::CLValue(CLValue::from_t("1.1.0".to_string()).unwrap()),
        "expected new version endpoint output"
    );
}

#[cfg(feature = "use-system-contracts")]
#[ignore]
#[test]
fn should_upgrade_system_contract_on_patch_bump() {
    let mut builder = InMemoryWasmTestBuilder::default();

    let sem_ver = PROTOCOL_VERSION.value();

    let new_protocol_version =
        ProtocolVersion::from_parts(sem_ver.major, sem_ver.minor, sem_ver.patch + 123);

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let mut upgrade_request = {
        let bytes = utils::read_wasm_file_bytes(MODIFIED_SYSTEM_UPGRADER_CONTRACT_NAME);
        let mut installer_code = DeployCode::new();
        installer_code.set_code(bytes);
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(PROTOCOL_VERSION)
            .with_new_protocol_version(new_protocol_version)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .with_installer_code(installer_code)
            .build()
    };

    builder.upgrade_with_upgrade_request(&mut upgrade_request);

    let upgrade_response = builder
        .get_upgrade_response(0)
        .expect("should have response");

    assert!(
        upgrade_response.has_success(),
        "upgrade_response expected success"
    );

    let exec_request = {
        ExecuteRequestBuilder::standard(
            *DEFAULT_ACCOUNT_ADDR,
            &MODIFIED_MINT_CALLER_CONTRACT_NAME,
            runtime_args! { ARG_TARGET => U512::from(PAYMENT_AMOUNT) },
        )
        .with_protocol_version(new_protocol_version)
        .build()
    };

    builder.exec(exec_request).expect_success();

    let transforms = builder.get_transforms();
    let transform = &transforms[0];

    let new_keys = if let Some(Transform::AddKeys(keys)) =
        transform.get(&Key::Account(*DEFAULT_ACCOUNT_ADDR))
    {
        keys
    } else {
        panic!(
            "expected AddKeys transform for given key but received {:?}",
            transforms[0]
        );
    };

    let version_uref = new_keys
        .get("output_version")
        .expect("version_uref should exist");

    builder.commit();

    let version_value = builder
        .query(None, *version_uref, &[])
        .expect("should find version_uref value");

    assert_eq!(
        version_value,
        StoredValue::CLValue(CLValue::from_t("1.1.0").unwrap()),
        "expected new version endpoint output"
    );
}

#[cfg(feature = "use-system-contracts")]
#[ignore]
#[test]
fn should_upgrade_system_contract_on_minor_bump() {
    let mut builder = InMemoryWasmTestBuilder::default();

    let sem_ver = PROTOCOL_VERSION.value();

    let new_protocol_version =
        ProtocolVersion::from_parts(sem_ver.major, sem_ver.minor + 1, sem_ver.patch);

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let mut upgrade_request = {
        let bytes = utils::read_wasm_file_bytes(MODIFIED_SYSTEM_UPGRADER_CONTRACT_NAME);
        let mut installer_code = DeployCode::new();
        installer_code.set_code(bytes);
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(PROTOCOL_VERSION)
            .with_new_protocol_version(new_protocol_version)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .with_installer_code(installer_code)
            .build()
    };

    builder.upgrade_with_upgrade_request(&mut upgrade_request);

    let upgrade_response = builder
        .get_upgrade_response(0)
        .expect("should have response");

    assert!(
        upgrade_response.has_success(),
        "upgrade_response expected success"
    );

    let exec_request = {
        ExecuteRequestBuilder::standard(
            *DEFAULT_ACCOUNT_ADDR,
            &MODIFIED_MINT_CALLER_CONTRACT_NAME,
            runtime_args! {ARG_TARGET => U512::from(PAYMENT_AMOUNT) },
        )
        .with_protocol_version(new_protocol_version)
        .build()
    };

    builder.exec(exec_request).expect_success();

    let transforms = builder.get_transforms();
    let transform = &transforms[0];

    let new_keys = if let Some(Transform::AddKeys(keys)) =
        transform.get(&Key::Account(*DEFAULT_ACCOUNT_ADDR))
    {
        keys
    } else {
        panic!(
            "expected AddKeys transform for given key but received {:?}",
            transforms[0]
        );
    };

    let version_uref = new_keys
        .get("output_version")
        .expect("version_uref should exist");

    builder.commit();

    let version_value = builder
        .query(None, *version_uref, &[])
        .expect("should find version_uref value");

    assert_eq!(
        version_value,
        StoredValue::CLValue(CLValue::from_t("1.1.0").unwrap()),
        "expected new version endpoint output"
    );
}

#[ignore]
#[test]
fn should_allow_only_wasm_costs_patch_version() {
    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let sem_ver = PROTOCOL_VERSION.value();
    let new_protocol_version =
        ProtocolVersion::from_parts(sem_ver.major, sem_ver.minor, sem_ver.patch + 2);

    let new_wasm_config = get_upgraded_wasm_config();

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(PROTOCOL_VERSION)
            .with_new_protocol_version(new_protocol_version)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .with_new_wasm_config(new_wasm_config.clone())
            .build()
    };

    builder.upgrade_with_upgrade_request(&mut upgrade_request);

    let upgrade_response = builder
        .get_upgrade_response(0)
        .expect("should have response");

    assert!(upgrade_response.has_success(), "expected success");

    let upgraded_wasm_config = builder
        .get_engine_state()
        .wasm_config(new_protocol_version)
        .expect("should have result")
        .expect("should have upgraded costs");

    assert_eq!(
        new_wasm_config, upgraded_wasm_config,
        "upgraded costs should equal new costs"
    );
}

#[ignore]
#[test]
fn should_allow_only_wasm_costs_minor_version() {
    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let sem_ver = PROTOCOL_VERSION.value();
    let new_protocol_version =
        ProtocolVersion::from_parts(sem_ver.major, sem_ver.minor + 1, sem_ver.patch);

    let new_wasm_config = get_upgraded_wasm_config();

    let mut upgrade_request = {
        let bytes = utils::read_wasm_file_bytes(MODIFIED_SYSTEM_UPGRADER_CONTRACT_NAME);
        let mut installer_code = DeployCode::new();
        installer_code.set_code(bytes);
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(PROTOCOL_VERSION)
            .with_new_protocol_version(new_protocol_version)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .with_new_wasm_config(new_wasm_config.clone())
            .with_installer_code(installer_code)
            .build()
    };

    builder.upgrade_with_upgrade_request(&mut upgrade_request);

    let upgrade_response = builder
        .get_upgrade_response(0)
        .expect("should have response");

    assert!(
        upgrade_response.has_success(),
        "expected success {:?}",
        upgrade_response
    );

    let upgraded_wasm_costs = builder
        .get_engine_state()
        .wasm_config(new_protocol_version)
        .expect("should have result")
        .expect("should have upgraded costs");

    assert_eq!(
        new_wasm_config, upgraded_wasm_costs,
        "upgraded costs should equal new costs"
    );
}

#[cfg(feature = "use-system-contracts")]
#[ignore]
#[test]
fn should_upgrade_system_contract_and_wasm_costs_major() {
    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let new_protocol_version = ProtocolVersion::from_parts(2, 0, 0);

    let new_wasm_config = get_upgraded_wasm_config();

    let mut upgrade_request = {
        let bytes = utils::read_wasm_file_bytes(MODIFIED_SYSTEM_UPGRADER_CONTRACT_NAME);
        let mut installer_code = DeployCode::new();
        installer_code.set_code(bytes);
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(PROTOCOL_VERSION)
            .with_new_protocol_version(new_protocol_version)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .with_installer_code(installer_code)
            .with_new_wasm_config(new_wasm_config.clone())
            .build()
    };

    builder.upgrade_with_upgrade_request(&mut upgrade_request);

    let upgrade_response = builder
        .get_upgrade_response(0)
        .expect("should have response");

    assert!(upgrade_response.has_success(), "expected success");

    let exec_request = {
        ExecuteRequestBuilder::standard(
            *DEFAULT_ACCOUNT_ADDR,
            &MODIFIED_MINT_CALLER_CONTRACT_NAME,
            runtime_args! {ARG_TARGET => U512::from(PAYMENT_AMOUNT) },
        )
        .with_protocol_version(new_protocol_version)
        .build()
    };

    builder.exec(exec_request).expect_success();

    let transforms = builder.get_transforms();
    let transform = &transforms[0];

    let new_keys = if let Some(Transform::AddKeys(keys)) =
        transform.get(&Key::Account(*DEFAULT_ACCOUNT_ADDR))
    {
        keys
    } else {
        panic!(
            "expected AddKeys transform for given key but received {:?}",
            transforms[0]
        );
    };

    let version_uref = new_keys
        .get("output_version")
        .expect("version_uref should exist");

    builder.commit();

    let version_value: StoredValue = builder
        .query(None, *version_uref, &[])
        .expect("should find version_uref value");

    assert_eq!(
        version_value,
        StoredValue::CLValue(CLValue::from_t("1.1.0".to_string()).unwrap()),
        "expected new version endpoint output"
    );

    let upgraded_wasm_costs = builder
        .get_engine_state()
        .wasm_config(new_protocol_version)
        .expect("should have result")
        .expect("should have upgraded costs");

    assert_eq!(
        new_wasm_config, upgraded_wasm_costs,
        "upgraded costs should equal new costs"
    );
}

#[ignore]
#[test]
fn should_not_downgrade() {
    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let new_protocol_version = ProtocolVersion::from_parts(2, 0, 0);

    let mut upgrade_request = {
        let bytes = utils::read_wasm_file_bytes(MODIFIED_SYSTEM_UPGRADER_CONTRACT_NAME);
        let mut installer_code = DeployCode::new();
        installer_code.set_code(bytes);
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(PROTOCOL_VERSION)
            .with_new_protocol_version(new_protocol_version)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .with_installer_code(installer_code)
            .build()
    };

    builder.upgrade_with_upgrade_request(&mut upgrade_request);

    let upgrade_response = builder
        .get_upgrade_response(0)
        .expect("should have response");

    assert!(
        upgrade_response.has_success(),
        "expected success but received {:?}",
        upgrade_response
    );

    let upgraded_wasm_costs = builder
        .get_engine_state()
        .wasm_config(new_protocol_version)
        .expect("should have result")
        .expect("should have costs");

    assert_eq!(
        *DEFAULT_WASM_CONFIG, upgraded_wasm_costs,
        "upgraded costs should equal original costs"
    );

    let mut downgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(new_protocol_version)
            .with_new_protocol_version(PROTOCOL_VERSION)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .build()
    };

    builder.upgrade_with_upgrade_request(&mut downgrade_request);

    let upgrade_response = builder
        .get_upgrade_response(1)
        .expect("should have response");

    assert!(!upgrade_response.has_success(), "expected failure");
}

#[ignore]
#[test]
fn should_not_skip_major_versions() {
    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let sem_ver = PROTOCOL_VERSION.value();

    let invalid_version =
        ProtocolVersion::from_parts(sem_ver.major + 2, sem_ver.minor, sem_ver.patch);

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(PROTOCOL_VERSION)
            .with_new_protocol_version(invalid_version)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .build()
    };

    builder.upgrade_with_upgrade_request(&mut upgrade_request);

    let upgrade_response = builder
        .get_upgrade_response(0)
        .expect("should have response");

    assert!(!upgrade_response.has_success(), "expected failure");
}

#[ignore]
#[test]
fn should_not_skip_minor_versions() {
    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let sem_ver = PROTOCOL_VERSION.value();

    let invalid_version =
        ProtocolVersion::from_parts(sem_ver.major, sem_ver.minor + 2, sem_ver.patch);

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(PROTOCOL_VERSION)
            .with_new_protocol_version(invalid_version)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .build()
    };

    builder.upgrade_with_upgrade_request(&mut upgrade_request);

    let upgrade_response = builder
        .get_upgrade_response(0)
        .expect("should have response");

    assert!(!upgrade_response.has_success(), "expected failure");
}

#[ignore]
#[test]
fn should_fail_major_upgrade_without_installer() {
    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let sem_ver = PROTOCOL_VERSION.value();

    let invalid_version =
        ProtocolVersion::from_parts(sem_ver.major + 1, sem_ver.minor, sem_ver.patch);

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(PROTOCOL_VERSION)
            .with_new_protocol_version(invalid_version)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .build()
    };

    builder.upgrade_with_upgrade_request(&mut upgrade_request);

    let upgrade_response = builder
        .get_upgrade_response(0)
        .expect("should have response");

    assert!(
        upgrade_response.has_failed_deploy(),
        "should have failed deploy"
    );

    let failed_deploy = upgrade_response.get_failed_deploy();
    assert_eq!(
        failed_deploy.message,
        Error::InvalidUpgradeConfig.to_string()
    );
}
