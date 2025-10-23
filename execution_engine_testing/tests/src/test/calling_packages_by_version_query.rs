use std::collections::BTreeSet;

/// This test assumes that the provided fixture has v1 and v2 installed in protocol version 1
use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, UpgradeRequestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_EXEC_CONFIG, DEFAULT_GENESIS_CONFIG_HASH, DEFAULT_PROTOCOL_VERSION,
};
use casper_execution_engine::{
    engine_state::{EngineConfigBuilder, Error, SessionDataV1, SessionInputData},
    execution::ExecError,
};
use casper_storage::data_access_layer::GenesisRequest;
use casper_types::{
    contracts::ProtocolVersionMajor, runtime_args, AddressableEntityHash, ChainspecRegistry,
    EntityVersion, EntityVersionKey, EraId, HashAddr, HoldBalanceHandling, Key, NamedKeys,
    PackageAddr, PricingMode, ProtocolVersion, RuntimeArgs, StoredValue, Timestamp,
    TransactionEntryPoint, TransactionInvocationTarget, TransactionRuntimeParams,
    TransactionTarget, TransactionV1Hash,
};
use once_cell::sync::Lazy;
use rand::Rng;

static V3_0_0: Lazy<ProtocolVersion> = Lazy::new(|| ProtocolVersion::from_parts(3, 0, 0));

const DISABLE_CONTRACT: &str = "disable_contract.wasm";

static CURRENT_PROTOCOL_MAJOR: Lazy<u32> = Lazy::new(|| DEFAULT_PROTOCOL_VERSION.value().major);

const CONTRACT_WASM: &str = "key_putter.wasm";

#[ignore]
#[test]
fn should_call_package_hash_by_exact_version() {
    let mut builder = prepare_v1_builder();
    install(&mut builder, CONTRACT_WASM, ProtocolVersion::V1_0_0);
    install(&mut builder, CONTRACT_WASM, ProtocolVersion::V1_0_0);

    exec_put_key_by_package_hash(&mut builder, Some(1), Some(1), ProtocolVersion::V1_0_0);

    let hash_of_1_1 = get_contract_hash_for_specific_version(&mut builder, 1, 1).unwrap();
    let value = get_value_of_named_key_for_contract_hash_as_str(
        &mut builder,
        hash_of_1_1,
        "key_placeholder",
    )
    .unwrap();
    assert_eq!(value, "key_putter_v1");
}

#[ignore]
#[test]
fn should_call_package_name_by_exact_version() {
    let mut builder = prepare_v1_builder();
    install(&mut builder, CONTRACT_WASM, ProtocolVersion::V1_0_0);
    install(&mut builder, CONTRACT_WASM, ProtocolVersion::V1_0_0);

    exec_put_key_by_package_name(&mut builder, Some(1), Some(1), ProtocolVersion::V1_0_0);

    let hash_of_1_1 = get_contract_hash_for_specific_version(&mut builder, 1, 1).unwrap();
    let value = get_value_of_named_key_for_contract_hash_as_str(
        &mut builder,
        hash_of_1_1,
        "key_placeholder",
    )
    .unwrap();
    assert_eq!(value, "key_putter_v1");
}

#[ignore]
#[test]
fn should_call_package_hash_by_exact_version_after_protocol_version_change() {
    let mut builder = prepare_v1_builder();
    install(&mut builder, CONTRACT_WASM, ProtocolVersion::V1_0_0);
    install(&mut builder, CONTRACT_WASM, ProtocolVersion::V1_0_0);

    upgrade_version(&mut builder, 2, false);
    install(&mut builder, CONTRACT_WASM, ProtocolVersion::V2_0_0);

    exec_put_key_by_package_hash(&mut builder, Some(1), Some(1), ProtocolVersion::V2_0_0);

    let hash_of_1_1 = get_contract_hash_for_specific_version(&mut builder, 1, 1).unwrap();
    let value = get_value_of_named_key_for_contract_hash_as_str(
        &mut builder,
        hash_of_1_1,
        "key_placeholder",
    )
    .unwrap();
    assert_eq!(value, "key_putter_v1");
}

#[ignore]
#[test]
fn should_call_package_name_by_exact_version_after_protocol_version_change() {
    let mut builder = prepare_v1_builder();
    install(&mut builder, CONTRACT_WASM, ProtocolVersion::V1_0_0);
    install(&mut builder, CONTRACT_WASM, ProtocolVersion::V1_0_0);
    upgrade_version(&mut builder, 2, false);

    exec_put_key_by_package_name(&mut builder, Some(1), Some(1), ProtocolVersion::V2_0_0);

    let hash_of_1_1 = get_contract_hash_for_specific_version(&mut builder, 1, 1).unwrap();
    let value = get_value_of_named_key_for_contract_hash_as_str(
        &mut builder,
        hash_of_1_1,
        "key_placeholder",
    )
    .unwrap();
    assert_eq!(value, "key_putter_v1");
}

#[ignore]
#[test]
fn should_call_by_hash_newest_version_when_only_major_specified() {
    let mut builder = prepare_v1_builder();
    install(&mut builder, CONTRACT_WASM, ProtocolVersion::V1_0_0);
    install(&mut builder, CONTRACT_WASM, ProtocolVersion::V1_0_0);
    upgrade_version(&mut builder, 2, false);
    install(&mut builder, CONTRACT_WASM, ProtocolVersion::V2_0_0);

    exec_put_key_by_package_hash(&mut builder, Some(1), None, ProtocolVersion::V2_0_0);

    let hash_of_1_2 = get_contract_hash_for_specific_version(&mut builder, 1, 2).unwrap();
    let value = get_value_of_named_key_for_contract_hash_as_str(
        &mut builder,
        hash_of_1_2,
        "key_placeholder",
    )
    .unwrap();
    assert_eq!(value, "key_putter_v2");

    disable_contract_version(&mut builder, 1, 2);
    // After disabling 1.2, selecting by major protocol 1 should point to 1.1

    exec_put_key_by_package_hash(&mut builder, Some(1), None, ProtocolVersion::V2_0_0);

    let hash_of_1_1 = get_contract_hash_for_specific_version(&mut builder, 1, 1).unwrap();
    let value = get_value_of_named_key_for_contract_hash_as_str(
        &mut builder,
        hash_of_1_1,
        "key_placeholder",
    )
    .unwrap();
    assert_eq!(value, "key_putter_v1");
}

#[ignore]
#[test]
fn should_call_by_name_newest_version_when_only_major_specified() {
    let mut builder = prepare_v1_builder();
    install(&mut builder, CONTRACT_WASM, ProtocolVersion::V1_0_0);
    install(&mut builder, CONTRACT_WASM, ProtocolVersion::V1_0_0);
    upgrade_version(&mut builder, 2, false);
    install(&mut builder, CONTRACT_WASM, ProtocolVersion::V2_0_0);

    exec_put_key_by_package_name(&mut builder, Some(1), None, ProtocolVersion::V2_0_0);

    let hash_of_1_2 = get_contract_hash_for_specific_version(&mut builder, 1, 2).unwrap();
    let value = get_value_of_named_key_for_contract_hash_as_str(
        &mut builder,
        hash_of_1_2,
        "key_placeholder",
    )
    .unwrap();
    assert_eq!(value, "key_putter_v2");

    disable_contract_version(&mut builder, 1, 2);
    // After disabling 1.2, selecting by major protocol 1 should point to 1.1

    exec_put_key_by_package_name(&mut builder, Some(1), None, ProtocolVersion::V2_0_0);

    let hash_of_1_1 = get_contract_hash_for_specific_version(&mut builder, 1, 1).unwrap();
    let value = get_value_of_named_key_for_contract_hash_as_str(
        &mut builder,
        hash_of_1_1,
        "key_placeholder",
    )
    .unwrap();
    assert_eq!(value, "key_putter_v1");
}

#[ignore]
#[test]
fn should_call_by_hash_the_newest_version() {
    let mut builder = prepare_v1_builder();
    install(&mut builder, CONTRACT_WASM, ProtocolVersion::V1_0_0);
    install(&mut builder, CONTRACT_WASM, ProtocolVersion::V1_0_0);
    upgrade_version(&mut builder, 2, false);

    exec_put_key_by_package_hash(&mut builder, None, None, ProtocolVersion::V1_0_0);

    let hash_of_1_2 = get_contract_hash_for_specific_version(&mut builder, 1, 2).unwrap();
    let value = get_value_of_named_key_for_contract_hash_as_str(
        &mut builder,
        hash_of_1_2,
        "key_placeholder",
    )
    .unwrap();
    assert_eq!(value, "key_putter_v2");

    upgrade_version(&mut builder, 2, false);
    install(&mut builder, CONTRACT_WASM, ProtocolVersion::V2_0_0);
    install(&mut builder, CONTRACT_WASM, ProtocolVersion::V2_0_0);

    exec_put_key_by_package_hash(&mut builder, None, None, ProtocolVersion::V2_0_0);

    let hash_of_2_2 =
        get_contract_hash_for_specific_version(&mut builder, *CURRENT_PROTOCOL_MAJOR, 2).unwrap();
    let value = get_value_of_named_key_for_contract_hash_as_str(
        &mut builder,
        hash_of_2_2,
        "key_placeholder",
    )
    .unwrap();
    assert_eq!(value, "key_putter_v4");

    disable_contract_version(&mut builder, 2, 2);
    // After disabling 2.2, selecting newest should point to 2.1

    exec_put_key_by_package_hash(&mut builder, None, None, ProtocolVersion::V2_0_0);

    let hash_of_2_1 = get_contract_hash_for_specific_version(&mut builder, 2, 1).unwrap();
    let value = get_value_of_named_key_for_contract_hash_as_str(
        &mut builder,
        hash_of_2_1,
        "key_placeholder",
    )
    .unwrap();
    assert_eq!(value, "key_putter_v3");
}

#[ignore]
#[test]
fn should_call_by_name_the_newest_version() {
    let mut builder = prepare_v1_builder();
    install(&mut builder, CONTRACT_WASM, ProtocolVersion::V1_0_0);
    install(&mut builder, CONTRACT_WASM, ProtocolVersion::V1_0_0);

    exec_put_key_by_package_name(&mut builder, None, None, ProtocolVersion::V1_0_0);

    let hash_of_1_2 = get_contract_hash_for_specific_version(&mut builder, 1, 2).unwrap();
    let value = get_value_of_named_key_for_contract_hash_as_str(
        &mut builder,
        hash_of_1_2,
        "key_placeholder",
    )
    .unwrap();
    assert_eq!(value, "key_putter_v2");

    upgrade_version(&mut builder, 2, false);
    install(&mut builder, CONTRACT_WASM, ProtocolVersion::V2_0_0);
    install(&mut builder, CONTRACT_WASM, ProtocolVersion::V2_0_0);

    exec_put_key_by_package_name(&mut builder, None, None, ProtocolVersion::V2_0_0);

    let hash_of_2_2 =
        get_contract_hash_for_specific_version(&mut builder, *CURRENT_PROTOCOL_MAJOR, 2).unwrap();
    let value = get_value_of_named_key_for_contract_hash_as_str(
        &mut builder,
        hash_of_2_2,
        "key_placeholder",
    )
    .unwrap();
    assert_eq!(value, "key_putter_v4");

    disable_contract_version(&mut builder, 2, 2);
    // After disabling 2.2, selecting newest should point to 2.1

    exec_put_key_by_package_name(&mut builder, None, None, ProtocolVersion::V2_0_0);

    let hash_of_2_1 = get_contract_hash_for_specific_version(&mut builder, 2, 1).unwrap();
    let value = get_value_of_named_key_for_contract_hash_as_str(
        &mut builder,
        hash_of_2_1,
        "key_placeholder",
    )
    .unwrap();
    assert_eq!(value, "key_putter_v3");
}

#[ignore]
#[test]
fn when_disamiguous_calls_are_enabled_should_call_by_hash_querying_by_version() {
    let mut builder = prepare_v1_builder();
    install(&mut builder, CONTRACT_WASM, ProtocolVersion::V1_0_0);
    install(&mut builder, CONTRACT_WASM, ProtocolVersion::V1_0_0);

    exec_put_key_by_package_hash(&mut builder, None, Some(1), ProtocolVersion::V1_0_0);

    let hash_of_1_1 = get_contract_hash_for_specific_version(&mut builder, 1, 1).unwrap();
    let value = get_value_of_named_key_for_contract_hash_as_str(
        &mut builder,
        hash_of_1_1,
        "key_placeholder",
    )
    .unwrap();
    assert_eq!(value, "key_putter_v1");

    upgrade_version(&mut builder, 2, false);
    install(&mut builder, CONTRACT_WASM, ProtocolVersion::V2_0_0);
    install(&mut builder, CONTRACT_WASM, ProtocolVersion::V2_0_0);

    exec_put_key_by_package_hash(&mut builder, None, Some(1), ProtocolVersion::V2_0_0);

    let hash_of_2_1 = get_contract_hash_for_specific_version(&mut builder, 2, 1).unwrap();
    let value = get_value_of_named_key_for_contract_hash_as_str(
        &mut builder,
        hash_of_2_1,
        "key_placeholder",
    )
    .unwrap();
    assert_eq!(value, "key_putter_v3");

    disable_contract_version(&mut builder, *CURRENT_PROTOCOL_MAJOR, 1);
    exec_put_key_by_package_hash(&mut builder, None, Some(1), ProtocolVersion::V2_0_0);

    let hash_of_1_1 = get_contract_hash_for_specific_version(&mut builder, 1, 1).unwrap();
    let value = get_value_of_named_key_for_contract_hash_as_str(
        &mut builder,
        hash_of_1_1,
        "key_placeholder",
    )
    .unwrap();
    assert_eq!(value, "key_putter_v1");
}

#[ignore]
#[test]
fn when_disamiguous_calls_are_disabled_then_ambiguous_call_by_hash_will_fail() {
    let mut builder = prepare_v1_builder();
    install(&mut builder, CONTRACT_WASM, ProtocolVersion::V1_0_0);
    install(&mut builder, CONTRACT_WASM, ProtocolVersion::V1_0_0);
    upgrade_version(&mut builder, 2, true);
    install(&mut builder, CONTRACT_WASM, ProtocolVersion::V2_0_0);
    install(&mut builder, CONTRACT_WASM, ProtocolVersion::V2_0_0);

    let package_hash = get_package_hash(&mut builder);
    let target = TransactionInvocationTarget::ByPackageHash {
        addr: package_hash.into(),
        version: Some(1),
        protocol_version_major: None,
    };
    let request = builder_for_calling_entrypoint(
        "put_key".to_owned(),
        target,
        RuntimeArgs::default(),
        ProtocolVersion::V2_0_0,
    );
    builder.exec(request).expect_failure().commit();
    let error = builder
        .get_last_exec_result()
        .unwrap()
        .error()
        .unwrap()
        .clone();
    assert!(matches!(
        error,
        Error::Exec(ExecError::AmbiguousEntityVersion)
    ))
}

#[ignore]
#[test]
fn when_disamiguous_calls_are_disabled_then_ambiguous_call_by_name_will_fail() {
    let mut builder = prepare_v1_builder();
    install(&mut builder, CONTRACT_WASM, ProtocolVersion::V1_0_0);
    install(&mut builder, CONTRACT_WASM, ProtocolVersion::V1_0_0);
    upgrade_version(&mut builder, 2, true);
    install(&mut builder, CONTRACT_WASM, ProtocolVersion::V2_0_0);
    install(&mut builder, CONTRACT_WASM, ProtocolVersion::V2_0_0);

    let target = TransactionInvocationTarget::ByPackageName {
        name: "package_name".to_owned(),
        version: Some(1),
        protocol_version_major: None,
    };
    let request = builder_for_calling_entrypoint(
        "put_key".to_owned(),
        target,
        RuntimeArgs::default(),
        ProtocolVersion::V2_0_0,
    );
    builder.exec(request).expect_failure().commit();
    let error = builder
        .get_last_exec_result()
        .unwrap()
        .error()
        .unwrap()
        .clone();
    assert!(matches!(
        error,
        Error::Exec(ExecError::AmbiguousEntityVersion)
    ))
}

#[ignore]
#[test]
fn calling_by_package_hash_should_work_when_more_then_two_protocol_versions() {
    let mut builder = prepare_v1_builder();
    install(&mut builder, CONTRACT_WASM, ProtocolVersion::V1_0_0);
    install(&mut builder, CONTRACT_WASM, ProtocolVersion::V1_0_0);
    upgrade_version(&mut builder, 2, false);
    upgrade_version(&mut builder, 3, false);
    install(&mut builder, CONTRACT_WASM, *V3_0_0);

    exec_put_key_by_package_hash(&mut builder, None, Some(1), *V3_0_0);
    assert_contract_version_hash_placeholder_value(&mut builder, 3, 1, "key_putter_v3");

    install(&mut builder, CONTRACT_WASM, *V3_0_0);
    exec_put_key_by_package_hash(&mut builder, None, None, *V3_0_0);
    assert_contract_version_hash_placeholder_value(&mut builder, 3, 2, "key_putter_v4");

    exec_put_key_by_package_hash(&mut builder, Some(1), None, *V3_0_0);
    assert_contract_version_hash_placeholder_value(&mut builder, 1, 2, "key_putter_v2");
}

fn assert_contract_version_hash_placeholder_value(
    builder: &mut LmdbWasmTestBuilder,
    protocol_version: ProtocolVersionMajor,
    entity_version: EntityVersion,
    expected_value: &str,
) {
    let hash_of_contract =
        get_contract_hash_for_specific_version(builder, protocol_version, entity_version).unwrap();
    let value = get_value_of_named_key_for_contract_hash_as_str(
        builder,
        hash_of_contract,
        "key_placeholder",
    )
    .unwrap();
    assert_eq!(value, expected_value);
}

fn install(builder: &mut LmdbWasmTestBuilder, file_name: &str, protocol_version: ProtocolVersion) {
    let install_request = ExecuteRequestBuilder::standard_with_protocol_version(
        *DEFAULT_ACCOUNT_ADDR,
        file_name,
        RuntimeArgs::default(),
        protocol_version,
    )
    .build();
    builder.exec(install_request).expect_success().commit();
}

fn get_value_of_named_key_for_contract_hash_as_str(
    builder: &mut LmdbWasmTestBuilder,
    hash: HashAddr,
    key_name: &str,
) -> Option<String> {
    let get_named_keys_for_contract_hash = get_named_keys_for_contract_hash(builder, hash);
    get_named_keys_for_contract_hash
        .get(key_name)
        .and_then(|key| match builder.query(None, *key, &[]) {
            Ok(v) => match v {
                StoredValue::CLValue(cl_value) => cl_value.into_t().ok(),
                _ => panic!("Unexpected stored value kind"),
            },
            Err(_) => None,
        })
}

fn disable_contract_version(
    builder: &mut LmdbWasmTestBuilder,
    protocol_version_major: ProtocolVersionMajor,
    version: EntityVersion,
) {
    let package_hash = get_package_hash(builder);
    let hash =
        get_contract_hash_for_specific_version(builder, protocol_version_major, version).unwrap();
    let stored_entity_hash = AddressableEntityHash::new(hash);
    let disable_request = {
        let session_args = runtime_args! {
            "contract_package_hash" => package_hash,
            "contract_hash" => stored_entity_hash,
        };

        ExecuteRequestBuilder::standard(*DEFAULT_ACCOUNT_ADDR, DISABLE_CONTRACT, session_args)
            .build()
    };
    builder.exec(disable_request).expect_success().commit();
}

fn get_named_keys_for_contract_hash(
    builder: &mut LmdbWasmTestBuilder,
    hash: HashAddr,
) -> NamedKeys {
    builder.get_named_keys_for_contract(AddressableEntityHash::new(hash))
}

fn call_contract_entrypoint(
    builder: &mut LmdbWasmTestBuilder,
    entry_point: String,
    id: TransactionInvocationTarget,
    args: RuntimeArgs,
    protocol_version: ProtocolVersion,
) {
    let request = builder_for_calling_entrypoint(entry_point, id, args, protocol_version);
    builder.exec(request).expect_success().commit();
}

fn builder_for_calling_entrypoint(
    entry_point: String,
    id: TransactionInvocationTarget,
    args: RuntimeArgs,
    protocol_version: ProtocolVersion,
) -> casper_engine_test_support::ExecuteRequest {
    let target = TransactionTarget::Stored {
        id,
        runtime: TransactionRuntimeParams::VmCasperV1,
    };
    let entry_point = TransactionEntryPoint::Custom(entry_point);
    let v1_hash = TransactionV1Hash::from_raw([5; 32]);
    let mut signers = BTreeSet::new();
    signers.insert(*DEFAULT_ACCOUNT_ADDR);
    let pricing_mode = PricingMode::PaymentLimited {
        payment_amount: 2_500_000,
        gas_price_tolerance: 1,
        standard_payment: true,
    };
    let initiator_addr = casper_types::InitiatorAddr::AccountHash(*DEFAULT_ACCOUNT_ADDR);
    let session_data_v1 = SessionDataV1::new(
        &args,
        &target,
        &entry_point,
        true,
        &v1_hash,
        &pricing_mode,
        &initiator_addr,
        signers,
        true,
    );
    let session_input_data = SessionInputData::SessionDataV1 {
        data: session_data_v1,
    };
    ExecuteRequestBuilder::from_session_input_data_for_protocol_version(
        &session_input_data,
        protocol_version,
    )
    .build()
}

fn get_package_hash(builder: &mut LmdbWasmTestBuilder) -> [u8; 32] {
    let account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
    let get = account.named_keys().get("package_name");
    let package_key = get.unwrap();
    let package_hash = match package_key {
        Key::Hash(hash) => hash,
        _ => {
            panic!("COULDN'T HANLDE")
        }
    };
    *package_hash
}

fn get_contract_hash_for_specific_version(
    builder: &mut LmdbWasmTestBuilder,
    protocol_version_major: ProtocolVersionMajor,
    version: EntityVersion,
) -> Option<HashAddr> {
    let maybe_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR);
    let account = maybe_account.unwrap();
    let get = account.named_keys().get("package_name");
    let package_key = get.unwrap();
    let package_hash = match package_key {
        Key::Hash(hash) => hash,
        _ => {
            panic!("COULDN'T HANLDE THE KEY")
        }
    };
    let package = builder
        .get_package(PackageAddr::new(*package_hash))
        .unwrap();
    let key = EntityVersionKey::new(protocol_version_major, version);
    package.versions().get(&key).map(|x| x.value())
}

fn exec_put_key_by_package_name(
    builder: &mut LmdbWasmTestBuilder,
    protocol_version_major: Option<ProtocolVersionMajor>,
    version: Option<EntityVersion>,
    protocol_version: ProtocolVersion,
) {
    let target = TransactionInvocationTarget::ByPackageName {
        name: "package_name".to_owned(),
        version,
        protocol_version_major,
    };
    call_contract_entrypoint(
        builder,
        "put_key".to_owned(),
        target,
        RuntimeArgs::default(),
        protocol_version,
    )
}

fn exec_put_key_by_package_hash(
    builder: &mut LmdbWasmTestBuilder,
    protocol_version_major: Option<ProtocolVersionMajor>,
    version: Option<EntityVersion>,
    protocol_version: ProtocolVersion,
) {
    let package_hash = get_package_hash(builder);
    let target = TransactionInvocationTarget::ByPackageHash {
        addr: package_hash.into(),
        version,
        protocol_version_major,
    };
    call_contract_entrypoint(
        builder,
        "put_key".to_owned(),
        target,
        RuntimeArgs::default(),
        protocol_version,
    )
}

fn upgrade_version(
    builder: &mut LmdbWasmTestBuilder,
    new_protocol_version_major: ProtocolVersionMajor,
    should_trap_on_ambiguous_entity_version: bool,
) {
    if new_protocol_version_major <= 1 {
        panic!("Can't upgrade to 1 or 0 major version");
    }
    let current_protocol_version =
        ProtocolVersion::from_parts(new_protocol_version_major - 1, 0, 0);
    let new_protocol_version = ProtocolVersion::from_parts(new_protocol_version_major, 0, 0);

    let activation_point = EraId::new(0u64);

    let mut upgrade_request = UpgradeRequestBuilder::new()
        .with_current_protocol_version(current_protocol_version)
        .with_new_protocol_version(new_protocol_version)
        .with_activation_point(activation_point)
        .with_new_gas_hold_handling(HoldBalanceHandling::Accrued)
        .with_new_gas_hold_interval(24 * 60 * 60 * 60)
        .with_addressable_entity_enabled(false)
        .build();
    let config = EngineConfigBuilder::new()
        .with_trap_on_ambiguous_entity_version(should_trap_on_ambiguous_entity_version)
        .build();
    builder
        .with_block_time(Timestamp::now().into())
        .upgrade_using_scratch(&mut upgrade_request)
        .expect_upgrade_success();
    builder.with_engine_config(config);
}

fn prepare_v1_builder() -> LmdbWasmTestBuilder {
    let mut rng = rand::thread_rng();
    let chainspec_bytes = rng.gen::<[u8; 32]>();
    let genesis_account = rng.gen::<[u8; 32]>();
    let chainspec_registry =
        ChainspecRegistry::new_with_genesis(&chainspec_bytes, &genesis_account);

    let run_genesis_request = GenesisRequest::new(
        DEFAULT_GENESIS_CONFIG_HASH,
        ProtocolVersion::V1_0_0,
        DEFAULT_EXEC_CONFIG.clone(),
        chainspec_registry,
    );
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(run_genesis_request);
    builder
}
