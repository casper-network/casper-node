use std::{cell::RefCell, collections::BTreeSet, convert::TryInto, iter::FromIterator, rc::Rc};

use once_cell::sync::Lazy;
use rand::RngCore;

use casper_storage::{
    global_state::state::lmdb::LmdbGlobalStateView, tracking_copy::new_temporary_tracking_copy,
    AddressGenerator, TrackingCopy,
};

use casper_types::{
    account::{AccountHash, ACCOUNT_HASH_LENGTH},
    addressable_entity::{
        ActionType, AddKeyFailure, AssociatedKeys, MessageTopics, NamedKeys, RemoveKeyFailure,
        SetThresholdFailure, Weight,
    },
    bytesrepr::ToBytes,
    execution::TransformKind,
    system::{AUCTION, HANDLE_PAYMENT, MINT, STANDARD_PAYMENT},
    AccessRights, AddressableEntity, AddressableEntityHash, BlockTime, ByteCodeHash, CLValue,
    ContextAccessRights, DeployHash, EntityAddr, EntityKind, EntryPointType, EntryPoints, Gas, Key,
    PackageHash, Phase, ProtocolVersion, PublicKey, RuntimeArgs, SecretKey, StoredValue,
    SystemEntityRegistry, Tagged, URef, KEY_HASH_LENGTH, U256, U512,
};
use tempfile::TempDir;

use super::{Error, RuntimeContext};
use crate::engine_state::EngineConfig;

const DEPLOY_HASH: [u8; 32] = [1u8; 32];
const PHASE: Phase = Phase::Session;
const GAS_LIMIT: u64 = 500_000_000_000_000u64;

static TEST_ENGINE_CONFIG: Lazy<EngineConfig> = Lazy::new(EngineConfig::default);

fn test_engine_config() -> EngineConfig {
    EngineConfig::default()
}

fn new_tracking_copy(
    account_hash: AccountHash,
    init_entity_key: Key,
    init_entity: AddressableEntity,
) -> (TrackingCopy<LmdbGlobalStateView>, TempDir) {
    let entity_key_cl_value = CLValue::from_t(init_entity_key).expect("must convert to cl value");

    let initial_data = [
        (init_entity_key, StoredValue::AddressableEntity(init_entity)),
        (
            Key::Account(account_hash),
            StoredValue::CLValue(entity_key_cl_value),
        ),
    ];
    new_temporary_tracking_copy(initial_data, None)
}

fn new_addressable_entity_with_purse(
    account_hash: AccountHash,
    entity_hash: AddressableEntityHash,
    entity_kind: EntityKind,
    purse: [u8; 32],
) -> (Key, Key, AddressableEntity) {
    let associated_keys = AssociatedKeys::new(account_hash, Weight::new(1));
    let entity = AddressableEntity::new(
        PackageHash::default(),
        ByteCodeHash::default(),
        EntryPoints::new_with_default_entry_point(),
        ProtocolVersion::V1_0_0,
        URef::new(purse, AccessRights::READ_ADD_WRITE),
        associated_keys,
        Default::default(),
        MessageTopics::default(),
        entity_kind,
    );
    let account_key = Key::Account(account_hash);
    let contract_key = Key::addressable_entity_key(entity_kind.tag(), entity_hash);

    (account_key, contract_key, entity)
}

fn new_addressable_entity(
    account_hash: AccountHash,
    entity_hash: AddressableEntityHash,
) -> (Key, Key, AddressableEntity) {
    new_addressable_entity_with_purse(
        account_hash,
        entity_hash,
        EntityKind::Account(account_hash),
        [0; 32],
    )
}

// create random account key.
fn random_account_key<G: RngCore>(entropy_source: &mut G) -> Key {
    let mut key = [0u8; 32];
    entropy_source.fill_bytes(&mut key);
    Key::Account(AccountHash::new(key))
}

// create random contract key.
fn random_contract_key<G: RngCore>(entropy_source: &mut G) -> Key {
    let mut key_hash = [0u8; 32];
    entropy_source.fill_bytes(&mut key_hash);
    Key::AddressableEntity(EntityAddr::SmartContract(key_hash))
}

// Create URef Key.
fn create_uref_as_key(address_generator: &mut AddressGenerator, rights: AccessRights) -> Key {
    let address = address_generator.create_address();
    Key::URef(URef::new(address, rights))
}

fn random_hash<G: RngCore>(entropy_source: &mut G) -> Key {
    let mut key = [0u8; KEY_HASH_LENGTH];
    entropy_source.fill_bytes(&mut key);
    Key::Hash(key)
}

fn new_runtime_context<'a>(
    addressable_entity: &'a AddressableEntity,
    account_hash: AccountHash,
    entity_address: Key,
    named_keys: &'a mut NamedKeys,
    access_rights: ContextAccessRights,
    address_generator: AddressGenerator,
) -> (RuntimeContext<'a, LmdbGlobalStateView>, TempDir) {
    let (mut tracking_copy, tempdir) =
        new_tracking_copy(account_hash, entity_address, addressable_entity.clone());

    let default_system_registry = {
        let mut registry = SystemEntityRegistry::new();
        registry.insert(MINT.to_string(), AddressableEntityHash::default());
        registry.insert(HANDLE_PAYMENT.to_string(), AddressableEntityHash::default());
        registry.insert(
            STANDARD_PAYMENT.to_string(),
            AddressableEntityHash::default(),
        );
        registry.insert(AUCTION.to_string(), AddressableEntityHash::default());
        StoredValue::CLValue(CLValue::from_t(registry).unwrap())
    };

    tracking_copy.write(Key::SystemEntityRegistry, default_system_registry);

    let runtime_context = RuntimeContext::new(
        named_keys,
        addressable_entity,
        entity_address,
        BTreeSet::from_iter(vec![account_hash]),
        access_rights,
        EntityKind::Account(account_hash),
        account_hash,
        Rc::new(RefCell::new(address_generator)),
        Rc::new(RefCell::new(tracking_copy)),
        TEST_ENGINE_CONFIG.clone(),
        BlockTime::new(0),
        ProtocolVersion::V1_0_0,
        DeployHash::from_raw([1u8; 32]),
        Phase::Session,
        RuntimeArgs::new(),
        Gas::new(U512::from(GAS_LIMIT)),
        Gas::default(),
        Vec::default(),
        U512::MAX,
        EntryPointType::Session,
    );

    (runtime_context, tempdir)
}

#[allow(clippy::assertions_on_constants)]
fn assert_forged_reference<T>(result: Result<T, Error>) {
    match result {
        Err(Error::ForgedReference(_)) => assert!(true),
        _ => panic!("Error. Test should have failed with ForgedReference error but didn't."),
    }
}

#[allow(clippy::assertions_on_constants)]
fn assert_invalid_access<T: std::fmt::Debug>(result: Result<T, Error>, expecting: AccessRights) {
    match result {
        Err(Error::InvalidAccess { required }) if required == expecting => assert!(true),
        other => panic!(
            "Error. Test should have failed with InvalidAccess error but didn't: {:?}.",
            other
        ),
    }
}

fn build_runtime_context_and_execute<T, F>(
    mut named_keys: NamedKeys,
    functor: F,
) -> Result<T, Error>
where
    F: FnOnce(RuntimeContext<LmdbGlobalStateView>) -> Result<T, Error>,
{
    let secret_key = SecretKey::ed25519_from_bytes([222; SecretKey::ED25519_LENGTH])
        .expect("should create secret key");
    let public_key = PublicKey::from(&secret_key);
    let account_hash = public_key.to_account_hash();
    let entity_hash = AddressableEntityHash::new([10u8; 32]);
    let deploy_hash = [1u8; 32];
    let (_, entity_key, addressable_entity) =
        new_addressable_entity(public_key.to_account_hash(), entity_hash);

    let address_generator = AddressGenerator::new(&deploy_hash, Phase::Session);
    let access_rights = addressable_entity.extract_access_rights(entity_hash, &named_keys);
    let (runtime_context, _tempdir) = new_runtime_context(
        &addressable_entity,
        account_hash,
        entity_key,
        &mut named_keys,
        access_rights,
        address_generator,
    );

    functor(runtime_context)
}

#[track_caller]
fn last_transform_kind_on_addressable_entity(
    runtime_context: &RuntimeContext<LmdbGlobalStateView>,
) -> TransformKind {
    let key = runtime_context.entity_key;
    runtime_context
        .effects()
        .transforms()
        .iter()
        .rev()
        .find_map(|transform| (transform.key() == &key).then(|| transform.kind().clone()))
        .unwrap()
}

#[test]
fn use_uref_valid() {
    // Test fixture
    let mut rng = AddressGenerator::new(&DEPLOY_HASH, PHASE);
    let uref_as_key = create_uref_as_key(&mut rng, AccessRights::READ_WRITE);
    let mut named_keys = NamedKeys::new();
    named_keys.insert(String::new(), uref_as_key);
    // Use uref as the key to perform an action on the global state.
    // This should succeed because the uref is valid.
    let value = StoredValue::CLValue(CLValue::from_t(43_i32).unwrap());
    let query_result = build_runtime_context_and_execute(named_keys, |mut rc| {
        rc.metered_write_gs(uref_as_key, value)
    });
    query_result.expect("writing using valid uref should succeed");
}

#[test]
fn use_uref_forged() {
    // Test fixture
    let mut rng = AddressGenerator::new(&DEPLOY_HASH, PHASE);
    let uref = create_uref_as_key(&mut rng, AccessRights::READ_WRITE);
    let named_keys = NamedKeys::new();
    // named_keys.insert(String::new(), Key::from(uref));
    let value = StoredValue::CLValue(CLValue::from_t(43_i32).unwrap());
    let query_result =
        build_runtime_context_and_execute(named_keys, |mut rc| rc.metered_write_gs(uref, value));

    assert_forged_reference(query_result);
}

#[test]
fn account_key_not_writeable() {
    let mut rng = rand::thread_rng();
    let acc_key = random_account_key(&mut rng);
    let query_result = build_runtime_context_and_execute(NamedKeys::new(), |mut rc| {
        rc.metered_write_gs(
            acc_key,
            StoredValue::CLValue(CLValue::from_t(1_i32).unwrap()),
        )
    });
    assert_invalid_access(query_result, AccessRights::WRITE);
}

#[test]
fn entity_key_readable_valid() {
    // Entity key is readable if it is a "base" key - current context of the
    // execution.
    let query_result = build_runtime_context_and_execute(NamedKeys::new(), |mut rc| {
        let base_key = rc.get_entity_key();
        let entity = rc.get_entity();

        let result = rc
            .read_gs(&base_key)
            .expect("Account key is readable.")
            .expect("Account is found in GS.");

        assert_eq!(result, StoredValue::AddressableEntity(entity));
        Ok(())
    });

    assert!(query_result.is_ok());
}

#[test]
fn account_key_addable_returns_type_mismatch() {
    // Account key is not addable anymore as we do not store an account underneath they key
    // but instead there is a CLValue which acts as an indirection to the corresponding entity.
    let mut rng = AddressGenerator::new(&DEPLOY_HASH, PHASE);
    let uref_as_key = create_uref_as_key(&mut rng, AccessRights::READ);
    let mut named_keys = NamedKeys::new();
    named_keys.insert(String::new(), uref_as_key);
    let query_result = build_runtime_context_and_execute(named_keys, |mut rc| {
        let account_key: Key = rc.account_hash.into();
        let uref_name = "NewURef".to_owned();
        let named_key = StoredValue::CLValue(CLValue::from_t((uref_name, uref_as_key)).unwrap());

        rc.metered_add_gs(account_key, named_key)
    });

    assert!(query_result.is_err());
}

#[test]
fn account_key_addable_invalid() {
    // Account key is NOT addable if it is a "base" key - current context of the
    // execution.
    let mut rng = rand::thread_rng();
    let other_acc_key = random_account_key(&mut rng);

    let query_result = build_runtime_context_and_execute(NamedKeys::new(), |mut rc| {
        rc.metered_add_gs(
            other_acc_key,
            StoredValue::CLValue(CLValue::from_t(1_i32).unwrap()),
        )
    });

    assert_invalid_access(query_result, AccessRights::ADD);
}

#[test]
fn contract_key_readable_valid() {
    // Account key is readable if it is a "base" key - current context of the
    // execution.
    let mut rng = rand::thread_rng();
    let contract_key = random_contract_key(&mut rng);
    let query_result =
        build_runtime_context_and_execute(NamedKeys::new(), |mut rc| rc.read_gs(&contract_key));

    assert!(query_result.is_ok());
}

#[test]
fn contract_key_not_writeable() {
    // Account key is readable if it is a "base" key - current context of the
    // execution.
    let mut rng = rand::thread_rng();
    let contract_key = random_contract_key(&mut rng);
    let query_result = build_runtime_context_and_execute(NamedKeys::new(), |mut rc| {
        rc.metered_write_gs(
            contract_key,
            StoredValue::CLValue(CLValue::from_t(1_i32).unwrap()),
        )
    });

    assert_invalid_access(query_result, AccessRights::WRITE);
}

#[test]
fn contract_key_addable_valid() {
    // Contract key is addable if it is a "base" key - current context of the execution.
    let account_hash = AccountHash::new([0u8; 32]);
    let entity_hash = AddressableEntityHash::new([1u8; 32]);
    let (_account_key, entity_key, entity) = new_addressable_entity(account_hash, entity_hash);
    let authorization_keys = BTreeSet::from_iter(vec![account_hash]);
    let mut address_generator = AddressGenerator::new(&DEPLOY_HASH, PHASE);

    let mut rng = rand::thread_rng();
    let contract_key = random_contract_key(&mut rng);
    let entity_as_stored_value = StoredValue::AddressableEntity(AddressableEntity::default());
    let mut access_rights = entity_as_stored_value
        .as_addressable_entity()
        .unwrap()
        .extract_access_rights(AddressableEntityHash::default(), &NamedKeys::new());

    let (tracking_copy, _tempdir) = new_tracking_copy(account_hash, entity_key, entity);
    let tracking_copy = Rc::new(RefCell::new(tracking_copy));
    tracking_copy
        .borrow_mut()
        .write(contract_key, entity_as_stored_value.clone());

    let default_system_registry = {
        let mut registry = SystemEntityRegistry::new();
        registry.insert(MINT.to_string(), AddressableEntityHash::default());
        registry.insert(HANDLE_PAYMENT.to_string(), AddressableEntityHash::default());
        registry.insert(
            STANDARD_PAYMENT.to_string(),
            AddressableEntityHash::default(),
        );
        registry.insert(AUCTION.to_string(), AddressableEntityHash::default());
        StoredValue::CLValue(CLValue::from_t(registry).unwrap())
    };

    tracking_copy
        .borrow_mut()
        .write(Key::SystemEntityRegistry, default_system_registry);

    let uref_as_key = create_uref_as_key(&mut address_generator, AccessRights::WRITE);
    let uref_name = "NewURef".to_owned();
    let named_uref_tuple =
        StoredValue::CLValue(CLValue::from_t((uref_name.clone(), uref_as_key)).unwrap());
    let mut named_keys = NamedKeys::new();
    named_keys.insert(uref_name, uref_as_key);

    access_rights.extend(&[uref_as_key.into_uref().expect("should be a URef")]);

    let mut runtime_context = RuntimeContext::new(
        &mut named_keys,
        entity_as_stored_value.as_addressable_entity().unwrap(),
        contract_key,
        authorization_keys,
        access_rights,
        EntityKind::SmartContract,
        account_hash,
        Rc::new(RefCell::new(address_generator)),
        Rc::clone(&tracking_copy),
        EngineConfig::default(),
        BlockTime::new(0),
        ProtocolVersion::V1_0_0,
        DeployHash::from_raw(DEPLOY_HASH),
        PHASE,
        RuntimeArgs::new(),
        Gas::new(U512::from(GAS_LIMIT)),
        Gas::default(),
        Vec::default(),
        U512::zero(),
        EntryPointType::Session,
    );

    assert!(runtime_context
        .metered_add_gs(contract_key, named_uref_tuple)
        .is_err())
}

#[test]
fn contract_key_addable_invalid() {
    let account_hash = AccountHash::new([0u8; 32]);
    let entity_hash = AddressableEntityHash::new([1u8; 32]);
    let (_, entity_key, entity) = new_addressable_entity(account_hash, entity_hash);
    let authorization_keys = BTreeSet::from_iter(vec![account_hash]);
    let mut address_generator = AddressGenerator::new(&DEPLOY_HASH, PHASE);
    let mut rng = rand::thread_rng();
    let contract_key = random_contract_key(&mut rng);

    let other_contract_key = random_contract_key(&mut rng);
    let contract = StoredValue::AddressableEntity(AddressableEntity::default());
    let mut access_rights = contract
        .as_addressable_entity()
        .unwrap()
        .extract_access_rights(AddressableEntityHash::default(), &NamedKeys::new());
    let (tracking_copy, _tempdir) = new_tracking_copy(account_hash, entity_key, entity.clone());
    let tracking_copy = Rc::new(RefCell::new(tracking_copy));

    tracking_copy.borrow_mut().write(contract_key, contract);

    let uref_as_key = create_uref_as_key(&mut address_generator, AccessRights::WRITE);
    let uref_name = "NewURef".to_owned();
    let named_uref_tuple = StoredValue::CLValue(CLValue::from_t((uref_name, uref_as_key)).unwrap());

    let mut named_keys = NamedKeys::new();
    named_keys.insert(String::new(), uref_as_key);

    access_rights.extend(&[uref_as_key.into_uref().expect("should be a URef")]);

    let mut runtime_context = RuntimeContext::new(
        &mut named_keys,
        &entity,
        other_contract_key,
        authorization_keys,
        access_rights,
        EntityKind::Account(account_hash),
        account_hash,
        Rc::new(RefCell::new(address_generator)),
        Rc::clone(&tracking_copy),
        EngineConfig::default(),
        BlockTime::new(0),
        ProtocolVersion::V1_0_0,
        DeployHash::from_raw(DEPLOY_HASH),
        PHASE,
        RuntimeArgs::new(),
        Gas::new(U512::from(GAS_LIMIT)),
        Gas::default(),
        Vec::default(),
        U512::zero(),
        EntryPointType::Session,
    );

    let result = runtime_context.metered_add_gs(contract_key, named_uref_tuple);

    assert_invalid_access(result, AccessRights::ADD);
}

#[test]
fn uref_key_readable_valid() {
    let mut rng = AddressGenerator::new(&DEPLOY_HASH, PHASE);
    let uref_key = create_uref_as_key(&mut rng, AccessRights::READ);

    let mut named_keys = NamedKeys::new();
    named_keys.insert(String::new(), uref_key);

    let query_result =
        build_runtime_context_and_execute(named_keys, |mut rc| rc.read_gs(&uref_key));
    assert!(query_result.is_ok());
}

#[test]
fn uref_key_readable_invalid() {
    let mut rng = AddressGenerator::new(&DEPLOY_HASH, PHASE);
    let uref_key = create_uref_as_key(&mut rng, AccessRights::WRITE);

    let mut named_keys = NamedKeys::new();
    named_keys.insert(String::new(), uref_key);

    let query_result =
        build_runtime_context_and_execute(named_keys, |mut rc| rc.read_gs(&uref_key));
    assert_invalid_access(query_result, AccessRights::READ);
}

#[test]
fn uref_key_writeable_valid() {
    let mut rng = AddressGenerator::new(&DEPLOY_HASH, PHASE);
    let uref_key = create_uref_as_key(&mut rng, AccessRights::WRITE);

    let mut named_keys = NamedKeys::new();
    named_keys.insert(String::new(), uref_key);

    let query_result = build_runtime_context_and_execute(named_keys, |mut rc| {
        rc.metered_write_gs(
            uref_key,
            StoredValue::CLValue(CLValue::from_t(1_i32).unwrap()),
        )
    });
    assert!(query_result.is_ok());
}

#[test]
fn uref_key_writeable_invalid() {
    let mut rng = AddressGenerator::new(&DEPLOY_HASH, PHASE);
    let uref_key = create_uref_as_key(&mut rng, AccessRights::READ);

    let mut named_keys = NamedKeys::new();
    named_keys.insert(String::new(), uref_key);

    let query_result = build_runtime_context_and_execute(named_keys, |mut rc| {
        rc.metered_write_gs(
            uref_key,
            StoredValue::CLValue(CLValue::from_t(1_i32).unwrap()),
        )
    });
    assert_invalid_access(query_result, AccessRights::WRITE);
}

#[test]
fn uref_key_addable_valid() {
    let mut rng = AddressGenerator::new(&DEPLOY_HASH, PHASE);
    let uref_key = create_uref_as_key(&mut rng, AccessRights::ADD_WRITE);

    let mut named_keys = NamedKeys::new();
    named_keys.insert(String::new(), uref_key);

    let query_result = build_runtime_context_and_execute(named_keys, |mut rc| {
        rc.metered_write_gs(uref_key, CLValue::from_t(10_i32).unwrap())
            .expect("Writing to the GlobalState should work.");
        rc.metered_add_gs(uref_key, CLValue::from_t(1_i32).unwrap())
    });
    assert!(query_result.is_ok());
}

#[test]
fn uref_key_addable_invalid() {
    let mut rng = AddressGenerator::new(&DEPLOY_HASH, PHASE);
    let uref_key = create_uref_as_key(&mut rng, AccessRights::WRITE);

    let mut named_keys = NamedKeys::new();
    named_keys.insert(String::new(), uref_key);

    let query_result = build_runtime_context_and_execute(named_keys, |mut rc| {
        rc.metered_add_gs(
            uref_key,
            StoredValue::CLValue(CLValue::from_t(1_i32).unwrap()),
        )
    });
    assert_invalid_access(query_result, AccessRights::ADD);
}

#[test]
fn hash_key_readable() {
    // values under hash's are universally readable
    let query = |runtime_context: RuntimeContext<LmdbGlobalStateView>| {
        let mut rng = rand::thread_rng();
        let key = random_hash(&mut rng);
        runtime_context.validate_readable(&key)
    };
    let query_result = build_runtime_context_and_execute(NamedKeys::new(), query);
    assert!(query_result.is_ok())
}

#[test]
fn hash_key_writeable() {
    // values under hash's are immutable
    let query = |runtime_context: RuntimeContext<LmdbGlobalStateView>| {
        let mut rng = rand::thread_rng();
        let key = random_hash(&mut rng);
        runtime_context.validate_writeable(&key)
    };
    let query_result = build_runtime_context_and_execute(NamedKeys::new(), query);
    assert!(query_result.is_err())
}

#[test]
fn hash_key_addable_invalid() {
    // values under hashes are immutable
    let query = |runtime_context: RuntimeContext<LmdbGlobalStateView>| {
        let mut rng = rand::thread_rng();
        let key = random_hash(&mut rng);
        runtime_context.validate_addable(&key)
    };
    let query_result = build_runtime_context_and_execute(NamedKeys::new(), query);
    assert!(query_result.is_err())
}

#[test]
fn manage_associated_keys() {
    // Testing a valid case only - successfully added a key, and successfully removed,
    // making sure `account_dirty` mutated
    let named_keys = NamedKeys::new();
    let query = |mut runtime_context: RuntimeContext<LmdbGlobalStateView>| {
        let account_hash = AccountHash::new([42; 32]);
        let weight = Weight::new(155);

        // Add a key (this doesn't check for all invariants as `add_key`
        // is already tested in different place)
        runtime_context
            .add_associated_key(account_hash, weight)
            .expect("Unable to add key");

        let transform_kind = last_transform_kind_on_addressable_entity(&runtime_context);
        let entity = match transform_kind {
            TransformKind::Write(StoredValue::AddressableEntity(entity)) => entity,
            _ => panic!("Invalid transform operation found"),
        };
        entity
            .associated_keys()
            .get(&account_hash)
            .expect("Account hash wasn't added to associated keys");

        let new_weight = Weight::new(100);
        runtime_context
            .update_associated_key(account_hash, new_weight)
            .expect("Unable to update key");

        let transform_kind = last_transform_kind_on_addressable_entity(&runtime_context);
        let entity = match transform_kind {
            TransformKind::Write(StoredValue::AddressableEntity(entity)) => entity,
            _ => panic!("Invalid transform operation found"),
        };
        let value = entity
            .associated_keys()
            .get(&account_hash)
            .expect("Account hash wasn't added to associated keys");

        assert_eq!(value, &new_weight, "value was not updated");

        // Remove a key that was already added
        runtime_context
            .remove_associated_key(account_hash)
            .expect("Unable to remove key");

        // Verify
        let transform_kind = last_transform_kind_on_addressable_entity(&runtime_context);
        let entity = match transform_kind {
            TransformKind::Write(StoredValue::AddressableEntity(entity)) => entity,
            _ => panic!("Invalid transform operation found"),
        };

        let actual = entity.associated_keys().get(&account_hash);

        assert!(actual.is_none());

        // Remove a key that was already removed
        runtime_context
            .remove_associated_key(account_hash)
            .expect_err("A non existing key was unexpectedly removed again");

        Ok(())
    };
    let _ = build_runtime_context_and_execute(named_keys, query);
}

#[test]
fn action_thresholds_management() {
    // Testing a valid case only - successfully added a key, and successfully removed,
    // making sure `account_dirty` mutated
    let named_keys = NamedKeys::new();
    let query = |mut runtime_context: RuntimeContext<LmdbGlobalStateView>| {
        let entity_hash_by_account_hash =
            CLValue::from_t(Key::Hash([2; 32])).expect("must convert to cl_value");

        runtime_context
            .metered_write_gs_unsafe(
                Key::Account(AccountHash::new([42; 32])),
                entity_hash_by_account_hash,
            )
            .expect("must write key to gs");

        runtime_context
            .add_associated_key(AccountHash::new([42; 32]), Weight::new(254))
            .expect("Unable to add associated key with maximum weight");
        runtime_context
            .set_action_threshold(ActionType::KeyManagement, Weight::new(253))
            .expect("Unable to set action threshold KeyManagement");
        runtime_context
            .set_action_threshold(ActionType::Deployment, Weight::new(252))
            .expect("Unable to set action threshold Deployment");

        let transform_kind = last_transform_kind_on_addressable_entity(&runtime_context);
        let mutated_entity = match transform_kind {
            TransformKind::Write(StoredValue::AddressableEntity(entity)) => entity,
            _ => panic!("Invalid transform operation found"),
        };

        assert_eq!(
            mutated_entity.action_thresholds().deployment(),
            &Weight::new(252)
        );
        assert_eq!(
            mutated_entity.action_thresholds().key_management(),
            &Weight::new(253)
        );

        runtime_context
            .set_action_threshold(ActionType::Deployment, Weight::new(255))
            .expect_err("Shouldn't be able to set deployment threshold higher than key management");

        Ok(())
    };
    let _ = build_runtime_context_and_execute(named_keys, query);
}

#[test]
fn should_verify_ownership_before_adding_key() {
    // Testing a valid case only - successfully added a key, and successfully removed,
    // making sure `account_dirty` mutated
    let named_keys = NamedKeys::new();
    let query = |mut runtime_context: RuntimeContext<LmdbGlobalStateView>| {
        // Overwrites a `base_key` to a different one before doing any operation as
        // account `[0; 32]`
        let entity_hash_by_account_hash =
            CLValue::from_t(Key::Hash([2; 32])).expect("must convert to cl_value");

        runtime_context
            .metered_write_gs_unsafe(
                Key::Account(AccountHash::new([84; 32])),
                entity_hash_by_account_hash,
            )
            .expect("must write key to gs");

        runtime_context
            .metered_write_gs_unsafe(Key::Hash([1; 32]), AddressableEntity::default())
            .expect("must write key to gs");

        runtime_context.entity_key = Key::Hash([1; 32]);

        let err = runtime_context
            .add_associated_key(AccountHash::new([84; 32]), Weight::new(123))
            .expect_err("This operation should return error");

        match err {
            Error::AddKeyFailure(AddKeyFailure::PermissionDenied) => {}
            e => panic!("Invalid error variant: {:?}", e),
        }

        Ok(())
    };
    let _ = build_runtime_context_and_execute(named_keys, query);
}

#[test]
fn should_verify_ownership_before_removing_a_key() {
    // Testing a valid case only - successfully added a key, and successfully removed,
    // making sure `account_dirty` mutated
    let named_keys = NamedKeys::new();
    let query = |mut runtime_context: RuntimeContext<LmdbGlobalStateView>| {
        // Overwrites a `base_key` to a different one before doing any operation as
        // account `[0; 32]`
        runtime_context.entity_key = Key::Hash([1; 32]);

        let err = runtime_context
            .remove_associated_key(AccountHash::new([84; 32]))
            .expect_err("This operation should return error");

        match err {
            Error::RemoveKeyFailure(RemoveKeyFailure::PermissionDenied) => {}
            ref e => panic!("Invalid error variant: {:?}", e),
        }

        Ok(())
    };
    let _ = build_runtime_context_and_execute(named_keys, query);
}

#[test]
fn should_verify_ownership_before_setting_action_threshold() {
    // Testing a valid case only - successfully added a key, and successfully removed,
    // making sure `account_dirty` mutated
    let named_keys = NamedKeys::new();
    let query = |mut runtime_context: RuntimeContext<LmdbGlobalStateView>| {
        // Overwrites a `base_key` to a different one before doing any operation as
        // account `[0; 32]`
        runtime_context.entity_key = Key::Hash([1; 32]);

        let err = runtime_context
            .set_action_threshold(ActionType::Deployment, Weight::new(123))
            .expect_err("This operation should return error");

        match err {
            Error::SetThresholdFailure(SetThresholdFailure::PermissionDeniedError) => {}
            ref e => panic!("Invalid error variant: {:?}", e),
        }

        Ok(())
    };
    let _ = build_runtime_context_and_execute(named_keys, query);
}

#[test]
fn can_roundtrip_key_value_pairs() {
    let named_keys = NamedKeys::new();
    let query = |mut runtime_context: RuntimeContext<LmdbGlobalStateView>| {
        let deploy_hash = [1u8; 32];
        let mut uref_address_generator = AddressGenerator::new(&deploy_hash, Phase::Session);
        let test_uref = create_uref_as_key(&mut uref_address_generator, AccessRights::default())
            .as_uref()
            .cloned()
            .expect("must have created URef from the key");
        let test_value = CLValue::from_t("test_value".to_string()).unwrap();

        runtime_context
            .write_purse_uref(test_uref.to_owned(), test_value.clone())
            .expect("should write_ls");

        let result = runtime_context
            .read_purse_uref(&test_uref)
            .expect("should read_ls");

        Ok(result == Some(test_value))
    };
    let query_result = build_runtime_context_and_execute(named_keys, query).expect("should be ok");
    assert!(query_result)
}

#[test]
fn remove_uref_works() {
    // Test that `remove_uref` removes Key from both ephemeral representation
    // which is one of the current RuntimeContext, and also puts that change
    // into the `TrackingCopy` so that it's later committed to the GlobalState.
    let deploy_hash = [1u8; 32];
    let mut address_generator = AddressGenerator::new(&deploy_hash, Phase::Session);
    let uref_name = "Foo".to_owned();
    let uref_key = create_uref_as_key(&mut address_generator, AccessRights::READ);
    let account_hash = AccountHash::new([0u8; 32]);
    let entity_hash = AddressableEntityHash::new([1u8; 32]);
    let mut named_keys = NamedKeys::new();
    named_keys.insert(uref_name.clone(), uref_key);
    let (_, entity_key, addressable_entity) = new_addressable_entity(account_hash, entity_hash);

    let access_rights = addressable_entity.extract_access_rights(entity_hash, &named_keys);

    let (mut runtime_context, _tempdir) = new_runtime_context(
        &addressable_entity,
        account_hash,
        entity_key,
        &mut named_keys,
        access_rights,
        address_generator,
    );

    assert!(runtime_context.named_keys_contains_key(&uref_name));
    assert!(runtime_context.remove_key(&uref_name).is_ok());
    // It is valid to retain the access right for the given runtime context
    // even if you remove the URef from the named keys.
    assert!(runtime_context.validate_key(&uref_key).is_ok());
    assert!(!runtime_context.named_keys_contains_key(&uref_name));
    let entity = runtime_context.get_entity();

    let entity_named_keys = runtime_context
        .get_named_keys(entity_key)
        .expect("must get named keys for entity");
    assert!(!entity_named_keys.contains(&uref_name));
    // The next time the account is used, the access right is gone for the removed
    // named key.
    let next_session_access_rights = entity.extract_access_rights(entity_hash, &named_keys);
    let address_generator = AddressGenerator::new(&deploy_hash, Phase::Session);
    let (runtime_context, _tempdir) = new_runtime_context(
        &entity,
        account_hash,
        entity_key,
        &mut named_keys,
        next_session_access_rights,
        address_generator,
    );
    assert!(runtime_context.validate_key(&uref_key).is_err());
}

#[test]
fn an_accounts_access_rights_should_include_main_purse() {
    let test_main_purse = URef::new([42u8; 32], AccessRights::READ_ADD_WRITE);
    // All other access rights except for main purse are extracted from named keys.
    let account_hash = AccountHash::new([0u8; 32]);
    let entity_hash = AddressableEntityHash::new([1u8; 32]);
    let named_keys = NamedKeys::new();
    let (_base_key, _, entity) = new_addressable_entity_with_purse(
        account_hash,
        entity_hash,
        EntityKind::Account(account_hash),
        test_main_purse.addr(),
    );
    assert!(
        named_keys.is_empty(),
        "Named keys does not contain main purse"
    );
    let access_rights = entity.extract_access_rights(entity_hash, &named_keys);
    assert!(
        access_rights.has_access_rights_to_uref(&test_main_purse),
        "Main purse should be included in access rights"
    );
}

#[test]
fn validate_valid_purse_of_an_account() {
    // Tests that URef which matches a purse of a given context gets validated
    let test_main_purse = URef::new([42u8; 32], AccessRights::READ_ADD_WRITE);

    let mut named_keys = NamedKeys::new();
    named_keys.insert("entry".to_string(), Key::from(test_main_purse));

    let deploy_hash = [1u8; 32];
    let account_hash = AccountHash::new([0u8; 32]);
    let entity_hash = AddressableEntityHash::new([1u8; 32]);
    let (base_key, _, entity) = new_addressable_entity_with_purse(
        account_hash,
        entity_hash,
        EntityKind::Account(account_hash),
        test_main_purse.addr(),
    );

    let mut access_rights = entity.extract_access_rights(entity_hash, &named_keys);
    access_rights.extend(&[test_main_purse]);

    let address_generator = AddressGenerator::new(&deploy_hash, Phase::Session);
    let (runtime_context, _tempdir) = new_runtime_context(
        &entity,
        account_hash,
        base_key,
        &mut named_keys,
        access_rights,
        address_generator,
    );

    // URef that has the same id as purse of an account gets validated
    // successfully.
    assert!(runtime_context.validate_uref(&test_main_purse).is_ok());

    let purse = test_main_purse.with_access_rights(AccessRights::READ);
    assert!(runtime_context.validate_uref(&purse).is_ok());
    let purse = test_main_purse.with_access_rights(AccessRights::ADD);
    assert!(runtime_context.validate_uref(&purse).is_ok());
    let purse = test_main_purse.with_access_rights(AccessRights::WRITE);
    assert!(runtime_context.validate_uref(&purse).is_ok());

    // Purse ID that doesn't match account's purse should fail as it's also not
    // in known urefs.
    let purse = URef::new([53; 32], AccessRights::READ_ADD_WRITE);
    assert!(runtime_context.validate_uref(&purse).is_err());
}

#[test]
fn should_meter_for_gas_storage_write() {
    // Test fixture
    let mut rng = AddressGenerator::new(&DEPLOY_HASH, PHASE);
    let uref_as_key = create_uref_as_key(&mut rng, AccessRights::READ_WRITE);

    let mut named_keys = NamedKeys::new();
    named_keys.insert("entry".to_string(), uref_as_key);

    let value = StoredValue::CLValue(CLValue::from_t(43_i32).unwrap());
    let expected_write_cost = test_engine_config()
        .wasm_config()
        .storage_costs()
        .calculate_gas_cost(value.serialized_length());

    let (gas_usage_before, gas_usage_after) =
        build_runtime_context_and_execute(named_keys, |mut rc| {
            let gas_before = rc.gas_counter();
            rc.metered_write_gs(uref_as_key, value)
                .expect("should write");
            let gas_after = rc.gas_counter();
            Ok((gas_before, gas_after))
        })
        .expect("should run test");

    assert!(
        gas_usage_after > gas_usage_before,
        "{} <= {}",
        gas_usage_after,
        gas_usage_before
    );

    assert_eq!(gas_usage_after, gas_usage_before + expected_write_cost);
}

#[test]
fn should_meter_for_gas_storage_add() {
    // Test fixture
    let mut rng = AddressGenerator::new(&DEPLOY_HASH, PHASE);
    let uref_as_key = create_uref_as_key(&mut rng, AccessRights::ADD_WRITE);

    let mut named_keys = NamedKeys::new();
    named_keys.insert("entry".to_string(), uref_as_key);

    let value = StoredValue::CLValue(CLValue::from_t(43_i32).unwrap());
    let expected_add_cost = test_engine_config()
        .wasm_config()
        .storage_costs()
        .calculate_gas_cost(value.serialized_length());

    let (gas_usage_before, gas_usage_after) =
        build_runtime_context_and_execute(named_keys, |mut rc| {
            rc.metered_write_gs(uref_as_key, value.clone())
                .expect("should write");
            let gas_before = rc.gas_counter();
            rc.metered_add_gs(uref_as_key, value).expect("should add");
            let gas_after = rc.gas_counter();
            Ok((gas_before, gas_after))
        })
        .expect("should run test");

    assert!(
        gas_usage_after > gas_usage_before,
        "{} <= {}",
        gas_usage_after,
        gas_usage_before
    );

    assert_eq!(gas_usage_after, gas_usage_before + expected_add_cost);
}

#[test]
fn associated_keys_add_full() {
    let final_add_result = build_runtime_context_and_execute(NamedKeys::new(), |mut rc| {
        let associated_keys_before = rc.entity().associated_keys().len();

        for count in 0..(rc.engine_config.max_associated_keys() as usize - associated_keys_before) {
            let account_hash = {
                let mut addr = [0; ACCOUNT_HASH_LENGTH];
                U256::from(count).to_big_endian(&mut addr);
                AccountHash::new(addr)
            };
            let weight = Weight::new(count.try_into().unwrap());
            rc.add_associated_key(account_hash, weight)
                .unwrap_or_else(|e| panic!("should add key {}: {:?}", count, e));
        }

        rc.add_associated_key(AccountHash::new([42; 32]), Weight::new(42))
    });

    assert!(matches!(
        final_add_result.expect_err("should error out"),
        Error::AddKeyFailure(AddKeyFailure::MaxKeysLimit)
    ));
}
