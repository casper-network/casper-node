use std::borrow::BorrowMut;

use assert_matches::assert_matches;
use once_cell::sync::Lazy;

use casper_engine_test_support::{
    UpgradeRequestBuilder, DEFAULT_CHAINSPEC_REGISTRY, DEFAULT_PROTOCOL_VERSION,
};
use casper_execution_engine::{engine_state::EngineConfig, tracking_copy::TrackingCopyQueryResult};
use casper_types::{
    system::{auction::ERA_ID_KEY, handle_payment::PAYMENT_PURSE_KEY},
    AccessRights, CLValue, EraId, Key, KeyTag, ProtocolVersion, StoredValue, URef,
};

use crate::lmdb_fixture;

const DEFAULT_ACTIVATION_POINT: EraId = EraId::new(1);
const DO_NOTHING_CONTRACT: &str = "do_nothing_stored";
const ACCESS_KEY_NAME: &str = "do_nothing_access";
const CONTRACT_VERSION_KEY_NAME: &str = "contract_version";

static OLD_PROTOCOL_VERSION: Lazy<ProtocolVersion> = Lazy::new(|| *DEFAULT_PROTOCOL_VERSION);
static NEW_PROTOCOL_VERSION: Lazy<ProtocolVersion> = Lazy::new(|| {
    ProtocolVersion::from_parts(
        OLD_PROTOCOL_VERSION.value().major,
        OLD_PROTOCOL_VERSION.value().minor,
        OLD_PROTOCOL_VERSION.value().patch + 1,
    )
});

#[test]
fn should_upgrade_urefs_in_system_contracts() {
    let (mut builder, _lmdb_fixture_state, _temp_dir) =
        lmdb_fixture::builder_from_global_state_fixture(lmdb_fixture::RELEASE_2_0_0);

    let mut upgrade_request = UpgradeRequestBuilder::new()
        .with_current_protocol_version(*OLD_PROTOCOL_VERSION)
        .with_new_protocol_version(*NEW_PROTOCOL_VERSION)
        .with_activation_point(DEFAULT_ACTIVATION_POINT)
        .with_chainspec_registry(DEFAULT_CHAINSPEC_REGISTRY.clone())
        .build();

    let engine_config = EngineConfig::default();

    builder
        .upgrade_with_upgrade_request(engine_config, &mut upgrade_request)
        .expect_upgrade_success();

    assert_eq!(
        builder.query(
            None,
            Key::from(builder.get_system_auction_hash()),
            &[ERA_ID_KEY.to_owned()],
        ),
        Ok(StoredValue::CLValue(
            CLValue::from_t(EraId::new(0)).unwrap()
        ))
    );

    assert_eq!(
        builder.query_uref_value(
            None,
            Key::from(builder.get_handle_payment_contract_hash()),
            &[PAYMENT_PURSE_KEY.to_owned()],
        ),
        Ok(CLValue::unit())
    );
}

#[test]
fn should_upgrade_urefs_in_non_system_contracts() {
    let (mut builder, _lmdb_fixture_state, _temp_dir) =
        lmdb_fixture::builder_from_global_state_fixture(lmdb_fixture::RELEASE_2_0_0);

    let mut upgrade_request = UpgradeRequestBuilder::new()
        .with_current_protocol_version(*OLD_PROTOCOL_VERSION)
        .with_new_protocol_version(*NEW_PROTOCOL_VERSION)
        .with_activation_point(DEFAULT_ACTIVATION_POINT)
        .with_chainspec_registry(DEFAULT_CHAINSPEC_REGISTRY.clone())
        .build();

    let engine_config = EngineConfig::default();

    builder
        .upgrade_with_upgrade_request(engine_config.clone(), &mut upgrade_request)
        .expect_upgrade_success();

    let mut tc = builder
        .get_engine_state()
        .tracking_copy(builder.get_post_state_hash())
        .expect("should read tracking copy at post state hash")
        .expect("should find tracking copy at post state hash");

    let do_nothing_contract = tc
        .get_keys(&KeyTag::Hash)
        .expect("should get contract keys")
        .into_iter()
        .filter_map(|key| {
            if let TrackingCopyQueryResult::Success { value, .. } = tc
                .query(&engine_config, key, &[])
                .expect("should query tracking copy")
            {
                value.into_addressable_entity()
            } else {
                None
            }
        })
        .find(|ent| {
            ent.named_keys().contains(ACCESS_KEY_NAME)
                && ent.named_keys().contains(CONTRACT_VERSION_KEY_NAME)
        })
        .expect("should find do-nothing contract");

    assert_eq!(
        builder.query_uref_value(
            None,
            *do_nothing_contract
                .named_keys()
                .get(ACCESS_KEY_NAME)
                .unwrap(),
            &[],
        ),
        Ok(CLValue::unit())
    );

    assert_eq!(
        builder.query_uref_value(
            None,
            *do_nothing_contract
                .named_keys()
                .get(CONTRACT_VERSION_KEY_NAME)
                .unwrap(),
            &[],
        ),
        Ok(CLValue::from_t(1u32).unwrap())
    );
}

#[test]
fn should_prune_unreachable_urefs() {
    let (mut builder, _lmdb_fixture_state, _temp_dir) =
        lmdb_fixture::builder_from_global_state_fixture(lmdb_fixture::RELEASE_2_0_0);

    let mut upgrade_request = UpgradeRequestBuilder::new()
        .with_current_protocol_version(*OLD_PROTOCOL_VERSION)
        .with_new_protocol_version(*NEW_PROTOCOL_VERSION)
        .with_activation_point(DEFAULT_ACTIVATION_POINT)
        .with_chainspec_registry(DEFAULT_CHAINSPEC_REGISTRY.clone())
        .build();

    let engine_config = EngineConfig::default();

    let mut tc = builder
        .get_engine_state()
        .tracking_copy(builder.get_post_state_hash())
        .expect("should read tracking copy at post state hash")
        .expect("should find tracking copy at post state hash");

    let key = Key::URef(URef::new([12; 32], AccessRights::all()));
    tc.borrow_mut()
        .write(key, CLValue::from_t(1).unwrap().into());
    builder
        .upgrade_with_upgrade_request(engine_config, &mut upgrade_request)
        .expect_upgrade_success();

    let res = builder.query_uref_value(None, Key::from(key), &[]);
    assert_matches!(res, Err(_));
}

mod fixture {
    use casper_engine_test_support::{
        ExecuteRequestBuilder, DEFAULT_ACCOUNT_ADDR, PRODUCTION_RUN_GENESIS_REQUEST,
    };
    use casper_types::RuntimeArgs;

    use super::DO_NOTHING_CONTRACT;
    use crate::lmdb_fixture;

    #[ignore = "RUN_FIXTURE_GENERATORS env var should be enabled"]
    #[test]
    fn generate_uref_migration_fixture() {
        if !lmdb_fixture::is_fixture_generator_enabled() {
            return;
        }
        // To generate this fixture again you have to re-run this code release-1.4.13.
        let genesis_request = PRODUCTION_RUN_GENESIS_REQUEST.clone();
        lmdb_fixture::generate_fixture(lmdb_fixture::RELEASE_2_0_0, genesis_request, |builder| {
            // Create contract package and store contract ver: 1.0.0 with "delegate" entry function
            let exec_request = {
                let contract_name = format!("{}.wasm", DO_NOTHING_CONTRACT);
                ExecuteRequestBuilder::standard(
                    *DEFAULT_ACCOUNT_ADDR,
                    &contract_name,
                    RuntimeArgs::default(),
                )
                .build()
            };

            builder.exec(exec_request).expect_success().commit();
        })
        .expect("should generate era info bloat fixture");
    }
}
