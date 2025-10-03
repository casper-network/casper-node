//! Unit tests for the data access layer component.

use casper_types::{
    system::auction::{Bid, BidAddr, BidKind, Unbond, UnbondEra, UnbondKind, UnbondingPurse},
    testing::TestRng,
    EraId, Key, PublicKey, StoredValue, URefAddr, OS_PAGE_SIZE, U512,
};
use lmdb::DatabaseFlags;
use rand::Rng;
use tempfile::TempDir;

use crate::{
    data_access_layer::{
        bids::{ValidatorBidRequest, ValidatorBidsResult},
        BlockStore, DataAccessLayer,
    },
    global_state::{
        state::{lmdb::LmdbGlobalState, CommitProvider, ScratchProvider, StateProvider},
        transaction_source::lmdb::LmdbEnvironment,
        trie_store::lmdb::LmdbTrieStore,
        DEFAULT_MAX_READERS,
    },
};
use std::{
    collections::BTreeSet,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
/// This is appended to the data dir path provided to the `LmdbWasmTestBuilder`".
const GLOBAL_STATE_DIR: &str = "global_state";
pub(crate) const DEFAULT_LMDB_PAGES: usize = 256_000_000;

#[test]
fn validator_method_fetches_bids_related_to_validator() {
    let mut test_rng = TestRng::new();
    let dal = build_data_access_layer();
    let scratch = dal.get_scratch_global_state();
    let prestate_hash = scratch.empty_root_hash;

    let (validator_public_key_1, validator_bid_key, validator_bid) =
        random_validator_bid(&mut test_rng);
    let (unified_bid_key, unified_bid) =
        unified_bid_for_public_key(validator_public_key_1.clone(), &mut test_rng);
    let (_, validator_bid_key_2, validator_bid_2) = random_validator_bid(&mut test_rng);
    let (_, delegated_account_bid_key, delegated_account_bid) =
        random_delegated_account_bid(validator_public_key_1.clone(), &mut test_rng);
    let (_, delegated_purse_bid_key, delegated_purse_bid) =
        random_delegated_purse_bid(validator_public_key_1.clone(), &mut test_rng);
    let (credit_key, credit_bid) =
        random_credit_for_public_key(validator_public_key_1.clone(), &mut test_rng);
    let (account_reservation_key, account_reservation_bid) =
        random_reservation_for_account_delegation(validator_public_key_1.clone(), &mut test_rng);
    let (purse_reservation_key, purse_reservation_bid) =
        random_reservation_for_purse_delegation(validator_public_key_1.clone(), &mut test_rng);
    let (unbond_account_key, unbond_account_bid) =
        random_unbond_account(validator_public_key_1.clone(), &mut test_rng);
    let (unbond_purse_key, unbond_purse_bid) =
        random_unbond_purse(validator_public_key_1.clone(), &mut test_rng);

    scratch
        .commit_values(
            prestate_hash,
            vec![
                (
                    validator_bid_key,
                    StoredValue::BidKind(validator_bid.clone()),
                ),
                (
                    validator_bid_key_2,
                    StoredValue::BidKind(validator_bid_2.clone()),
                ),
                (unified_bid_key, StoredValue::BidKind(unified_bid)),
                (credit_key, StoredValue::BidKind(credit_bid.clone())),
                (
                    delegated_account_bid_key,
                    StoredValue::BidKind(delegated_account_bid.clone()),
                ),
                (
                    delegated_purse_bid_key,
                    StoredValue::BidKind(delegated_purse_bid.clone()),
                ),
                (
                    account_reservation_key,
                    StoredValue::BidKind(account_reservation_bid.clone()),
                ),
                (
                    purse_reservation_key,
                    StoredValue::BidKind(purse_reservation_bid.clone()),
                ),
                (
                    unbond_account_key,
                    StoredValue::BidKind(unbond_account_bid.clone()),
                ),
                (
                    unbond_purse_key,
                    StoredValue::BidKind(unbond_purse_bid.clone()),
                ),
            ],
            BTreeSet::new(),
        )
        .unwrap();
    let new_state_root = dal.write_scratch_to_db(prestate_hash, scratch).unwrap();
    let bids_result = dal.validator_bids(ValidatorBidRequest::new(
        new_state_root,
        validator_public_key_1,
    ));
    assert_eq!(
        ValidatorBidsResult::Success {
            bids: vec![
                validator_bid,
                delegated_account_bid,
                delegated_purse_bid,
                credit_bid,
                account_reservation_bid,
                purse_reservation_bid,
                unbond_account_bid,
                unbond_purse_bid
            ]
        },
        bids_result
    )
}

#[test]
fn validator_method_fetches_historic_bids_fitted_to_new_structure() {
    let mut test_rng = TestRng::new();
    let dal = build_data_access_layer();
    let scratch = dal.get_scratch_global_state();
    let prestate_hash = scratch.empty_root_hash;

    let (validator_public_key_1, validator_bid_key, validator_bid) =
        random_historic_validator_bid(&mut test_rng);
    let historic_unbond_key = Key::Unbond(validator_public_key_1.to_account_hash());
    let bonding_purse_1 = test_rng.gen();
    let unbonder_public_key_1: PublicKey = test_rng.gen();
    let validator_public_key_2: PublicKey = test_rng.gen();
    let ub_1 = UnbondingPurse::new(
        bonding_purse_1,
        validator_public_key_1.clone(),
        unbonder_public_key_1.clone(),
        EraId::new(100),
        U512::from(555),
        Some(validator_public_key_2.clone()),
    );
    let bonding_purse_2 = test_rng.gen();
    let ub_2 = UnbondingPurse::new(
        bonding_purse_2,
        validator_public_key_1.clone(),
        validator_public_key_1.clone(),
        EraId::new(101),
        U512::from(556),
        None,
    );
    let unbonding_purses = vec![ub_1, ub_2];
    scratch
        .commit_values(
            prestate_hash,
            vec![
                (
                    validator_bid_key,
                    StoredValue::Bid(Box::new(validator_bid.clone())),
                ),
                (
                    historic_unbond_key,
                    StoredValue::Unbonding(unbonding_purses.clone()),
                ),
            ],
            BTreeSet::new(),
        )
        .unwrap();
    let new_state_root = dal.write_scratch_to_db(prestate_hash, scratch).unwrap();
    let bids_result = dal.validator_bids(ValidatorBidRequest::new(
        new_state_root,
        validator_public_key_1.clone(),
    ));

    let expected_validator_bid_kind = BidKind::Unified(Box::new(validator_bid));

    let expected_unbond_bid_kind = BidKind::Unbond(Box::new(Unbond::new(
        validator_public_key_1.clone(),
        UnbondKind::DelegatedPublicKey(unbonder_public_key_1),
        vec![UnbondEra::new(
            bonding_purse_1,
            EraId::new(100),
            U512::from(555),
            Some(validator_public_key_2),
        )],
    )));
    let expected_unbond_bid_kind_2 = BidKind::Unbond(Box::new(Unbond::new(
        validator_public_key_1.clone(),
        UnbondKind::Validator(validator_public_key_1),
        vec![UnbondEra::new(
            bonding_purse_2,
            EraId::new(101),
            U512::from(556),
            None,
        )],
    )));

    assert_eq!(
        ValidatorBidsResult::Success {
            bids: vec![
                expected_validator_bid_kind,
                expected_unbond_bid_kind_2,
                expected_unbond_bid_kind,
            ]
        },
        bids_result
    )
}

fn build_data_access_layer() -> DataAccessLayer<LmdbGlobalState> {
    let data_dir = TempDir::new().expect("should create temp dir");
    let global_state_dir = global_state_dir(data_dir.path());
    create_global_state_dir(&global_state_dir);
    let page_size = *OS_PAGE_SIZE;
    let environment = Arc::new(
        LmdbEnvironment::new(
            &global_state_dir,
            page_size * DEFAULT_LMDB_PAGES,
            DEFAULT_MAX_READERS,
            true,
        )
        .expect("should create LmdbEnvironment"),
    );
    let trie_store = Arc::new(
        LmdbTrieStore::new(&environment, None, DatabaseFlags::empty())
            .expect("should create LmdbTrieStore"),
    );
    let global_state = LmdbGlobalState::empty(environment, trie_store, 5, false)
        .expect("should create LmdbGlobalState");

    DataAccessLayer {
        block_store: BlockStore::new(),
        state: global_state,
        max_query_depth: 5,
        addressable_entity_enabled: false,
    }
}

fn global_state_dir<T: AsRef<OsStr> + ?Sized>(data_dir: &T) -> PathBuf {
    let mut path = PathBuf::from(data_dir);
    path.push(GLOBAL_STATE_DIR);
    path
}

fn create_global_state_dir<T: AsRef<Path>>(global_state_path: T) {
    fs::create_dir_all(&global_state_path).unwrap_or_else(|_| {
        panic!(
            "Expected to create {}",
            global_state_path.as_ref().display()
        )
    });
}

fn unified_bid_for_public_key(public_key: PublicKey, rng: &mut TestRng) -> (Key, BidKind) {
    let validator_bid_key = Key::BidAddr(BidAddr::Unified(public_key.to_account_hash()));
    let validator_bid = BidKind::random_unified_bid_public_key(rng, public_key);
    (validator_bid_key, validator_bid)
}

fn random_historic_validator_bid(rng: &mut TestRng) -> (PublicKey, Key, Bid) {
    let public_key = PublicKey::random(rng);
    let validator_bid_key = Key::Bid(public_key.to_account_hash());
    let validator_bid = Bid::random_for_public_key(rng, public_key.clone());
    (public_key, validator_bid_key, validator_bid)
}

fn random_validator_bid(rng: &mut TestRng) -> (PublicKey, Key, BidKind) {
    let public_key = PublicKey::random(rng);
    let validator_bid_key = Key::BidAddr(BidAddr::Validator(public_key.to_account_hash()));
    let validator_bid = BidKind::random_validator_bid_public_key(rng, public_key.clone());
    (public_key, validator_bid_key, validator_bid)
}

fn random_delegated_account_bid(
    validator_public_key: PublicKey,
    rng: &mut TestRng,
) -> (PublicKey, Key, BidKind) {
    let delegator_public_key = PublicKey::random(rng);
    let key = Key::BidAddr(BidAddr::DelegatedAccount {
        validator: validator_public_key.to_account_hash(),
        delegator: delegator_public_key.to_account_hash(),
    });
    let validator_bid =
        BidKind::random_delegated_account(rng, validator_public_key, delegator_public_key.clone());
    (delegator_public_key, key, validator_bid)
}

fn random_delegated_purse_bid(
    validator_public_key: PublicKey,
    rng: &mut TestRng,
) -> (URefAddr, Key, BidKind) {
    let delegator = rng.gen();

    let key = Key::BidAddr(BidAddr::DelegatedPurse {
        validator: validator_public_key.to_account_hash(),
        delegator,
    });
    let validator_bid = BidKind::random_delegated_purse(rng, validator_public_key, delegator);
    (delegator, key, validator_bid)
}

fn random_credit_for_public_key(
    validator_public_key: PublicKey,
    rng: &mut TestRng,
) -> (Key, BidKind) {
    let era_id = EraId::new(rng.gen());
    let key = Key::BidAddr(BidAddr::Credit {
        validator: validator_public_key.to_account_hash(),
        era_id,
    });
    let validator_bid = BidKind::random_credit(rng);
    (key, validator_bid)
}

fn random_reservation_for_account_delegation(
    validator_public_key: PublicKey,
    rng: &mut TestRng,
) -> (Key, BidKind) {
    let key = Key::BidAddr(BidAddr::ReservedDelegationAccount {
        validator: validator_public_key.to_account_hash(),
        delegator: rng.gen(),
    });
    let validator_bid = BidKind::random_reserved_delegation_for_account(rng);
    (key, validator_bid)
}

fn random_reservation_for_purse_delegation(
    validator_public_key: PublicKey,
    rng: &mut TestRng,
) -> (Key, BidKind) {
    let key = Key::BidAddr(BidAddr::ReservedDelegationPurse {
        validator: validator_public_key.to_account_hash(),
        delegator: rng.gen(),
    });
    let validator_bid = BidKind::random_reserved_delegation_for_purse(rng);
    (key, validator_bid)
}

fn random_unbond_account(validator_public_key: PublicKey, rng: &mut TestRng) -> (Key, BidKind) {
    let key = Key::BidAddr(BidAddr::UnbondAccount {
        validator: validator_public_key.to_account_hash(),
        unbonder: rng.gen(),
    });
    let validator_bid = BidKind::random_unbond(rng);
    (key, validator_bid)
}

fn random_unbond_purse(validator_public_key: PublicKey, rng: &mut TestRng) -> (Key, BidKind) {
    let key = Key::BidAddr(BidAddr::UnbondPurse {
        validator: validator_public_key.to_account_hash(),
        unbonder: rng.gen(),
    });
    let validator_bid = BidKind::random_unbond(rng);
    (key, validator_bid)
}
