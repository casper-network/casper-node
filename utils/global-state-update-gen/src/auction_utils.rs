use std::collections::{BTreeMap, BTreeSet};

use casper_engine_test_support::LmdbWasmTestBuilder;
use casper_types::{
    system::auction::{
        Bid, SeigniorageRecipient, SeigniorageRecipientsSnapshot,
        SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY,
    },
    AsymmetricType, EraId, Key, PublicKey, StoredValue, U512,
};

use crate::utils::ValidatorsDiff;

/// Reads the `SeigniorageRecipientsSnapshot` stored in the global state.
pub fn read_snapshot(builder: &LmdbWasmTestBuilder) -> (Key, SeigniorageRecipientsSnapshot) {
    // Find the hash of the auction contract.
    let auction_contract_hash = builder.get_system_auction_hash();

    // Read the key under which the snapshot is stored.
    let validators_key = builder
        .get_contract(auction_contract_hash)
        .expect("auction should exist")
        .named_keys()[SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY];

    // Decode the old snapshot.
    let stored_value = builder
        .query(None, validators_key, &[])
        .expect("should query");
    let cl_value = stored_value
        .as_cl_value()
        .cloned()
        .expect("should be cl value");
    (validators_key, cl_value.into_t().expect("should convert"))
}

/// Generates a new `SeigniorageRecipientsSnapshot` based on:
/// - The list of validators, in format (validator_public_key,stake), both expressed as strings.
/// - The starting era ID (the era ID at which the snapshot should start).
/// - Count - the number of eras to be included in the snapshot.
pub fn gen_snapshot(
    validators: Vec<(String, String)>,
    starting_era_id: EraId,
    count: u64,
) -> SeigniorageRecipientsSnapshot {
    let mut new_snapshot = BTreeMap::new();
    let mut era_validators = BTreeMap::new();
    for (pub_key_str, bonded_amount_str) in &validators {
        let validator_pub_key = PublicKey::from_hex(pub_key_str.as_bytes()).unwrap();
        let bonded_amount = U512::from_dec_str(bonded_amount_str).unwrap();
        let seigniorage_recipient =
            SeigniorageRecipient::new(bonded_amount, Default::default(), Default::default());
        let _ = era_validators.insert(validator_pub_key, seigniorage_recipient);
    }
    for era_id in starting_era_id.iter(count) {
        let _ = new_snapshot.insert(era_id, era_validators.clone());
    }

    new_snapshot
}

/// Returns the set of public keys that have bids larger than the smallest bid among the new
/// validators.
pub fn find_large_bids(
    builder: &mut LmdbWasmTestBuilder,
    new_snapshot: &SeigniorageRecipientsSnapshot,
) -> BTreeSet<PublicKey> {
    let seigniorage_recipients = new_snapshot.values().next().unwrap();
    let min_bid = seigniorage_recipients
        .values()
        .map(SeigniorageRecipient::stake)
        .min()
        .unwrap();
    builder
        .get_bids()
        .into_iter()
        .filter(|(pkey, bid)| {
            bid.staked_amount() >= min_bid && !seigniorage_recipients.contains_key(pkey)
        })
        .map(|(pkey, _bid)| pkey)
        .collect()
}

/// Generates a set of writes necessary to "fix" the bids, ie.:
/// - set the bids of the new validators to their desired stakes,
/// - remove the bids of the old validators that are no longer validators,
/// - remove all the bids that are larger than the smallest bid among the new validators
/// (necessary, because such bidders would outbid the validators decided by the social consensus).
pub fn generate_entries_removing_bids(
    builder: &mut LmdbWasmTestBuilder,
    validators_diff: &ValidatorsDiff,
    new_snapshot: &SeigniorageRecipientsSnapshot,
) -> BTreeMap<Key, StoredValue> {
    let large_bids = find_large_bids(builder, new_snapshot);
    let to_unbid = validators_diff.removed.union(&large_bids);

    validators_diff
        .added
        .iter()
        .map(|pkey| {
            let amount = *new_snapshot
                .values()
                .next()
                .unwrap()
                .get(pkey)
                .unwrap()
                .stake();
            let account_hash = pkey.to_account_hash();
            let account = builder.get_account(account_hash).unwrap();
            (
                Key::Bid(account_hash),
                Bid::unlocked(
                    pkey.clone(),
                    account.main_purse(),
                    amount,
                    Default::default(),
                )
                .into(),
            )
        })
        .chain(to_unbid.into_iter().map(|pkey| {
            let account_hash = pkey.to_account_hash();
            let account = builder.get_account(account_hash).unwrap();
            (
                Key::Bid(account_hash),
                Bid::empty(pkey.clone(), account.main_purse()).into(),
            )
        }))
        .collect()
}

/// Generates the writes to the global state that will remove the pending withdraws of all the old
/// validators that will cease to be validators.
pub fn generate_entries_removing_withdraws(
    builder: &mut LmdbWasmTestBuilder,
    validators_diff: &ValidatorsDiff,
) -> BTreeMap<Key, StoredValue> {
    let withdraws = builder.get_unbonds();
    let withdraw_keys: BTreeSet<_> = withdraws.keys().collect();
    validators_diff
        .removed
        .iter()
        .map(PublicKey::to_account_hash)
        .filter(|acc| withdraw_keys.contains(&acc))
        .map(|acc| (Key::Withdraw(acc), StoredValue::Withdraw(vec![])))
        .collect()
}
