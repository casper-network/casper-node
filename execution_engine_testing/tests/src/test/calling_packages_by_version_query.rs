use std::collections::BTreeSet;

/// This test assumes that the provided fixture has v1 and v2 installed in protocol version 1
use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR, DEFAULT_PROTOCOL_VERSION,
    LOCAL_GENESIS_REQUEST,
};
use casper_execution_engine::engine_state::{SessionDataV1, SessionInputData};
use casper_types::{
    contracts::ProtocolVersionMajor, runtime_args, AddressableEntityHash, EntityVersion,
    EntityVersionKey, HashAddr, Key, NamedKeys, PackageHash, PricingMode, RuntimeArgs, StoredValue,
    TransactionEntryPoint, TransactionInvocationTarget, TransactionRuntimeParams,
    TransactionTarget, TransactionV1Hash,
};
use once_cell::sync::Lazy;

use crate::lmdb_fixture;

const COUNTER_V3_INSTALLER_WASM: &str = "key_putter_v3.wasm";
const COUNTER_V4_INSTALLER_WASM: &str = "key_putter_v4.wasm";

const DISABLE_CONTRACT: &str = "disable_contract.wasm";

const FIXTURE: &str = "contract_in_different_versions";

static CURRENT_PROTOCOL_MAJOR: Lazy<u32> = Lazy::new(|| DEFAULT_PROTOCOL_VERSION.value().major);

const COUNTER_V1_INSTALLER_WASM: &str = "key_putter_v1.wasm";
const COUNTER_V2_INSTALLER_WASM: &str = "key_putter_v2.wasm";
#[test]
fn gen_fixture() {
    lmdb_fixture::generate_fixture(
        "contract_in_different_versions",
        LOCAL_GENESIS_REQUEST.clone(),
        |builder| {
            install(builder, COUNTER_V1_INSTALLER_WASM);
            install(builder, COUNTER_V2_INSTALLER_WASM);
        },
    )
    .expect("should gen fixture");
}

#[ignore]
#[test]
fn should_call_package_by_exact_version() {
    let (mut builder, _lmdb_fixture_state, _temp_dir) =
        lmdb_fixture::builder_from_global_state_fixture(FIXTURE);

    exec_put_key_in_version(&mut builder, Some(1), Some(1));

    let hash_of_1_1 = get_contract_hash_for_specific_version(&mut builder, 1, 1).unwrap();
    let value =
        get_value_of_named_key_for_contract_hash_as_str(&mut builder, hash_of_1_1, "key_name")
            .unwrap();
    assert_eq!(value, "key_putter_v1");
}

#[ignore]
#[test]
fn should_call_newest_version_when_only_major_specified() {
    let (mut builder, _lmdb_fixture_state, _temp_dir) =
        lmdb_fixture::builder_from_global_state_fixture(FIXTURE);

    exec_put_key_in_version(&mut builder, Some(1), None);

    let hash_of_1_2 = get_contract_hash_for_specific_version(&mut builder, 1, 2).unwrap();
    let value =
        get_value_of_named_key_for_contract_hash_as_str(&mut builder, hash_of_1_2, "key_name")
            .unwrap();
    assert_eq!(value, "key_putter_v2");
}

#[ignore]
#[test]
fn should_call_newest_version_when_only_major_specified_including_disabled() {
    let (mut builder, _lmdb_fixture_state, _temp_dir) =
        lmdb_fixture::builder_from_global_state_fixture(FIXTURE);
    disable_contract_version(&mut builder, 1, 2);

    exec_put_key_in_version(&mut builder, Some(1), None);

    let hash_of_1_1 = get_contract_hash_for_specific_version(&mut builder, 1, 1).unwrap();
    let value =
        get_value_of_named_key_for_contract_hash_as_str(&mut builder, hash_of_1_1, "key_name")
            .unwrap();
    assert_eq!(value, "key_putter_v1");
}

#[ignore]
#[test]
fn should_call_the_newest_version_when_nothing_specified() {
    let (mut builder, _lmdb_fixture_state, _temp_dir) =
        lmdb_fixture::builder_from_global_state_fixture(FIXTURE);
    install(&mut builder, COUNTER_V3_INSTALLER_WASM);
    install(&mut builder, COUNTER_V4_INSTALLER_WASM);

    exec_put_key_in_version(&mut builder, None, None);

    let hash_of_2_2 =
        get_contract_hash_for_specific_version(&mut builder, *CURRENT_PROTOCOL_MAJOR, 2).unwrap();
    let value =
        get_value_of_named_key_for_contract_hash_as_str(&mut builder, hash_of_2_2, "key_name")
            .unwrap();
    assert_eq!(value, "key_putter_v4");
}

#[ignore]
#[test]
fn should_consider_disables_when_calling_newest() {
    let (mut builder, _lmdb_fixture_state, _temp_dir) =
        lmdb_fixture::builder_from_global_state_fixture(FIXTURE);
    install(&mut builder, COUNTER_V3_INSTALLER_WASM);
    install(&mut builder, COUNTER_V4_INSTALLER_WASM);
    disable_contract_version(&mut builder, *CURRENT_PROTOCOL_MAJOR, 2);

    exec_put_key_in_version(&mut builder, None, None);

    let hash_of_2_1 =
        get_contract_hash_for_specific_version(&mut builder, *CURRENT_PROTOCOL_MAJOR, 1).unwrap();
    let value =
        get_value_of_named_key_for_contract_hash_as_str(&mut builder, hash_of_2_1, "key_name")
            .unwrap();
    assert_eq!(value, "key_putter_v3");
}

#[ignore]
#[test]
fn should_execute_version_in_newest_protocol() {
    let (mut builder, _lmdb_fixture_state, _temp_dir) =
        lmdb_fixture::builder_from_global_state_fixture(FIXTURE);
    install(&mut builder, COUNTER_V3_INSTALLER_WASM);
    install(&mut builder, COUNTER_V4_INSTALLER_WASM);

    exec_put_key_in_version(&mut builder, None, Some(1));

    let hash_of_2_1 =
        get_contract_hash_for_specific_version(&mut builder, *CURRENT_PROTOCOL_MAJOR, 1).unwrap();
    let value =
        get_value_of_named_key_for_contract_hash_as_str(&mut builder, hash_of_2_1, "key_name")
            .unwrap();
    assert_eq!(value, "key_putter_v3");
}

#[ignore]
#[test]
fn should_consider_disables_when_executing_by_version() {
    let (mut builder, _lmdb_fixture_state, _temp_dir) =
        lmdb_fixture::builder_from_global_state_fixture(FIXTURE);
    install(&mut builder, COUNTER_V3_INSTALLER_WASM);
    install(&mut builder, COUNTER_V4_INSTALLER_WASM);
    disable_contract_version(&mut builder, *CURRENT_PROTOCOL_MAJOR, 1);

    exec_put_key_in_version(&mut builder, None, Some(1));

    let hash_of_1_1 = get_contract_hash_for_specific_version(&mut builder, 1, 1).unwrap();
    let value =
        get_value_of_named_key_for_contract_hash_as_str(&mut builder, hash_of_1_1, "key_name")
            .unwrap();
    assert_eq!(value, "key_putter_v1");
}

fn install(builder: &mut LmdbWasmTestBuilder, file_name: &str) {
    let install_request =
        ExecuteRequestBuilder::standard(*DEFAULT_ACCOUNT_ADDR, file_name, RuntimeArgs::default())
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
    protocol_version_major: Option<ProtocolVersionMajor>,
    version: Option<EntityVersion>,
    args: RuntimeArgs,
) {
    let package_hash = get_package_hash(builder);

    let target = TransactionTarget::Stored {
        id: TransactionInvocationTarget::ByPackageHash {
            addr: package_hash,
            version,
            protocol_version_major,
        },
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
    let request = ExecuteRequestBuilder::from_session_input_data(&session_input_data).build();
    builder.exec(request).expect_success().commit();
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
    let z = builder.get_account(*DEFAULT_ACCOUNT_ADDR);
    let account = z.unwrap();
    let get = account.named_keys().get("package_name");
    let package_key = get.unwrap();
    let package_hash = match package_key {
        Key::Hash(hash) => hash,
        _ => {
            panic!("COULDN'T HANLDE THE KEY")
        }
    };
    let package = builder
        .get_package(PackageHash::new(*package_hash))
        .unwrap();
    let key = EntityVersionKey::new(protocol_version_major, version);
    package.versions().get(&key).map(|x| x.value())
}

fn exec_put_key_in_version(
    builder: &mut LmdbWasmTestBuilder,
    protocol_version_major: Option<ProtocolVersionMajor>,
    version: Option<EntityVersion>,
) {
    call_contract_entrypoint(
        builder,
        "put_key".to_owned(),
        protocol_version_major,
        version,
        RuntimeArgs::default(),
    )
}
