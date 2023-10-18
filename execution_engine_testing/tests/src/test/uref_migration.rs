use std::borrow::BorrowMut;

use assert_matches::assert_matches;
use once_cell::sync::Lazy;

use casper_engine_test_support::{
    LmdbWasmTestBuilder, UpgradeRequestBuilder, DEFAULT_CHAINSPEC_REGISTRY,
    DEFAULT_PROTOCOL_VERSION,
};
use casper_execution_engine::{engine_state::EngineConfig, tracking_copy::TrackingCopyQueryResult};
use casper_types::{
    system::{
        auction::{
            AUCTION_DELAY_KEY, ERA_END_TIMESTAMP_MILLIS_KEY, ERA_ID_KEY,
            SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY, UNBONDING_DELAY_KEY, VALIDATOR_SLOTS_KEY,
        },
        handle_payment::{ACCUMULATION_PURSE_KEY, PAYMENT_PURSE_KEY},
        mint::{ROUND_SEIGNIORAGE_RATE_KEY, TOTAL_SUPPLY_KEY},
    },
    AccessRights, CLValue, ContractHash, EraId, Key, KeyTag, ProtocolVersion, URef,
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

    validate_urefs_behind_keys(
        &builder,
        builder.get_system_auction_hash(),
        &[
            ERA_ID_KEY,
            ERA_END_TIMESTAMP_MILLIS_KEY,
            SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY,
            VALIDATOR_SLOTS_KEY,
            AUCTION_DELAY_KEY,
            UNBONDING_DELAY_KEY,
        ],
    );

    validate_urefs_behind_keys(
        &builder,
        builder.get_mint_contract_hash(),
        &[ROUND_SEIGNIORAGE_RATE_KEY, TOTAL_SUPPLY_KEY],
    );

    validate_urefs_behind_keys(
        &builder,
        builder.get_handle_payment_contract_hash(),
        &[PAYMENT_PURSE_KEY, ACCUMULATION_PURSE_KEY],
    );
}

#[test]
fn should_upgrade_urefs_in_a_non_system_contract() {
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

    let urefs: Vec<_> =
        std::iter::repeat_with(|| URef::new(rand::random(), AccessRights::READ_ADD_WRITE))
            .take(32)
            .collect();

    for &uref in &urefs {
        let value = rand::random::<i32>();
        tc.borrow_mut()
            .write(Key::URef(uref), CLValue::from_t(value).unwrap().into());
    }

    builder
        .upgrade_with_upgrade_request(engine_config, &mut upgrade_request)
        .expect_upgrade_success();

    for &uref in &urefs {
        let res = builder.query_uref_value(None, Key::URef(uref), &[]);
        assert_matches!(res, Err(_));
    }
}

fn validate_urefs_behind_keys(builder: &LmdbWasmTestBuilder, owner: ContractHash, keys: &[&str]) {
    for &key in keys {
        assert_matches!(
            builder.query_uref_value(None, Key::from(owner), &[key.to_owned()],),
            Ok(_)
        );
    }
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
        // To generate this fixture again you have to re-run this code release-2.0.0.
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
        .expect("should generate uref migration fixture");
    }
}
