use std::{cell::Cell, iter, rc::Rc};

use assert_matches::assert_matches;
use proptest::prelude::*;

use casper_hashing::Digest;
use casper_types::{
    account::{Account, AccountHash, AssociatedKeys, Weight, ACCOUNT_HASH_LENGTH},
    contracts::NamedKeys,
    gens::*,
    AccessRights, CLValue, Contract, EntryPoints, HashAddr, Key, KeyTag, ProtocolVersion,
    StoredValue, URef, U256, U512,
};

use super::{
    meter::count_meter::Count, AddResult, TrackingCopy, TrackingCopyCache, TrackingCopyQueryResult,
};
use crate::{
    core::{engine_state::EngineConfig, runtime_context::dictionary, ValidationError},
    shared::{execution_journal::ExecutionJournal, newtypes::CorrelationId, transform::Transform},
    storage::{
        global_state::{in_memory::InMemoryGlobalState, StateProvider, StateReader},
        trie::merkle_proof::TrieMerkleProof,
    },
};

struct CountingDb {
    count: Rc<Cell<i32>>,
    value: Option<StoredValue>,
}

impl CountingDb {
    fn new(counter: Rc<Cell<i32>>) -> CountingDb {
        CountingDb {
            count: counter,
            value: None,
        }
    }

    fn new_init(v: StoredValue) -> CountingDb {
        CountingDb {
            count: Rc::new(Cell::new(0)),
            value: Some(v),
        }
    }
}

impl StateReader<Key, StoredValue> for CountingDb {
    type Error = String;
    fn read(
        &self,
        _correlation_id: CorrelationId,
        _key: &Key,
    ) -> Result<Option<StoredValue>, Self::Error> {
        let count = self.count.get();
        let value = match self.value {
            Some(ref v) => v.clone(),
            None => StoredValue::CLValue(CLValue::from_t(count).unwrap()),
        };
        self.count.set(count + 1);
        Ok(Some(value))
    }

    fn read_with_proof(
        &self,
        _correlation_id: CorrelationId,
        _key: &Key,
    ) -> Result<Option<TrieMerkleProof<Key, StoredValue>>, Self::Error> {
        Ok(None)
    }

    fn keys_with_prefix(
        &self,
        _correlation_id: CorrelationId,
        _prefix: &[u8],
    ) -> Result<Vec<Key>, Self::Error> {
        Ok(Vec::new())
    }
}

#[test]
fn tracking_copy_new() {
    let counter = Rc::new(Cell::new(0));
    let db = CountingDb::new(counter);
    let tc = TrackingCopy::new(db);

    assert!(tc.journal.is_empty());
}

#[test]
fn tracking_copy_caching() {
    let correlation_id = CorrelationId::new();
    let counter = Rc::new(Cell::new(0));
    let db = CountingDb::new(Rc::clone(&counter));
    let mut tc = TrackingCopy::new(db);
    let k = Key::Hash([0u8; 32]);

    let zero = StoredValue::CLValue(CLValue::from_t(0_i32).unwrap());
    // first read
    let value = tc.read(correlation_id, &k).unwrap().unwrap();
    assert_eq!(value, zero);

    // second read; should use cache instead
    // of going back to the DB
    let value = tc.read(correlation_id, &k).unwrap().unwrap();
    let db_value = counter.get();
    assert_eq!(value, zero);
    assert_eq!(db_value, 1);
}

#[test]
fn tracking_copy_read() {
    let correlation_id = CorrelationId::new();
    let counter = Rc::new(Cell::new(0));
    let db = CountingDb::new(Rc::clone(&counter));
    let mut tc = TrackingCopy::new(db);
    let k = Key::Hash([0u8; 32]);

    let zero = StoredValue::CLValue(CLValue::from_t(0_i32).unwrap());
    let value = tc.read(correlation_id, &k).unwrap().unwrap();
    // value read correctly
    assert_eq!(value, zero);
    // Reading does produce an identity transform.
    assert_eq!(
        tc.journal,
        ExecutionJournal::new(vec![(k, Transform::Identity)])
    );
}

#[test]
fn tracking_copy_write() {
    let counter = Rc::new(Cell::new(0));
    let db = CountingDb::new(Rc::clone(&counter));
    let mut tc = TrackingCopy::new(db);
    let k = Key::Hash([0u8; 32]);

    let one = StoredValue::CLValue(CLValue::from_t(1_i32).unwrap());
    let two = StoredValue::CLValue(CLValue::from_t(2_i32).unwrap());

    // writing should work
    let _ = tc.write(k, one.clone());
    // write does not need to query the DB
    let db_value = counter.get();
    assert_eq!(db_value, 0);
    // Writing creates a write transform.
    assert_eq!(
        tc.journal,
        ExecutionJournal::new(vec![(k, Transform::Write(one.clone()))])
    );

    // writing again should update the values
    let _ = tc.write(k, two.clone());
    let db_value = counter.get();
    assert_eq!(db_value, 0);
    assert_eq!(
        tc.journal,
        ExecutionJournal::new(vec![(k, Transform::Write(one)), (k, Transform::Write(two))])
    );
}

#[test]
fn tracking_copy_add_i32() {
    let correlation_id = CorrelationId::new();
    let counter = Rc::new(Cell::new(0));
    let db = CountingDb::new(counter);
    let mut tc = TrackingCopy::new(db);
    let k = Key::Hash([0u8; 32]);

    let three = StoredValue::CLValue(CLValue::from_t(3_i32).unwrap());

    // adding should work
    let add = tc.add(correlation_id, k, three.clone());
    assert_matches!(add, Ok(_));

    // Adding creates an add transform.
    assert_eq!(
        tc.journal,
        ExecutionJournal::new(vec![(k, Transform::AddInt32(3))])
    );

    // adding again should update the values
    let add = tc.add(correlation_id, k, three);
    assert_matches!(add, Ok(_));
    assert_eq!(
        tc.journal,
        ExecutionJournal::new(vec![(k, Transform::AddInt32(3)); 2])
    );
}

#[test]
fn tracking_copy_add_named_key() {
    let zero_account_hash = AccountHash::new([0u8; ACCOUNT_HASH_LENGTH]);
    let correlation_id = CorrelationId::new();
    // DB now holds an `Account` so that we can test adding a `NamedKey`
    let associated_keys = AssociatedKeys::new(zero_account_hash, Weight::new(1));
    let account = Account::new(
        zero_account_hash,
        NamedKeys::new(),
        URef::new([0u8; 32], AccessRights::READ_ADD_WRITE),
        associated_keys,
        Default::default(),
    );
    let db = CountingDb::new_init(StoredValue::Account(account));
    let mut tc = TrackingCopy::new(db);
    let k = Key::Hash([0u8; 32]);
    let u1 = Key::URef(URef::new([1u8; 32], AccessRights::READ_WRITE));
    let u2 = Key::URef(URef::new([2u8; 32], AccessRights::READ_WRITE));

    let name1 = "test".to_string();
    let named_key = StoredValue::CLValue(CLValue::from_t((name1.clone(), u1)).unwrap());
    let name2 = "test2".to_string();
    let other_named_key = StoredValue::CLValue(CLValue::from_t((name2.clone(), u2)).unwrap());
    let mut map = NamedKeys::new();
    map.insert(name1.clone(), u1);

    // adding the wrong type should fail
    let failed_add = tc.add(
        correlation_id,
        k,
        StoredValue::CLValue(CLValue::from_t(3_i32).unwrap()),
    );
    assert_matches!(failed_add, Ok(AddResult::TypeMismatch(_)));
    assert!(tc.journal.is_empty());

    // adding correct type works
    let add = tc.add(correlation_id, k, named_key);
    assert_matches!(add, Ok(_));
    assert_eq!(
        tc.journal,
        ExecutionJournal::new(vec![(
            k,
            Transform::AddKeys(iter::once((name1.clone(), u1)).collect())
        )])
    );

    // adding again updates the values
    map.insert(name2.clone(), u2);
    let add = tc.add(correlation_id, k, other_named_key);
    assert_matches!(add, Ok(_));
    assert_eq!(
        tc.journal,
        ExecutionJournal::new(vec![
            (k, Transform::AddKeys(iter::once((name1, u1)).collect())),
            (k, Transform::AddKeys(iter::once((name2, u2)).collect()))
        ])
    );
}

#[test]
fn tracking_copy_rw() {
    let correlation_id = CorrelationId::new();
    let counter = Rc::new(Cell::new(0));
    let db = CountingDb::new(counter);
    let mut tc = TrackingCopy::new(db);
    let k = Key::Hash([0u8; 32]);

    // reading then writing should update the op
    let value = StoredValue::CLValue(CLValue::from_t(3_i32).unwrap());
    let _ = tc.read(correlation_id, &k);
    let _ = tc.write(k, value.clone());
    assert_eq!(
        tc.journal,
        ExecutionJournal::new(vec![(k, Transform::Identity), (k, Transform::Write(value))])
    );
}

#[test]
fn tracking_copy_ra() {
    let correlation_id = CorrelationId::new();
    let counter = Rc::new(Cell::new(0));
    let db = CountingDb::new(counter);
    let mut tc = TrackingCopy::new(db);
    let k = Key::Hash([0u8; 32]);

    // reading then adding should update the op
    let value = StoredValue::CLValue(CLValue::from_t(3_i32).unwrap());
    let _ = tc.read(correlation_id, &k);
    let _ = tc.add(correlation_id, k, value);
    assert_eq!(
        tc.journal,
        ExecutionJournal::new(vec![(k, Transform::Identity), (k, Transform::AddInt32(3))])
    );
}

#[test]
fn tracking_copy_aw() {
    let correlation_id = CorrelationId::new();
    let counter = Rc::new(Cell::new(0));
    let db = CountingDb::new(counter);
    let mut tc = TrackingCopy::new(db);
    let k = Key::Hash([0u8; 32]);

    // adding then writing should update the op
    let value = StoredValue::CLValue(CLValue::from_t(3_i32).unwrap());
    let write_value = StoredValue::CLValue(CLValue::from_t(7_i32).unwrap());
    let _ = tc.add(correlation_id, k, value);
    let _ = tc.write(k, write_value.clone());
    assert_eq!(
        tc.journal,
        ExecutionJournal::new(vec![
            (k, Transform::AddInt32(3)),
            (k, Transform::Write(write_value))
        ])
    );
}

proptest! {
    #[test]
    fn query_empty_path(k in key_arb(), missing_key in key_arb(), v in stored_value_arb()) {
        let correlation_id = CorrelationId::new();

        let value = dictionary::handle_stored_value_into(k, v.clone()).unwrap();

        let (gs, root_hash) = InMemoryGlobalState::from_pairs(correlation_id, &[(k, value)]).unwrap();
        let view = gs.checkout(root_hash).unwrap().unwrap();
        let tc = TrackingCopy::new(view);
        let empty_path = Vec::new();
        if let Ok(TrackingCopyQueryResult::Success { value, .. }) = tc.query(correlation_id, &EngineConfig::default(), k, &empty_path) {
            assert_eq!(v, value);
        } else {
            panic!("Query failed when it should not have!");
        }

        if missing_key != k {
            let result = tc.query(correlation_id, &EngineConfig::default(), missing_key, &empty_path);
            assert_matches!(result, Ok(TrackingCopyQueryResult::ValueNotFound(_)));
        }
    }

    #[test]
    fn query_contract_state(
        k in key_arb(), // key state is stored at
        v in stored_value_arb(), // value in contract state
        name in "\\PC*", // human-readable name for state
        missing_name in "\\PC*",
        hash in u8_slice_32(), // hash for contract key
    ) {
        let correlation_id = CorrelationId::new();
        let mut named_keys = NamedKeys::new();
        named_keys.insert(name.clone(), k);
        let contract =
            StoredValue::Contract(Contract::new(
            [2; 32].into(),
            [3; 32].into(),
            named_keys,
            EntryPoints::default(),
            ProtocolVersion::V1_0_0,
        ));
        let contract_key = Key::Hash(hash);

        let value = dictionary::handle_stored_value_into(k, v.clone()).unwrap();

        let (gs, root_hash) = InMemoryGlobalState::from_pairs(
            correlation_id,
            &[(k, value), (contract_key, contract)]
        ).unwrap();
        let view = gs.checkout(root_hash).unwrap().unwrap();
        let tc = TrackingCopy::new(view);
        let path = vec!(name.clone());
        if let Ok(TrackingCopyQueryResult::Success { value, .. }) = tc.query(correlation_id, &EngineConfig::default(), contract_key, &path) {
            assert_eq!(v, value);
        } else {
            panic!("Query failed when it should not have!");
        }

        if missing_name != name {
            let result = tc.query(correlation_id, &EngineConfig::default(), contract_key, &[missing_name]);
            assert_matches!(result, Ok(TrackingCopyQueryResult::ValueNotFound(_)));
        }
    }

    #[test]
    fn query_account_state(
        k in key_arb(), // key state is stored at
        v in stored_value_arb(), // value in account state
        name in "\\PC*", // human-readable name for state
        missing_name in "\\PC*",
        pk in account_hash_arb(), // account hash
        address in account_hash_arb(), // address for account hash
    ) {
        let correlation_id = CorrelationId::new();
        let named_keys = iter::once((name.clone(), k)).collect();
        let purse = URef::new([0u8; 32], AccessRights::READ_ADD_WRITE);
        let associated_keys = AssociatedKeys::new(pk, Weight::new(1));
        let account = Account::new(
            pk,
            named_keys,
            purse,
            associated_keys,
            Default::default(),
        );
        let account_key = Key::Account(address);

        let value = dictionary::handle_stored_value_into(k, v.clone()).unwrap();

        let (gs, root_hash) = InMemoryGlobalState::from_pairs(
            correlation_id,
            &[(k, value), (account_key, StoredValue::Account(account))],
        ).unwrap();
        let view = gs.checkout(root_hash).unwrap().unwrap();
        let tc = TrackingCopy::new(view);
        let path = vec!(name.clone());
        if let Ok(TrackingCopyQueryResult::Success { value, .. }) = tc.query(correlation_id, &EngineConfig::default(),account_key, &path) {
            assert_eq!(v, value);
        } else {
            panic!("Query failed when it should not have!");
        }

        if missing_name != name {
            let result = tc.query(correlation_id, &EngineConfig::default(), account_key, &[missing_name]);
            assert_matches!(result, Ok(TrackingCopyQueryResult::ValueNotFound(_)));
        }
    }

    #[test]
    fn query_path(
        k in key_arb(), // key state is stored at
        v in stored_value_arb(), // value in contract state
        state_name in "\\PC*", // human-readable name for state
        contract_name in "\\PC*", // human-readable name for contract
        pk in account_hash_arb(), // account hash
        address in account_hash_arb(), // address for account hash
        hash in u8_slice_32(), // hash for contract key
    ) {
        let correlation_id = CorrelationId::new();
        // create contract which knows about value
        let mut contract_named_keys = NamedKeys::new();
        contract_named_keys.insert(state_name.clone(), k);
        let contract =
            StoredValue::Contract(Contract::new(
            [2; 32].into(),
            [3; 32].into(),
            contract_named_keys,
            EntryPoints::default(),
            ProtocolVersion::V1_0_0,
        ));
        let contract_key = Key::Hash(hash);

        // create account which knows about contract
        let mut account_named_keys = NamedKeys::new();
        account_named_keys.insert(contract_name.clone(), contract_key);
        let purse = URef::new([0u8; 32], AccessRights::READ_ADD_WRITE);
        let associated_keys = AssociatedKeys::new(pk, Weight::new(1));
        let account = Account::new(
            pk,
            account_named_keys,
            purse,
            associated_keys,
            Default::default(),
        );
        let account_key = Key::Account(address);

        let value = dictionary::handle_stored_value_into(k, v.clone()).unwrap();

        let (gs, root_hash) = InMemoryGlobalState::from_pairs(correlation_id, &[
            (k, value),
            (contract_key, contract),
            (account_key, StoredValue::Account(account)),
        ]).unwrap();
        let view = gs.checkout(root_hash).unwrap().unwrap();
        let tc = TrackingCopy::new(view);
        let path = vec!(contract_name, state_name);

        let results =  tc.query(correlation_id, &EngineConfig::default(), account_key, &path);
        if let Ok(TrackingCopyQueryResult::Success { value, .. }) = results {
            assert_eq!(v, value);
        } else {
            panic!("Query failed when it should not have!");
        }
    }
}

#[test]
fn cache_reads_invalidation() {
    let mut tc_cache = TrackingCopyCache::new(2, Count);
    let (k1, v1) = (
        Key::Hash([1u8; 32]),
        StoredValue::CLValue(CLValue::from_t(1_i32).unwrap()),
    );
    let (k2, v2) = (
        Key::Hash([2u8; 32]),
        StoredValue::CLValue(CLValue::from_t(2_i32).unwrap()),
    );
    let (k3, v3) = (
        Key::Hash([3u8; 32]),
        StoredValue::CLValue(CLValue::from_t(3_i32).unwrap()),
    );
    tc_cache.insert_read(k1, v1);
    tc_cache.insert_read(k2, v2.clone());
    tc_cache.insert_read(k3, v3.clone());
    assert!(tc_cache.get(&k1).is_none()); // first entry should be invalidated
    assert_eq!(tc_cache.get(&k2), Some(&v2)); // k2 and k3 should be there
    assert_eq!(tc_cache.get(&k3), Some(&v3));
}

#[test]
fn cache_writes_not_invalidated() {
    let mut tc_cache = TrackingCopyCache::new(2, Count);
    let (k1, v1) = (
        Key::Hash([1u8; 32]),
        StoredValue::CLValue(CLValue::from_t(1_i32).unwrap()),
    );
    let (k2, v2) = (
        Key::Hash([2u8; 32]),
        StoredValue::CLValue(CLValue::from_t(2_i32).unwrap()),
    );
    let (k3, v3) = (
        Key::Hash([3u8; 32]),
        StoredValue::CLValue(CLValue::from_t(3_i32).unwrap()),
    );
    tc_cache.insert_write(k1, v1.clone());
    tc_cache.insert_read(k2, v2.clone());
    tc_cache.insert_read(k3, v3.clone());
    // Writes are not subject to cache invalidation
    assert_eq!(tc_cache.get(&k1), Some(&v1));
    assert_eq!(tc_cache.get(&k2), Some(&v2)); // k2 and k3 should be there
    assert_eq!(tc_cache.get(&k3), Some(&v3));
}

#[test]
fn query_for_circular_references_should_fail() {
    // create self-referential key
    let cl_value_key = Key::URef(URef::new([255; 32], AccessRights::READ));
    let cl_value = StoredValue::CLValue(CLValue::from_t(cl_value_key).unwrap());
    let key_name = "key".to_string();

    // create contract with this self-referential key in its named keys, and also a key referring to
    // itself in its named keys.
    let contract_key = Key::Hash([1; 32]);
    let contract_name = "contract".to_string();
    let mut named_keys = NamedKeys::new();
    named_keys.insert(key_name.clone(), cl_value_key);
    named_keys.insert(contract_name.clone(), contract_key);
    let contract = StoredValue::Contract(Contract::new(
        [2; 32].into(),
        [3; 32].into(),
        named_keys,
        EntryPoints::default(),
        ProtocolVersion::V1_0_0,
    ));

    let correlation_id = CorrelationId::new();
    let (global_state, root_hash) = InMemoryGlobalState::from_pairs(
        correlation_id,
        &[(cl_value_key, cl_value), (contract_key, contract)],
    )
    .unwrap();
    let view = global_state.checkout(root_hash).unwrap().unwrap();
    let tracking_copy = TrackingCopy::new(view);

    // query for the self-referential key (second path element of arbitrary value required to cause
    // iteration _into_ the self-referential key)
    let path = vec![key_name, String::new()];
    if let Ok(TrackingCopyQueryResult::CircularReference(msg)) = tracking_copy.query(
        correlation_id,
        &EngineConfig::default(),
        contract_key,
        &path,
    ) {
        let expected_path_msg = format!("at path: {:?}/{}", contract_key, path[0]);
        assert!(msg.contains(&expected_path_msg));
    } else {
        panic!("Query didn't fail with a circular reference error");
    }

    // query for itself in its own named keys
    let path = vec![contract_name];
    if let Ok(TrackingCopyQueryResult::CircularReference(msg)) = tracking_copy.query(
        correlation_id,
        &EngineConfig::default(),
        contract_key,
        &path,
    ) {
        let expected_path_msg = format!("at path: {:?}/{}", contract_key, path[0]);
        assert!(msg.contains(&expected_path_msg));
    } else {
        panic!("Query didn't fail with a circular reference error");
    }
}

#[test]
fn validate_query_proof_should_work() {
    // create account
    let account_hash = AccountHash::new([3; 32]);
    let fake_purse = URef::new([4; 32], AccessRights::READ_ADD_WRITE);
    let account_value = StoredValue::Account(Account::create(
        account_hash,
        NamedKeys::default(),
        fake_purse,
    ));
    let account_key = Key::Account(account_hash);

    // create contract that refers to that account
    let account_name = "account".to_string();
    let named_keys = {
        let mut tmp = NamedKeys::new();
        tmp.insert(account_name.clone(), account_key);
        tmp
    };
    let contract_value = StoredValue::Contract(Contract::new(
        [2; 32].into(),
        [3; 32].into(),
        named_keys,
        EntryPoints::default(),
        ProtocolVersion::V1_0_0,
    ));
    let contract_key = Key::Hash([5; 32]);

    // create account that refers to that contract
    let account_hash = AccountHash::new([7; 32]);
    let fake_purse = URef::new([6; 32], AccessRights::READ_ADD_WRITE);
    let contract_name = "contract".to_string();
    let named_keys = {
        let mut tmp = NamedKeys::new();
        tmp.insert(contract_name.clone(), contract_key);
        tmp
    };
    let main_account_value =
        StoredValue::Account(Account::create(account_hash, named_keys, fake_purse));
    let main_account_key = Key::Account(account_hash);

    // random value for proof injection attack
    let cl_value = CLValue::from_t(U512::zero()).expect("should convert");
    let uref_value = StoredValue::CLValue(cl_value);
    let uref_key = Key::URef(URef::new([8; 32], AccessRights::READ_ADD_WRITE));

    // persist them
    let correlation_id = CorrelationId::new();
    let (global_state, root_hash) = InMemoryGlobalState::from_pairs(
        correlation_id,
        &[
            (account_key, account_value.to_owned()),
            (contract_key, contract_value.to_owned()),
            (main_account_key, main_account_value.to_owned()),
            (uref_key, uref_value),
        ],
    )
    .unwrap();

    let view = global_state
        .checkout(root_hash)
        .expect("should checkout")
        .expect("should have view");

    let tracking_copy = TrackingCopy::new(view);

    let path = &[contract_name, account_name];

    let result = tracking_copy
        .query(
            correlation_id,
            &EngineConfig::default(),
            main_account_key,
            path,
        )
        .expect("should query");

    let proofs = if let TrackingCopyQueryResult::Success { proofs, .. } = result {
        proofs
    } else {
        panic!("query was not successful: {:?}", result)
    };

    // Happy path
    crate::core::validate_query_proof(&root_hash, &proofs, &main_account_key, path, &account_value)
        .expect("should validate");

    // Path should be the same length as the proofs less one (so it should be of length 2)
    assert_eq!(
        crate::core::validate_query_proof(
            &root_hash,
            &proofs,
            &main_account_key,
            &[],
            &account_value
        ),
        Err(ValidationError::PathLengthDifferentThanProofLessOne)
    );

    // Find an unexpected value after tracing the proof
    assert_eq!(
        crate::core::validate_query_proof(
            &root_hash,
            &proofs,
            &main_account_key,
            path,
            &main_account_value
        ),
        Err(ValidationError::UnexpectedValue)
    );

    // Wrong key provided for the first entry in the proof
    assert_eq!(
        crate::core::validate_query_proof(&root_hash, &proofs, &account_key, path, &account_value),
        Err(ValidationError::UnexpectedKey)
    );

    // Bad proof hash
    assert_eq!(
        crate::core::validate_query_proof(
            &Digest::hash(&[]),
            &proofs,
            &main_account_key,
            path,
            &account_value
        ),
        Err(ValidationError::InvalidProofHash)
    );

    // Provided path contains an unexpected key
    assert_eq!(
        crate::core::validate_query_proof(
            &root_hash,
            &proofs,
            &main_account_key,
            &[
                "a non-existent path key 1".to_string(),
                "a non-existent path key 2".to_string()
            ],
            &account_value
        ),
        Err(ValidationError::PathCold)
    );

    let misfit_result = tracking_copy
        .query(correlation_id, &EngineConfig::default(), uref_key, &[])
        .expect("should query");

    let misfit_proof = if let TrackingCopyQueryResult::Success { proofs, .. } = misfit_result {
        proofs[0].to_owned()
    } else {
        panic!("query was not successful: {:?}", misfit_result)
    };

    // Proof has been subject to an injection
    assert_eq!(
        crate::core::validate_query_proof(
            &root_hash,
            &[
                proofs[0].to_owned(),
                misfit_proof.to_owned(),
                proofs[2].to_owned()
            ],
            &main_account_key,
            path,
            &account_value
        ),
        Err(ValidationError::UnexpectedKey)
    );

    // Proof has been subject to an injection
    assert_eq!(
        crate::core::validate_query_proof(
            &root_hash,
            &[
                misfit_proof.to_owned(),
                proofs[1].to_owned(),
                proofs[2].to_owned()
            ],
            &uref_key.normalize(),
            path,
            &account_value
        ),
        Err(ValidationError::PathCold)
    );

    // Proof has been subject to an injection
    assert_eq!(
        crate::core::validate_query_proof(
            &root_hash,
            &[misfit_proof, proofs[1].to_owned(), proofs[2].to_owned()],
            &uref_key.normalize(),
            path,
            &account_value
        ),
        Err(ValidationError::PathCold)
    );

    let (misfit_global_state, misfit_root_hash) = InMemoryGlobalState::from_pairs(
        correlation_id,
        &[
            (account_key, account_value.to_owned()),
            (contract_key, contract_value),
            (main_account_key, main_account_value),
        ],
    )
    .unwrap();

    let misfit_view = misfit_global_state
        .checkout(misfit_root_hash)
        .expect("should checkout")
        .expect("should have view");

    let misfit_tracking_copy = TrackingCopy::new(misfit_view);

    let misfit_result = misfit_tracking_copy
        .query(
            correlation_id,
            &EngineConfig::default(),
            main_account_key,
            path,
        )
        .expect("should query");

    let misfit_proof = if let TrackingCopyQueryResult::Success { proofs, .. } = misfit_result {
        proofs[1].to_owned()
    } else {
        panic!("query was not successful: {:?}", misfit_result)
    };

    // Proof has been subject to an injection
    assert_eq!(
        crate::core::validate_query_proof(
            &root_hash,
            &[proofs[0].to_owned(), misfit_proof, proofs[2].to_owned()],
            &main_account_key,
            path,
            &account_value
        ),
        Err(ValidationError::InvalidProofHash)
    );
}

#[test]
fn get_keys_should_return_keys_in_the_account_keyspace() {
    // account 1
    let account_1_hash = AccountHash::new([1; 32]);
    let fake_purse = URef::new([42; 32], AccessRights::READ_ADD_WRITE);
    let account_1_value = StoredValue::Account(Account::create(
        account_1_hash,
        NamedKeys::default(),
        fake_purse,
    ));
    let account_1_key = Key::Account(account_1_hash);

    // account 2
    let account_2_hash = AccountHash::new([2; 32]);
    let fake_purse = URef::new([43; 32], AccessRights::READ_ADD_WRITE);
    let account_2_value = StoredValue::Account(Account::create(
        account_2_hash,
        NamedKeys::default(),
        fake_purse,
    ));
    let account_2_key = Key::Account(account_2_hash);

    // random value
    let cl_value = CLValue::from_t(U512::zero()).expect("should convert");
    let uref_value = StoredValue::CLValue(cl_value);
    let uref_key = Key::URef(URef::new([8; 32], AccessRights::READ_ADD_WRITE));

    // persist them
    let correlation_id = CorrelationId::new();
    let (global_state, root_hash) = InMemoryGlobalState::from_pairs(
        correlation_id,
        &[
            (account_1_key, account_1_value),
            (account_2_key, account_2_value),
            (uref_key, uref_value),
        ],
    )
    .unwrap();

    let view = global_state
        .checkout(root_hash)
        .expect("should checkout")
        .expect("should have view");

    let mut tracking_copy = TrackingCopy::new(view);

    let key_set = tracking_copy
        .get_keys(correlation_id, &KeyTag::Account)
        .unwrap();

    assert_eq!(key_set.len(), 2);
    assert!(key_set.contains(&account_1_key));
    assert!(key_set.contains(&account_2_key));
    assert!(!key_set.contains(&uref_key));
}

#[test]
fn get_keys_should_return_keys_in_the_uref_keyspace() {
    // account
    let account_hash = AccountHash::new([1; 32]);
    let fake_purse = URef::new([42; 32], AccessRights::READ_ADD_WRITE);
    let account_value = StoredValue::Account(Account::create(
        account_hash,
        NamedKeys::default(),
        fake_purse,
    ));
    let account_key = Key::Account(account_hash);

    // random value 1
    let cl_value = CLValue::from_t(U512::zero()).expect("should convert");
    let uref_1_value = StoredValue::CLValue(cl_value);
    let uref_1_key = Key::URef(URef::new([8; 32], AccessRights::READ_ADD_WRITE));

    // random value 2
    let cl_value = CLValue::from_t(U512::one()).expect("should convert");
    let uref_2_value = StoredValue::CLValue(cl_value);
    let uref_2_key = Key::URef(URef::new([9; 32], AccessRights::READ_ADD_WRITE));

    // persist them
    let correlation_id = CorrelationId::new();
    let (global_state, root_hash) = InMemoryGlobalState::from_pairs(
        correlation_id,
        &[
            (account_key, account_value),
            (uref_1_key, uref_1_value),
            (uref_2_key, uref_2_value),
        ],
    )
    .unwrap();

    let view = global_state
        .checkout(root_hash)
        .expect("should checkout")
        .expect("should have view");

    let mut tracking_copy = TrackingCopy::new(view);

    let key_set = tracking_copy
        .get_keys(correlation_id, &KeyTag::URef)
        .unwrap();

    assert_eq!(key_set.len(), 2);
    assert!(key_set.contains(&uref_1_key.normalize()));
    assert!(key_set.contains(&uref_2_key.normalize()));
    assert!(!key_set.contains(&account_key));

    // random value 3
    let cl_value = CLValue::from_t(U512::from(2)).expect("should convert");
    let uref_3_value = StoredValue::CLValue(cl_value);
    let uref_3_key = Key::URef(URef::new([10; 32], AccessRights::READ_ADD_WRITE));
    let _ = tracking_copy.write(uref_3_key, uref_3_value);

    let key_set = tracking_copy
        .get_keys(correlation_id, &KeyTag::URef)
        .unwrap();

    assert_eq!(key_set.len(), 3);
    assert!(key_set.contains(&uref_1_key.normalize()));
    assert!(key_set.contains(&uref_2_key.normalize()));
    assert!(key_set.contains(&uref_3_key.normalize()));
    assert!(!key_set.contains(&account_key));
}

#[test]
fn get_keys_should_handle_reads_from_empty_trie() {
    let correlation_id = CorrelationId::new();
    let (global_state, root_hash) = InMemoryGlobalState::from_pairs(correlation_id, &[]).unwrap();

    let view = global_state
        .checkout(root_hash)
        .expect("should checkout")
        .expect("should have view");

    let mut tracking_copy = TrackingCopy::new(view);

    let key_set = tracking_copy
        .get_keys(correlation_id, &KeyTag::URef)
        .unwrap();

    assert_eq!(key_set.len(), 0);
    assert!(key_set.is_empty());

    // persist random value 1
    let cl_value = CLValue::from_t(U512::zero()).expect("should convert");
    let uref_1_value = StoredValue::CLValue(cl_value);
    let uref_1_key = Key::URef(URef::new([8; 32], AccessRights::READ_ADD_WRITE));
    let _ = tracking_copy.write(uref_1_key, uref_1_value);

    let key_set = tracking_copy
        .get_keys(correlation_id, &KeyTag::URef)
        .unwrap();

    assert_eq!(key_set.len(), 1);
    assert!(key_set.contains(&uref_1_key.normalize()));

    // persist random value 2
    let cl_value = CLValue::from_t(U512::one()).expect("should convert");
    let uref_2_value = StoredValue::CLValue(cl_value);
    let uref_2_key = Key::URef(URef::new([9; 32], AccessRights::READ_ADD_WRITE));
    let _ = tracking_copy.write(uref_2_key, uref_2_value);

    let key_set = tracking_copy
        .get_keys(correlation_id, &KeyTag::URef)
        .unwrap();

    assert_eq!(key_set.len(), 2);
    assert!(key_set.contains(&uref_1_key.normalize()));
    assert!(key_set.contains(&uref_2_key.normalize()));

    // persist account
    let account_hash = AccountHash::new([1; 32]);
    let fake_purse = URef::new([42; 32], AccessRights::READ_ADD_WRITE);
    let account_value = StoredValue::Account(Account::create(
        account_hash,
        NamedKeys::default(),
        fake_purse,
    ));
    let account_key = Key::Account(account_hash);
    let _ = tracking_copy.write(account_key, account_value);

    assert_eq!(key_set.len(), 2);
    assert!(key_set.contains(&uref_1_key.normalize()));
    assert!(key_set.contains(&uref_2_key.normalize()));
    assert!(!key_set.contains(&account_key));

    // persist random value 3
    let cl_value = CLValue::from_t(U512::from(2)).expect("should convert");
    let uref_3_value = StoredValue::CLValue(cl_value);
    let uref_3_key = Key::URef(URef::new([10; 32], AccessRights::READ_ADD_WRITE));
    let _ = tracking_copy.write(uref_3_key, uref_3_value);

    let key_set = tracking_copy
        .get_keys(correlation_id, &KeyTag::URef)
        .unwrap();

    assert_eq!(key_set.len(), 3);
    assert!(key_set.contains(&uref_1_key.normalize()));
    assert!(key_set.contains(&uref_2_key.normalize()));
    assert!(key_set.contains(&uref_3_key.normalize()));
    assert!(!key_set.contains(&account_key));
}

fn val_to_hashaddr<T: Into<U256>>(value: T) -> HashAddr {
    let mut addr = HashAddr::default();
    value.into().to_big_endian(&mut addr);
    addr
}

#[test]
fn query_with_large_depth_with_fixed_path_should_fail() {
    let engine_config = EngineConfig::default();

    let mut pairs = Vec::new();
    let mut contract_keys = Vec::new();
    let mut path = Vec::new();

    const WASM_OFFSET: u64 = 1_000_000;
    const PACKAGE_OFFSET: u64 = 1_000;

    // create a long chain of contract at address X with a named key that points to a contract X+1
    // which has a size that exceeds configured max query depth.
    for value in 1..=engine_config.max_query_depth {
        let contract_key = Key::Hash(val_to_hashaddr(value));
        let next_contract_key = Key::Hash(val_to_hashaddr(value + 1));
        let contract_name = format!("contract{}", value);

        let named_keys = {
            let mut named_keys = NamedKeys::new();
            named_keys.insert(contract_name.clone(), next_contract_key);
            named_keys
        };
        let contract = StoredValue::Contract(Contract::new(
            val_to_hashaddr(PACKAGE_OFFSET + value).into(),
            val_to_hashaddr(WASM_OFFSET + value).into(),
            named_keys,
            EntryPoints::default(),
            ProtocolVersion::V1_0_0,
        ));
        pairs.push((contract_key, contract));
        contract_keys.push(contract_key);
        path.push(contract_name.clone());
    }

    let correlation_id = CorrelationId::new();
    let (global_state, root_hash) =
        InMemoryGlobalState::from_pairs(correlation_id, &pairs).unwrap();

    let view = global_state.checkout(root_hash).unwrap().unwrap();
    let tracking_copy = TrackingCopy::new(view);

    let contract_key = contract_keys[0];
    let result = tracking_copy.query(correlation_id, &engine_config, contract_key, &path);

    assert!(
        matches!(result, Ok(TrackingCopyQueryResult::DepthLimit {
        depth
    }) if depth == engine_config.max_query_depth),
        "{:?}",
        result
    );
}

#[test]
fn query_with_large_depth_with_urefs_should_fail() {
    let engine_config = EngineConfig::default();

    let mut pairs = Vec::new();
    let mut uref_keys = Vec::new();

    const WASM_OFFSET: u64 = 1_000_000;
    const PACKAGE_OFFSET: u64 = 1_000;
    let root_key_name = "key".to_string();

    // create a long chain of urefs at address X with a uref that points to a uref X+1
    // which has a size that exceeds configured max query depth.
    for value in 1..=engine_config.max_query_depth {
        let uref_addr = val_to_hashaddr(value);
        let uref = Key::URef(URef::new(uref_addr, AccessRights::READ));

        let next_uref_addr = val_to_hashaddr(value + 1);
        let next_uref = Key::URef(URef::new(next_uref_addr, AccessRights::READ));
        let next_cl_value = StoredValue::CLValue(CLValue::from_t(next_uref).unwrap());

        pairs.push((uref, next_cl_value));
        uref_keys.push(uref);
    }

    let named_keys = {
        let mut named_keys = NamedKeys::new();
        named_keys.insert(root_key_name.clone(), uref_keys[0]);
        named_keys
    };
    let contract = StoredValue::Contract(Contract::new(
        val_to_hashaddr(PACKAGE_OFFSET).into(),
        val_to_hashaddr(WASM_OFFSET).into(),
        named_keys,
        EntryPoints::default(),
        ProtocolVersion::V1_0_0,
    ));
    let contract_key = Key::Hash([0; 32]);
    pairs.push((contract_key, contract));

    let correlation_id = CorrelationId::new();
    let (global_state, root_hash) =
        InMemoryGlobalState::from_pairs(correlation_id, &pairs).unwrap();

    let view = global_state.checkout(root_hash).unwrap().unwrap();
    let tracking_copy = TrackingCopy::new(view);

    // query for the beginning of a long chain of urefs
    // (second path element of arbitrary value required to cause iteration _into_ the nested key)
    let path = vec![root_key_name, String::new()];
    let result = tracking_copy.query(correlation_id, &engine_config, contract_key, &path);

    assert!(
        matches!(result, Ok(TrackingCopyQueryResult::DepthLimit {
        depth
    }) if depth == engine_config.max_query_depth),
        "{:?}",
        result
    );
}
